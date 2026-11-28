//! Built-in check of the whole camera, so that nobody has to run a test
//! protocol by hand: flash, assemble, power on, open the page, done.
//!
//! - [`run_selftest`] (about a minute): one real full-resolution photo with
//!   its RAW, written to the USB key and read back, free memory, clock,
//!   power supply. Run once by itself on the first visit, again on demand.
//! - [`start_wake_test`] (Raspberry Pi 5): power off and wake up 3 minutes
//!   later by the RTC alarm. The verdict is written at the next boot
//!   ([`check_wake_marker`]), so it also catches a battery that cuts its
//!   output while the board sleeps.
//! - [`get_report`]: one text file with everything useful to diagnose a
//!   camera from afar (versions, checks, results, recent logs).

use std::path::Path as FsPath;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::web::api::{capture_in_progress, err, require_system_actions, ApiError};
use crate::web::AppState;

/// Minutes between the power-off and the RTC alarm of the wake test.
pub const WAKE_TEST_DELAY_SECS: i64 = 180;
/// A RAW of the HQ Camera (12.3 Mpx, 12 bits) is about 24 MB: much less
/// means a truncated file.
const MIN_DNG_BYTES: usize = 10_000_000;
/// Below this, a night of RAW risks falling behind (24 MB per photo).
const MIN_USB_MB_PER_S: f64 = 5.0;
/// Free memory below which the board is too short (Pi Zero, many tabs...).
const MIN_FREE_MEMORY_BYTES: u64 = 100_000_000;
/// A capture slower than this is reported (slow board or camera retries).
const SLOW_CAPTURE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Step {
    pub id: String,
    /// "ok", "warn" or "error".
    pub level: String,
    pub title: String,
    pub detail: String,
}

fn step(id: &str, level: &str, title: impl Into<String>, detail: impl Into<String>) -> Step {
    Step { id: id.into(), level: level.into(), title: title.into(), detail: detail.into() }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SelfTest {
    pub version: String,
    pub board: String,
    /// Local date and time of the check.
    pub at: String,
    /// Worst level of the steps.
    pub level: String,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WakeResult {
    pub ok: bool,
    pub at: String,
    pub detail: String,
}

/// What is kept on the SD card between visits and boots.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Stored {
    pub test: Option<SelfTest>,
    pub wake: Option<WakeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct WakeMarker {
    /// Unix seconds of the programmed alarm.
    scheduled: i64,
}

pub fn load(path: &FsPath) -> Stored {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(path: &FsPath, stored: &Stored) {
    if let Ok(json) = serde_json::to_vec_pretty(stored) {
        if let Err(e) = crate::core::config::write_atomic(path, &json, 0o600) {
            tracing::warn!("Résultat de vérification non enregistré: {}", e);
        }
    }
}

/// Worst level of a list of steps.
pub fn overall(steps: &[Step]) -> String {
    if steps.iter().any(|s| s.level == "error") {
        "error"
    } else if steps.iter().any(|s| s.level == "warn") {
        "warn"
    } else {
        "ok"
    }
    .to_string()
}

/// `MemAvailable` of `/proc/meminfo`, in bytes.
pub fn parse_mem_available(meminfo: &str) -> Option<u64> {
    meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemAvailable:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        .map(|kb| kb * 1024)
}

/// Verdict of `vcgencmd get_throttled` (bit 0: under-voltage now, bit 16:
/// under-voltage since boot).
pub fn power_step(flags: u32) -> Step {
    if flags & 0x1 != 0 {
        step("power", "error", "Alimentation trop faible",
            "Utilisez une batterie ou une alimentation 5 V / 3 A avec un câble court et épais.")
    } else if flags & 0x10000 != 0 {
        step("power", "warn", "Baisse de tension depuis le démarrage",
            "La batterie ou le câble sont limites : risque d'arrêt pendant la nuit.")
    } else {
        step("power", "ok", "Alimentation correcte", "Aucune baisse de tension depuis le démarrage.")
    }
}

/// Did the board start by itself at the alarm? `boot` is the Unix time of
/// the boot (now minus uptime).
pub fn wake_verdict(scheduled: i64, boot: i64, at: &str) -> WakeResult {
    let late = boot - scheduled;
    if boot < 1_704_067_200 {
        WakeResult { ok: false, at: at.into(), detail: "Heure inconnue au redémarrage : la pile de l'horloge est-elle branchée ?".into() }
    } else if (-60..=300).contains(&late) {
        WakeResult { ok: true, at: at.into(), detail: format!("Rallumé seul à l'heure prévue ({:+} s).", late) }
    } else if late > 300 {
        WakeResult {
            ok: false,
            at: at.into(),
            detail: format!(
                "Pas de rallumage seul : démarré {} min après l'heure prévue, sans doute à la main. \
                 Sur batterie, elle coupe peut-être sa sortie quand le Pi est éteint.",
                late / 60
            ),
        }
    } else {
        WakeResult { ok: false, at: at.into(), detail: "Démarré avant l'heure prévue : test interrompu (débranché ?).".into() }
    }
}

fn now_text() -> String {
    crate::core::time::now_local().strftime("%d/%m/%Y %H:%M").to_string()
}

fn board_model() -> String {
    std::fs::read("/proc/device-tree/model")
        .map(|m| String::from_utf8_lossy(&m).trim_end_matches('\0').trim().to_string())
        .unwrap_or_else(|_| if cfg!(feature = "rpi") { "Raspberry Pi".into() } else { "PC (dev)".into() })
}

/// One full-resolution photo with its RAW: the real chain of a night.
async fn capture_pair(state: &AppState, awb: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    if !cfg!(feature = "rpi") {
        // Simulated camera on PC: files of the right shape, small
        return Ok((vec![0xFF; 400_000], vec![0x2A; 2_000_000]));
    }
    let jpg = state.paths.tmp_dir.join("aurion_selftest.jpg");
    let dng = jpg.with_extension("dng");
    let _ = std::fs::remove_file(&jpg);
    let _ = std::fs::remove_file(&dng);
    let out = jpg.to_string_lossy().to_string();
    let result = crate::sys::run(
        "rpicam-still",
        &["--nopreview", "-o", &out, "-t", "100", "--shutter", "10000", "--gain", "1", "--awb", awb,
          "--thumb", "320:240:90", "--raw"],
        Duration::from_secs(40),
    )
    .await;
    let files = result.and_then(|_| {
        let j = std::fs::read(&jpg).map_err(|e| format!("JPEG absent : {}", e))?;
        let d = std::fs::read(&dng).map_err(|e| format!("RAW absent : {}", e))?;
        Ok((j, d))
    });
    let _ = std::fs::remove_file(&jpg);
    let _ = std::fs::remove_file(&dng);
    files
}

/// Write `data` on the key, flush it to the device, read it back.
/// Returns the write speed in MB/s.
fn usb_roundtrip(dir: &FsPath, data: &[u8]) -> Result<f64, String> {
    use std::io::Write;
    let path = dir.join(".aurion_autotest.bin");
    let start = Instant::now();
    let written = (|| {
        let mut f = std::fs::File::create(&path)?;
        f.write_all(data)?;
        f.sync_all()
    })();
    let secs = start.elapsed().as_secs_f64().max(0.001);
    let check = written
        .map_err(|e| format!("écriture impossible : {}", e))
        .and_then(|_| std::fs::read(&path).map_err(|e| format!("relecture impossible : {}", e)))
        .and_then(|back| if back == data { Ok(()) } else { Err("fichier relu différent de l'original".into()) });
    let _ = std::fs::remove_file(&path);
    check.map(|_| data.len() as f64 / 1e6 / secs)
}

/// The whole check. Never during a night, never twice at once.
pub async fn run_selftest(State(state): State<AppState>) -> Result<Json<Stored>, ApiError> {
    if capture_in_progress(state.current_phase().await) {
        return Err(err(StatusCode::CONFLICT, "Nuit en cours : la vérification se fait de jour"));
    }
    let Ok(_camera) = state.camera_lock.try_lock() else {
        return Err(err(StatusCode::CONFLICT, "La caméra est déjà utilisée (aperçu ou darks)"));
    };
    let config = state.config.read().await.clone();
    let mut steps = Vec::new();

    // 1. Camera: a real photo with its RAW
    let start = Instant::now();
    let pair = capture_pair(&state, &config.capture.awb).await;
    let took = start.elapsed();
    let mut sample = None;
    match pair {
        Err(e) => steps.push(step("camera", "error", "La caméra ne prend pas de photo",
            format!("{}. Débranchez le Raspberry Pi, vérifiez la nappe (sens et loquet) des deux côtés, puis rallumez.", e))),
        Ok((jpg, dng)) if cfg!(feature = "rpi") && dng.len() < MIN_DNG_BYTES => {
            steps.push(step("camera", "error", "Photo RAW incomplète",
                format!("RAW de {:.1} Mo au lieu d'environ 24 Mo (JPEG {:.1} Mo).", dng.len() as f64 / 1e6, jpg.len() as f64 / 1e6)));
        }
        Ok((jpg, dng)) => {
            let detail = format!("JPEG {:.1} Mo et RAW {:.1} Mo en {:.1} s.", jpg.len() as f64 / 1e6, dng.len() as f64 / 1e6, took.as_secs_f64());
            if took > SLOW_CAPTURE {
                steps.push(step("camera", "warn", "Photo pleine résolution lente", detail));
            } else {
                steps.push(step("camera", "ok", "Photo pleine résolution et RAW", detail));
            }
            sample = Some(dng);
        }
    }

    // 2. USB key: the RAW written, flushed and read back
    let mount = FsPath::new(&config.storage.mount_point).to_path_buf();
    let health = crate::sys::storage_health(&mount);
    if !(health.writable && (health.is_mountpoint || !cfg!(feature = "rpi"))) {
        steps.push(step("usb", "error", "Clé USB absente ou illisible",
            "Branchez une clé USB, ou appuyez sur « Préparer la clé » sur l'accueil."));
    } else {
        let data = sample.clone().unwrap_or_else(|| vec![0x2A; 8_000_000]);
        let m = mount.clone();
        match tokio::task::spawn_blocking(move || usb_roundtrip(&m, &data)).await.unwrap_or_else(|e| Err(e.to_string())) {
            Err(e) => steps.push(step("usb", "error", "La clé USB n'enregistre pas correctement",
                format!("{}. Essayez une autre clé.", e))),
            Ok(speed) if cfg!(feature = "rpi") && speed < MIN_USB_MB_PER_S => steps.push(step("usb", "warn", "Clé USB lente",
                format!("{:.1} Mo/s : une photo RAW prend plus de 5 s à enregistrer. Une clé USB 3 de marque est conseillée.", speed))),
            Ok(speed) => {
                let free = crate::sys::disk_usage(&mount).map(|(_, f)| format!(", {:.0} Go libres", f as f64 / 1e9)).unwrap_or_default();
                steps.push(step("usb", "ok", "Clé USB : écriture et relecture", format!("{:.0} Mo/s{}.", speed, free)));
            }
        }
    }

    // 3. Memory left after a full-resolution photo
    if let Some(avail) = std::fs::read_to_string("/proc/meminfo").ok().as_deref().and_then(parse_mem_available) {
        let mb = avail as f64 / 1e6;
        if avail < MIN_FREE_MEMORY_BYTES {
            steps.push(step("memory", "warn", "Mémoire presque pleine", format!("{:.0} Mo libres.", mb)));
        } else {
            steps.push(step("memory", "ok", "Mémoire suffisante", format!("{:.0} Mo libres.", mb)));
        }
    }

    // 4. Clock
    let now = crate::core::time::now_local();
    let from_rtc = state.clock_from_rtc.load(std::sync::atomic::Ordering::Relaxed);
    let hour = now.strftime("%d/%m/%Y %H:%M").to_string();
    if crate::core::time::clock_unset() {
        steps.push(step("clock", "error", "Heure non réglée", "Rechargez la page depuis le téléphone : l'heure se règle seule."));
    } else if from_rtc {
        steps.push(step("clock", "ok", format!("Heure : {}", hour), "Horloge matérielle : juste même sans téléphone."));
    } else if state.clock_trusted() {
        steps.push(step("clock", "ok", format!("Heure : {}", hour), "Réglée par le téléphone."));
    } else {
        steps.push(step("clock", "warn", format!("Heure à confirmer : {}", hour), "Ouvrez la page depuis le téléphone pour la régler."));
    }

    // 5. Power supply (Raspberry Pi only)
    if let Ok(out) = crate::sys::run("vcgencmd", &["get_throttled"], Duration::from_secs(3)).await {
        steps.push(power_step(u32::from_str_radix(out.trim().trim_start_matches("throttled=0x"), 16).unwrap_or(0)));
    }

    // 6. Automatic wake-up (Raspberry Pi 5)
    let mut stored = load(&state.paths.selftest);
    if crate::sys::rtc_info().wake_capable {
        steps.push(match &stored.wake {
            Some(w) if w.ok => step("wake", "ok", "Rallumage automatique vérifié", format!("{} ({}).", w.detail, w.at)),
            Some(w) => step("wake", "error", "Rallumage automatique en échec", format!("{} ({}). Relancez le test dans Diagnostics.", w.detail, w.at)),
            None => step("wake", "warn", "Rallumage automatique pas encore testé",
                "Pour les nuits sans toucher la caméra : Diagnostics, « Tester le rallumage » (3 minutes)."),
        });
    }

    let test = SelfTest {
        version: crate::VERSION.to_string(),
        board: board_model(),
        at: now_text(),
        level: overall(&steps),
        steps,
    };
    state.add_log(format!("Vérification d'Aurion : {}", match test.level.as_str() {
        "ok" => "tout est prêt",
        "warn" => "prêt, avec des conseils",
        _ => "problème à corriger",
    }))
    .await;
    stored.test = Some(test);
    save(&state.paths.selftest, &stored);
    Ok(Json(stored))
}

/// Last results (empty before the first check).
pub async fn get_selftest(State(state): State<AppState>) -> Json<Stored> {
    Json(load(&state.paths.selftest))
}

/// Raspberry Pi 5: power off now, wake up by the RTC alarm 3 minutes later.
pub async fn start_wake_test(State(state): State<AppState>) -> Result<Json<serde_json::Value>, ApiError> {
    require_system_actions(&state)?;
    if capture_in_progress(state.current_phase().await) {
        return Err(err(StatusCode::CONFLICT, "Nuit en cours : le test se fait de jour"));
    }
    if !crate::sys::rtc_info().wake_capable {
        return Err(err(StatusCode::CONFLICT, "Rallumage automatique indisponible : Raspberry Pi 5 requis"));
    }
    let scheduled = crate::core::time::Timestamp::now().as_second() + WAKE_TEST_DELAY_SECS;
    crate::sys::helper(&["rtc-wake", &scheduled.to_string()], None, Duration::from_secs(10))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Réveil non programmé : {}", e)))?;
    let marker = serde_json::to_vec(&WakeMarker { scheduled }).unwrap_or_default();
    crate::core::config::write_atomic(&state.paths.wake_test, &marker, 0o600)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Test non enregistré : {}", e)))?;
    let at = crate::core::time::to_local(crate::core::time::Timestamp::from_second(scheduled).unwrap_or_default())
        .strftime("%H:%M")
        .to_string();
    state.add_log(format!("Test de rallumage : extinction, réveil programmé à {}", at)).await;
    let _ = crate::sys::run("sync", &[], Duration::from_secs(30)).await;
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Err(e) = crate::sys::helper(&["shutdown"], None, Duration::from_secs(30)).await {
            tracing::error!("Shutdown failed: {}", e);
        }
    });
    Ok(Json(serde_json::json!({ "wake_at": at })))
}

/// At boot: a wake test was pending, write its verdict.
pub fn check_wake_marker(state_paths: &crate::web::Paths, now: i64, uptime_secs: f64) -> Option<WakeResult> {
    let marker: WakeMarker = serde_json::from_slice(&std::fs::read(&state_paths.wake_test).ok()?).ok()?;
    let _ = std::fs::remove_file(&state_paths.wake_test);
    let verdict = wake_verdict(marker.scheduled, now - uptime_secs as i64, &now_text());
    let mut stored = load(&state_paths.selftest);
    stored.wake = Some(verdict.clone());
    save(&state_paths.selftest, &stored);
    Some(verdict)
}

/// One text file to send when something goes wrong.
pub async fn get_report(State(state): State<AppState>) -> Response {
    let stored = load(&state.paths.selftest);
    let diag = crate::web::api::get_diagnostics(State(state.clone())).await.0;
    let preflight = crate::web::api::get_preflight(State(state.clone())).await.0;
    let logs = state.log_buffer.read().await.clone();
    let mut r = String::new();
    r.push_str(&format!("Rapport Aurion du {}\n\n", now_text()));
    r.push_str(&format!("Version : {}\nCarte : {}\nPlateforme : {}\nHeure : {} {}\nFuseau : {}\nAlimentation (throttled) : {}\nTempérature : {}\n\n",
        diag.version, board_model(), diag.platform, diag.system_date, diag.system_time, diag.timezone,
        diag.throttled.unwrap_or_else(|| "-".into()),
        diag.cpu_temp.map(|t| format!("{:.0} °C", t)).unwrap_or_else(|| "-".into())));
    r.push_str("== Prêt pour la nuit ? ==\n");
    for c in &preflight.checks {
        r.push_str(&format!("[{}] {} : {}\n", c.level, c.title, c.detail));
    }
    r.push_str("\n== Dernière vérification ==\n");
    match &stored.test {
        Some(t) => {
            r.push_str(&format!("{} ({}, {})\n", t.at, t.version, t.level));
            for s in &t.steps {
                r.push_str(&format!("[{}] {} : {}\n", s.level, s.title, s.detail));
            }
        }
        None => r.push_str("jamais lancée\n"),
    }
    r.push_str("\n== Rallumage automatique ==\n");
    match &stored.wake {
        Some(w) => r.push_str(&format!("{} : {} {}\n", w.at, if w.ok { "OK" } else { "ÉCHEC" }, w.detail)),
        None => r.push_str("jamais testé\n"),
    }
    r.push_str("\n== Journal ==\n");
    for l in logs.iter().rev().take(200).rev() {
        r.push_str(l);
        r.push('\n');
    }
    let name = format!("aurion-rapport-{}.txt", crate::core::time::now_local().strftime("%Y-%m-%d_%H%M"));
    (
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", name)),
        ],
        r,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_and_parsing() {
        assert_eq!(overall(&[]), "ok");
        assert_eq!(overall(&[step("a", "ok", "", ""), step("b", "warn", "", "")]), "warn");
        assert_eq!(overall(&[step("a", "error", "", ""), step("b", "warn", "", "")]), "error");
        assert_eq!(parse_mem_available("MemTotal: 4000 kB\nMemAvailable:    123456 kB\n"), Some(123_456 * 1024));
        assert_eq!(parse_mem_available("MemTotal: 4000 kB\n"), None);
        assert_eq!(power_step(0).level, "ok");
        assert_eq!(power_step(0x50000).level, "warn");
        assert_eq!(power_step(0x50005).level, "error");
    }

    #[test]
    fn wake_verdicts() {
        let s = 1_800_000_000;
        assert!(wake_verdict(s, s + 40, "x").ok, "booted 40 s after the alarm");
        assert!(wake_verdict(s, s - 30, "x").ok, "clock slightly behind");
        assert!(!wake_verdict(s, s + 3_600, "x").ok, "powered on by hand an hour later");
        assert!(!wake_verdict(s, s - 600, "x").ok, "restarted before the alarm");
        assert!(!wake_verdict(s, 100, "x").ok, "clock lost at boot");
    }

    #[test]
    fn usb_roundtrip_checks_what_it_wrote() {
        let dir = tempfile::tempdir().unwrap();
        let speed = usb_roundtrip(dir.path(), &vec![7u8; 1_000_000]).unwrap();
        assert!(speed > 0.0);
        assert!(!dir.path().join(".aurion_autotest.bin").exists(), "test file removed");
        assert!(usb_roundtrip(&dir.path().join("missing"), b"x").is_err());
    }

    #[test]
    fn wake_marker_gives_a_verdict_once() {
        let dir = tempfile::tempdir().unwrap();
        let paths = crate::web::Paths::from_config_dir(dir.path());
        assert!(check_wake_marker(&paths, 1_800_000_200, 60.0).is_none(), "no test pending");
        std::fs::write(&paths.wake_test, br#"{"scheduled":1800000000}"#).unwrap();
        let v = check_wake_marker(&paths, 1_800_000_200, 160.0).unwrap();
        assert!(v.ok, "{}", v.detail);
        assert_eq!(load(&paths.selftest).wake, Some(v));
        assert!(check_wake_marker(&paths, 1_800_000_300, 60.0).is_none(), "verdict written once");
    }
}
