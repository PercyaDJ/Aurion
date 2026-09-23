use std::path::Path as FsPath;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Datelike;
use serde::{Deserialize, Serialize};

use crate::core::config::{AppConfig, Preset, DEFAULT_WIFI_PASSWORD, PASSWORD_MASK};
use crate::core::models::{check_star_trail_rule, Phase};
use crate::core::validate;
use crate::web::AppState;

/// Crop factor of the IMX477 sensor (Raspberry Pi HQ Camera).
const HQ_CAMERA_CROP_FACTOR: f64 = 5.6;

pub(crate) type ApiError = (StatusCode, String);

pub(crate) fn err(status: StatusCode, msg: impl Into<String>) -> ApiError {
    (status, msg.into())
}

/// Phases during which the night capture is running (camera busy,
/// clock and binary must not be touched).
pub(crate) fn capture_in_progress(phase: Phase) -> bool {
    matches!(phase, Phase::Disconnect | Phase::Calibration | Phase::Watch | Phase::Run)
}

pub(crate) fn require_system_actions(state: &AppState) -> Result<(), ApiError> {
    if state.system_actions {
        Ok(())
    } else {
        Err(err(
            StatusCode::NOT_IMPLEMENTED,
            "Action système indisponible sur cette plateforme (mode PC)",
        ))
    }
}

// ─── Status ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusResponse {
    pub phase: String,
    pub time: String,
    pub date: String,
    /// Device clock (Unix milliseconds), used by the UI to detect drift.
    pub epoch_ms: i64,
    pub storage: Option<StorageResponse>,
    pub warnings: Vec<String>,
    /// A USB drive is mounted on the capture directory and writable.
    pub usb_mounted: bool,
    /// The capture directory is writable (possibly on the SD card).
    pub usb_writable: bool,
    pub default_password: bool,
    pub time_synced: bool,
    /// Seconds before an interrupted night resumes on its own.
    pub resume_in_secs: Option<u64>,
}

pub async fn get_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let phase = state.current_phase().await;
    let config = state.config.read().await.clone();
    let now = chrono::Local::now();

    let mount_point = FsPath::new(&config.storage.mount_point);
    let health = crate::sys::storage_health(mount_point);
    // On the Pi a real USB mount is required; on a dev PC any writable
    // folder is fine.
    let usb_mounted = health.writable && (health.is_mountpoint || !cfg!(feature = "rpi"));

    let mut warnings = Vec::new();
    if !usb_mounted {
        warnings.push(if health.writable {
            "Clé USB non détectée : les images seraient enregistrées sur la carte SD".to_string()
        } else {
            "Clé USB absente ou en lecture seule : aucune image ne pourra être enregistrée".to_string()
        });
    }
    if now.year() < 2024 {
        warnings.push("Horloge non réglée : ouvrez l'interface depuis le téléphone pour la synchroniser".into());
    }
    if config.network.password == DEFAULT_WIFI_PASSWORD {
        warnings.push("Mot de passe Wi-Fi par défaut : changez-le dans Réglages avancés".into());
    }
    if config.exposure.iso_max > 1600 {
        warnings.push("ISO élevé : risque de bruit capteur".into());
    }
    if check_star_trail_rule(config.capture.focal_length_mm, config.exposure.shutter_max_us, HQ_CAMERA_CROP_FACTOR) {
        warnings.push("Règle des 500 dépassée : risque d'étoiles filées".into());
    }

    Json(StatusResponse {
        phase: phase.to_string(),
        time: now.format("%H:%M:%S").to_string(),
        date: now.format("%d/%m/%Y").to_string(),
        epoch_ms: now.timestamp_millis(),
        storage: storage_response(mount_point),
        warnings,
        usb_mounted,
        usb_writable: health.writable,
        default_password: config.network.password == DEFAULT_WIFI_PASSWORD,
        time_synced: state.time_synced.load(std::sync::atomic::Ordering::Relaxed),
        resume_in_secs: resume_in_secs(&state).await,
    })
}

/// USB keys plugged into the Pi, and the state of the capture drive.
pub async fn get_usb_devices(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mount = state.config.read().await.storage.mount_point.clone();
    let health = crate::sys::storage_health(FsPath::new(&mount));
    Json(serde_json::json!({
        "devices": crate::sys::usb_disks(),
        "mounted": health.is_mountpoint,
        "writable": health.writable,
    }))
}

#[derive(Deserialize)]
pub struct FormatRequest {
    pub device: String,
    /// Must be "EFFACER": the whole key is erased.
    pub confirm: String,
}

/// Prepare a USB key: erase it, one exFAT partition "AURION", mount it.
pub async fn format_usb(State(state): State<AppState>, Json(req): Json<FormatRequest>) -> Result<String, ApiError> {
    if req.confirm != "EFFACER" {
        return Err(err(StatusCode::BAD_REQUEST, "Confirmation manquante"));
    }
    let valid = req.device.len() == 3 && req.device.starts_with("sd") && req.device.as_bytes()[2].is_ascii_lowercase();
    if !valid {
        return Err(err(StatusCode::BAD_REQUEST, "Périphérique invalide"));
    }
    if state.current_phase().await != Phase::Arm {
        return Err(err(StatusCode::CONFLICT, "Impossible pendant une nuit"));
    }
    require_system_actions(&state)?;
    state.add_log(format!("Préparation de la clé {} (effacement, exFAT)…", req.device)).await;
    crate::sys::helper(&["usb-format", &req.device], None, Duration::from_secs(300))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Préparation impossible : {}", e)))?;
    // The mount is asynchronous (systemd-mount --no-block): wait for it
    let mount = state.config.read().await.storage.mount_point.clone();
    for _ in 0..30 {
        if crate::sys::storage_health(FsPath::new(&mount)).writable {
            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    // The settings are copied on the new key right away
    let _ = state.config.read().await.save_usb_backup();
    state.add_log("Clé prête".into()).await;
    Ok("Clé prête".into())
}

async fn resume_in_secs(state: &AppState) -> Option<u64> {
    state.resume_at.read().await.map(|at| at.saturating_duration_since(tokio::time::Instant::now()).as_secs())
}

/// Cancel the automatic resume of an interrupted night (back to normal ARM).
pub async fn cancel_resume(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    if state.resume_at.write().await.take().is_none() {
        return Err(err(StatusCode::CONFLICT, "Aucune reprise de nuit en attente"));
    }
    state.add_log("Reprise de la nuit interrompue annulée depuis le téléphone".into()).await;
    Ok(StatusCode::OK)
}

// ─── Pre-flight checks (home screen) ──────────────────────

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Check {
    pub id: &'static str,
    /// "ok", "warn" or "error" (error = the night cannot start)
    pub level: &'static str,
    pub title: String,
    pub detail: String,
    /// Button offered with the check ("format_usb": prepare the key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Preflight {
    /// No blocking error: the night can be started.
    pub ready: bool,
    pub phase: String,
    pub checks: Vec<Check>,
    /// Capture time the free space allows with the current settings.
    pub capacity_hours: Option<f64>,
    /// Planned duration of the night.
    pub planned_hours: f64,
    /// Seconds before an interrupted night resumes on its own.
    pub resume_in_secs: Option<u64>,
    /// Expedition mode enabled.
    pub expedition: bool,
    /// Expedition: seconds before the night starts on its own.
    pub auto_start_in_secs: Option<u64>,
    /// Expedition: why the night cannot start on its own yet.
    pub auto_start_blocked: Option<String>,
    /// Nights of capture the free space allows (planned duration).
    pub nights_capacity: Option<f64>,
    /// The board can switch itself on for the next night (Raspberry Pi 5).
    pub wake_capable: bool,
}

fn check(id: &'static str, level: &'static str, title: impl Into<String>, detail: impl Into<String>) -> Check {
    Check { id, level, title: title.into(), detail: detail.into(), action: None }
}

/// "4 h 48", "9 h": same wording as the web page.
pub fn fmt_hours(hours: f64) -> String {
    let total = (hours.max(0.0) * 60.0).round() as u64;
    match total % 60 {
        0 => format!("{} h", total / 60),
        m => format!("{} h {:02}", total / 60, m),
    }
}

/// Planned night duration in hours (timer, or time range crossing midnight).
pub fn planned_hours(config: &AppConfig) -> f64 {
    if let Some(h) = config.time_range.duration_hours {
        return h;
    }
    let start = config.time_range.start;
    let end = config.time_range.end;
    let secs = (end - start).num_seconds();
    let secs = if secs <= 0 { secs + 24 * 3600 } else { secs };
    secs as f64 / 3600.0
}

/// Estimated size of one capture, from the images already on the key
/// (average per type), or typical values for the IMX477 otherwise.
pub fn bytes_per_capture(config: &AppConfig, images: &[(String, u64)]) -> u64 {
    let avg = |ext: &str, fallback: u64| {
        let v: Vec<u64> = images
            .iter()
            // Files under 200 kB are not real captures (empty or truncated
            // file after a power cut): they would make the estimate absurd.
            .filter(|(n, size)| validate::extension_lower(n).as_deref() == Some(ext) && !n.contains("_STACK") && *size >= 200_000)
            .map(|(_, s)| *s)
            .collect();
        if v.is_empty() { fallback } else { v.iter().sum::<u64>() / v.len() as u64 }
    };
    // Typical: JPEG ~4 MB, night DNG 12 to 15 MB (field measurement);
    // replaced by the real sizes after the first night.
    let jpg = avg("jpg", 4_000_000);
    let dng = avg("dng", 14_000_000);
    use crate::core::models::OutputFormat::*;
    let thumb = 20_000;
    match config.capture.output_format {
        Jpg => jpg + thumb,
        RawDng => dng,
        RawAndJpg => jpg + dng + thumb,
        // RAW only during auroras: the JPEG stream is what fills the key
        // night after night (the RAW of the auroras are added on top).
        JpgAuroraRaw => jpg + thumb,
    }
}

/// Seconds `rpicam-still` needs beyond the exposure (camera start, files):
/// assumption until the first night gives the real throughput.
const CAPTURE_OVERHEAD_SECS: f64 = 2.0;

/// Hours of capture the free space allows without any measured night
/// (SAFE mode: one frame every pause + longest exposure + overhead).
pub fn capacity_hours(config: &AppConfig, free_bytes: u64, bytes_per_frame: u64) -> f64 {
    let period = config.capture.capture_interval_secs as f64 + config.exposure.shutter_max_us as f64 / 1e6 + CAPTURE_OVERHEAD_SECS;
    let frames = free_bytes as f64 / bytes_per_frame.max(1) as f64;
    round1(frames * period / 3600.0)
}

pub async fn camera_detected(state: &AppState) -> bool {
    if !cfg!(feature = "rpi") {
        return true; // simulated camera on PC
    }
    if let Some((at, ok)) = *state.camera_check.read().await {
        if at.elapsed() < Duration::from_secs(30) || capture_in_progress(state.current_phase().await) {
            return ok;
        }
    }
    let ok = crate::sys::run("rpicam-hello", &["--list-cameras"], Duration::from_secs(15))
        .await
        .map(|out| out.contains("Available cameras") && !out.contains("No cameras"))
        .unwrap_or(false);
    *state.camera_check.write().await = Some((std::time::Instant::now(), ok));
    ok
}

/// Everything the user must know before leaving the camera for the night,
/// in plain words, ordered by importance.
pub async fn get_preflight(State(state): State<AppState>) -> Json<Preflight> {
    let config = state.config.read().await.clone();
    let phase = state.current_phase().await;
    let mount = FsPath::new(&config.storage.mount_point).to_path_buf();
    let mut checks = Vec::new();

    // Camera
    if camera_detected(&state).await {
        checks.push(check("camera", "ok", "Caméra détectée", ""));
    } else {
        checks.push(check("camera", "error", "Caméra non détectée",
            "Débranchez le Raspberry Pi, vérifiez la nappe (sens et loquet) des deux côtés, puis rallumez."));
    }

    // USB key and capacity
    let health = crate::sys::storage_health(&mount);
    let usb_ok = health.writable && (health.is_mountpoint || !cfg!(feature = "rpi"));
    let planned = planned_hours(&config);
    let mut capacity = None;
    if !usb_ok {
        let plugged = crate::sys::usb_disks();
        if let Some(disk) = plugged.first() {
            // A key is plugged but not usable: unformatted, or ext4 / NTFS...
            let mut c = check("usb", "error", "Clé USB à préparer",
                format!("La clé {} ({:.0} Go) n'est pas lisible par Aurion. « Préparer la clé » l'efface et la formate en exFAT.",
                    disk.model, disk.size_bytes as f64 / 1e9));
            c.action = Some("format_usb");
            checks.push(c);
        } else {
            checks.push(check("usb", "error", "Clé USB absente",
                "Branchez une clé USB (512 Go conseillés pour plusieurs nuits de RAW)."));
        }
    } else if let Some((_, free)) = crate::sys::disk_usage(&mount) {
        let m = mount.clone();
        let images: Vec<(String, u64)> = tokio::task::spawn_blocking(move || {
            crate::web::gallery::recent_image_sizes(&m, 60)
        })
        .await
        .unwrap_or_default();
        // Measured throughput of the last night first, model otherwise
        let m2 = mount.clone();
        let rate = tokio::task::spawn_blocking(move || crate::web::gallery::recent_night_rate(&m2)).await.ok().flatten();
        let hours = match rate {
            Some(bytes_per_hour) if bytes_per_hour > 0.0 => round1(free as f64 / bytes_per_hour),
            _ => capacity_hours(&config, free, bytes_per_capture(&config, &images)),
        };
        capacity = Some(hours);
        let free_gb = free as f64 / 1e9;
        if hours < planned {
            checks.push(check("usb", "warn", format!("Place limitée : environ {} de capture", fmt_hours(hours)),
                format!("{:.0} Go libres pour une nuit prévue de {}. Libérez de la place (Photos) ou espacez les prises.", free_gb, fmt_hours(planned))));
        } else {
            checks.push(check("usb", "ok", format!("Clé USB : {:.0} Go libres", free_gb),
                format!("Environ {} de capture possibles.", fmt_hours(hours))));
        }
    }

    // Clock
    let now = chrono::Local::now();
    let from_rtc = state.clock_from_rtc.load(std::sync::atomic::Ordering::Relaxed);
    if now.year() < 2024 {
        checks.push(check("clock", "error", "Heure non réglée",
            "Rechargez cette page depuis le téléphone : l'heure se règle automatiquement."));
    } else if from_rtc {
        checks.push(check("clock", "ok", format!("Heure : {}", now.format("%H:%M")), "Horloge matérielle : l'heure est juste même sans téléphone."));
    } else if state.clock_trusted() {
        checks.push(check("clock", "ok", format!("Heure : {}", now.format("%H:%M")), "Réglée par le téléphone."));
    } else if config.expedition.enabled {
        checks.push(check("clock", "warn", format!("Heure à confirmer : {}", now.format("%H:%M")),
            "Ce Raspberry Pi n'a pas d'horloge sauvegardée : l'heure se règle quand un téléphone ouvre cette page. \
             Pour des nuits sans téléphone, un Raspberry Pi 5 ou un module horloge (RTC DS3231) est nécessaire."));
    } else {
        checks.push(check("clock", "ok", format!("Heure : {}", now.format("%H:%M")), ""));
    }

    // Power supply and temperature (Raspberry Pi only)
    if let Ok(out) = crate::sys::run("vcgencmd", &["get_throttled"], Duration::from_secs(3)).await {
        let flags = u32::from_str_radix(out.trim().trim_start_matches("throttled=0x"), 16).unwrap_or(0);
        if flags & 0x1 != 0 {
            checks.push(check("power", "error", "Alimentation trop faible",
                "Utilisez une batterie ou une alimentation 5 V / 3 A avec un câble court et épais."));
        } else if flags & 0x10000 != 0 {
            checks.push(check("power", "warn", "Baisse de tension détectée depuis le démarrage",
                "La batterie ou le câble sont limites : risque d'arrêt pendant la nuit."));
        } else {
            checks.push(check("power", "ok", "Alimentation correcte", ""));
        }
    }
    if let Some(t) = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok().and_then(|s| s.trim().parse::<f64>().ok()) {
        if t / 1000.0 > 75.0 {
            checks.push(check("temperature", "warn", format!("Processeur chaud ({:.0} °C)", t / 1000.0),
                "Placez le boîtier à l'ombre et à l'air : la chaleur augmente le bruit des photos."));
        }
    }

    // Security
    if config.network.password == DEFAULT_WIFI_PASSWORD {
        checks.push(check("password", "warn", "Mot de passe Wi-Fi d'usine",
            "Choisissez votre propre mot de passe pour que personne d'autre ne puisse piloter la caméra."));
    }

    // Expedition: how many nights fit, and how the next nights start
    let wake_capable = crate::sys::rtc_info().wake_capable;
    let nights_capacity = capacity.filter(|_| planned > 0.0).map(|h| round1(h / planned));
    if config.expedition.enabled {
        let nights = nights_capacity.map(|n| format!("environ {:.0} nuit(s) de place sur la clé", n.floor())).unwrap_or_default();
        let how = if wake_capable {
            "Raspberry Pi 5 : il s'éteint le matin et se rallume seul chaque soir."
        } else {
            "Chaque soir : branchez la batterie, la nuit démarre seule 5 min après (heure juste requise)."
        };
        let raw_note = if matches!(config.capture.output_format, crate::core::models::OutputFormat::JpgAuroraRaw) {
            " Les RAW des aurores s'ajoutent à cette estimation."
        } else {
            ""
        };
        checks.push(check("expedition", "ok", format!("Mode expédition{}{}", if nights.is_empty() { "" } else { " : " }, nights),
            format!("{}{}", how, raw_note)));
    }

    let (auto_start_in_secs, auto_start_blocked) = match *state.auto_start.read().await {
        crate::web::AutoStart::At(at) => (Some(at.saturating_duration_since(tokio::time::Instant::now()).as_secs()), None),
        crate::web::AutoStart::Blocked(why) => (None, Some(why.to_string())),
        crate::web::AutoStart::Off => (None, None),
    };
    let ready = !checks.iter().any(|c| c.level == "error");
    Json(Preflight {
        ready,
        phase: phase.to_string(),
        checks,
        capacity_hours: capacity,
        planned_hours: round1(planned),
        resume_in_secs: resume_in_secs(&state).await,
        expedition: config.expedition.enabled,
        auto_start_in_secs,
        auto_start_blocked,
        nights_capacity,
        wake_capable,
    })
}

// ─── Preview ───────────────────────────────────────────────

pub async fn get_preview(State(state): State<AppState>) -> impl IntoResponse {
    let preview = state.latest_preview.read().await;
    match preview.as_ref() {
        Some(data) => (StatusCode::OK, [("content-type", "image/jpeg")], data.clone()).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

async fn rpicam_still(
    output: &FsPath,
    settings: &crate::core::models::ExposureSettings,
    awb: &str,
) -> Result<Vec<u8>, String> {
    let shutter = settings.shutter_us.to_string();
    let gain = format!("{:.2}", settings.iso as f64 / 100.0);
    let out = output.to_string_lossy().to_string();
    let timeout = Duration::from_secs(settings.shutter_us.div_ceil(1_000_000) * 3 + 20);
    crate::sys::run(
        "rpicam-still",
        &["--nopreview", "-o", &out, "-t", "100", "--shutter", &shutter, "--gain", &gain, "--awb", awb],
        timeout,
    )
    .await?;
    tokio::fs::read(output).await.map_err(|e| format!("Lecture échouée: {}", e))
}

/// Capture a preview image with a quick auto-exposure (up to 4 shots):
/// start at the geometric middle of the user's range, meter the sky
/// (ROI only) and jump towards the target brightness.
pub async fn capture_preview(State(state): State<AppState>) -> Response {
    if capture_in_progress(state.current_phase().await) {
        return err(StatusCode::CONFLICT, "Capture nocturne en cours : preview indisponible").into_response();
    }
    let Ok(_camera) = state.camera_lock.try_lock() else {
        return err(StatusCode::CONFLICT, "Une preview est déjà en cours").into_response();
    };

    let config = state.config.read().await.clone();
    let tmp_path = state.paths.tmp_dir.join("aurion_preview.jpg");

    let mid_iso = ((config.exposure.iso_min as f64) * (config.exposure.iso_max as f64)).sqrt() as u32;
    let mid_shutter = ((config.exposure.shutter_min_us as f64) * (config.exposure.shutter_max_us as f64)).sqrt() as u64;
    let mut expo = crate::core::exposure::ExposureController::from_config(&config.exposure);
    expo.set(crate::core::models::ExposureSettings::new(mid_iso, mid_shutter));

    let mut last: Option<(Vec<u8>, crate::core::models::ExposureSettings)> = None;
    for i in 0..4 {
        let settings = expo.current();
        let data = match rpicam_still(&tmp_path, &settings, &config.capture.awb).await {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Preview capture {}/4 failed: {}", i + 1, e);
                return err(StatusCode::INTERNAL_SERVER_ERROR, format!("Caméra indisponible: {}", e)).into_response();
            }
        };
        let hist = image::load_from_memory(&data).ok().map(|img| {
            let rgb = img.to_rgb8();
            let roi = crate::core::orchestrator::crop_roi(rgb.as_raw(), rgb.width(), rgb.height(), config.detection.roi_top_percent);
            crate::core::exposure::compute_histogram(roi)
        });
        last = Some((data, settings));
        let Some(hist) = hist else { break };
        let next = expo.jump(&hist);
        let change = (next.shutter_us as f64 * next.iso as f64) / (settings.shutter_us as f64 * settings.iso as f64);
        tracing::info!("Preview auto-expo {}/4: ISO {} / {}µs → ISO {} / {}µs", i + 1, settings.iso, settings.shutter_us, next.iso, next.shutter_us);
        if (0.85..=1.18).contains(&change) {
            break; // converged
        }
    }

    let Some((data, settings)) = last else {
        return err(StatusCode::INTERNAL_SERVER_ERROR, "Aucune image capturée").into_response();
    };
    *state.latest_preview.write().await = Some(data.clone());
    *state.last_preview.write().await = Some(settings);
    state
        .add_log(format!("Preview: ISO {} / {} µs", settings.iso, settings.shutter_us))
        .await;

    (
        StatusCode::OK,
        [
            ("content-type", "image/jpeg".to_string()),
            ("x-aurion-iso", settings.iso.to_string()),
            ("x-aurion-shutter-us", settings.shutter_us.to_string()),
        ],
        data,
    )
        .into_response()
}

// ─── Config ────────────────────────────────────────────────

/// Current configuration. The Wi-Fi password is never sent back.
pub async fn get_config(State(state): State<AppState>) -> Json<AppConfig> {
    Json(state.config.read().await.masked())
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(mut update): Json<AppConfig>,
) -> Result<Json<AppConfig>, ApiError> {
    let mut config = state.config.write().await;

    // The UI sends back the masked placeholder when the password is unchanged.
    // Substitute it BEFORE validating (the mask itself is too short).
    if update.network.password == PASSWORD_MASK {
        update.network.password = config.network.password.clone();
    }
    if update.online_update.password == PASSWORD_MASK {
        update.online_update.password = config.online_update.password.clone();
    }
    update.validate().map_err(|e| err(StatusCode::BAD_REQUEST, e.to_string()))?;

    let network_changed = update.network.ssid != config.network.ssid
        || update.network.password != config.network.password
        || update.network.channel != config.network.channel;

    update
        .save(&state.paths.config_file)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Sauvegarde impossible: {}", e)))?;
    crate::core::config::mark_user_settings(&state.paths.config_file);
    let _ = update.save_usb_backup(); // settings kept on the key (new SD card)
    *config = update;
    let masked = config.masked();
    drop(config);

    state.add_log("Configuration mise à jour".into()).await;
    if network_changed {
        if state.system_actions && state.current_phase().await == Phase::Arm {
            // Apply now: the phone gets the answer, then the hotspot restarts
            // with the new name / password (the user reconnects).
            state.add_log("Réseau Wi-Fi modifié : redémarrage du hotspot dans 3 s".into()).await;
            let st = state.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(3)).await;
                let net = st.config.read().await.network.clone();
                let channel = net.channel.to_string();
                if let Err(e) = crate::sys::helper(&["ap-start", &net.ssid, &channel], Some(&net.password), Duration::from_secs(60)).await {
                    st.add_log(format!("Redémarrage du hotspot impossible: {}", e)).await;
                }
            });
        } else {
            state
                .add_log("Réseau Wi-Fi modifié : appliqué au prochain démarrage du hotspot".into())
                .await;
        }
    }
    Ok(Json(masked))
}

// ─── Presets ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct PresetsResponse {
    pub presets: Vec<PresetInfo>,
}

#[derive(Serialize)]
pub struct PresetInfo {
    pub name: String,
    pub is_builtin: bool,
}

fn user_preset_path(state: &AppState, name: &str) -> std::path::PathBuf {
    state
        .paths
        .presets_dir
        .join(format!("{}.json", validate::preset_file_stem(name)))
}

fn load_user_preset(path: &FsPath) -> Option<Preset> {
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Preset>(&content).ok()
}

pub async fn get_presets(State(state): State<AppState>) -> Json<PresetsResponse> {
    let mut presets: Vec<PresetInfo> = AppConfig::builtin_presets()
        .into_iter()
        .map(|p| PresetInfo { name: p.name, is_builtin: true })
        .collect();

    if let Ok(entries) = std::fs::read_dir(&state.paths.presets_dir) {
        let mut user: Vec<String> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
            .filter_map(|p| load_user_preset(&p))
            .map(|p| p.name)
            .filter(|n| validate::is_valid_preset_name(n))
            .collect();
        user.sort();
        for name in user {
            if !presets.iter().any(|p| p.name == name) {
                presets.push(PresetInfo { name, is_builtin: false });
            }
        }
    }

    Json(PresetsResponse { presets })
}

#[derive(Deserialize)]
pub struct SavePresetRequest {
    pub name: String,
}

pub async fn save_preset(
    State(state): State<AppState>,
    Json(req): Json<SavePresetRequest>,
) -> Result<StatusCode, ApiError> {
    let name = req.name.trim().to_string();
    if !validate::is_valid_preset_name(&name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "Nom de preset invalide (lettres, chiffres, espaces, - et _ ; 40 caractères max)",
        ));
    }
    if AppConfig::builtin_presets().iter().any(|p| p.name.eq_ignore_ascii_case(&name)) {
        return Err(err(StatusCode::CONFLICT, "Ce nom est réservé à un preset intégré"));
    }

    // Presets never store the Wi-Fi password.
    let preset = Preset { name: name.clone(), config: state.config.read().await.masked(), is_builtin: false };
    let content = serde_json::to_string_pretty(&preset)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    crate::core::config::write_atomic(&user_preset_path(&state, &name), content.as_bytes(), 0o600)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    state.add_log(format!("Preset '{}' sauvegardé", name)).await;
    Ok(StatusCode::CREATED)
}

pub async fn apply_preset(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<AppConfig>, ApiError> {
    let preset_config = if let Some(p) = AppConfig::builtin_presets().into_iter().find(|p| p.name == name) {
        p.config
    } else {
        if !validate::is_valid_preset_name(&name) {
            return Err(err(StatusCode::BAD_REQUEST, "Nom de preset invalide"));
        }
        load_user_preset(&user_preset_path(&state, &name))
            .ok_or_else(|| err(StatusCode::NOT_FOUND, format!("Preset '{}' introuvable", name)))?
            .config
    };

    let mut config = state.config.write().await;
    let merged = config.with_preset(&preset_config, &name);
    merged
        .validate()
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("Preset invalide: {}", e)))?;
    merged
        .save(&state.paths.config_file)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Sauvegarde impossible: {}", e)))?;
    crate::core::config::mark_user_settings(&state.paths.config_file);
    let _ = merged.save_usb_backup();
    *config = merged;
    let masked = config.masked();
    drop(config);

    state.add_log(format!("Preset '{}' appliqué", name)).await;
    Ok(Json(masked))
}

pub async fn delete_preset(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    if AppConfig::builtin_presets().iter().any(|p| p.name == name) {
        return Err(err(StatusCode::FORBIDDEN, "Les presets intégrés ne peuvent pas être supprimés"));
    }
    if !validate::is_valid_preset_name(&name) {
        return Err(err(StatusCode::BAD_REQUEST, "Nom de preset invalide"));
    }
    std::fs::remove_file(user_preset_path(&state, &name))
        .map_err(|_| err(StatusCode::NOT_FOUND, format!("Preset '{}' introuvable", name)))?;
    state.add_log(format!("Preset '{}' supprimé", name)).await;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Dark frames ───────────────────────────────────────────

#[derive(Deserialize)]
pub struct DarksRequest {
    /// Number of dark frames (1 to 30, default 10).
    #[serde(default)]
    pub count: Option<u32>,
    /// ISO / shutter; default: those of the last preview.
    #[serde(default)]
    pub iso: Option<u32>,
    #[serde(default)]
    pub shutter_us: Option<u64>,
}

/// Capture a series of dark frames (lens cap ON) in RAW, with the same ISO
/// and shutter as the night. Subtracting them in post-processing (Sequator,
/// Siril, PixInsight...) removes the thermal noise and hot pixels of the
/// sensor from the DNGs. Files: `<key>/darks/dark_<date>_ISO<iso>_<t>s_<n>.dng`.
/// Runs in the background; progress with GET /api/darks.
pub async fn capture_darks(
    State(state): State<AppState>,
    Json(req): Json<DarksRequest>,
) -> Result<Json<crate::web::DarkProgress>, ApiError> {
    if state.current_phase().await != Phase::Arm {
        return Err(err(StatusCode::CONFLICT, "Les darks se font avant de lancer la nuit (phase ARM)"));
    }
    let count = req.count.unwrap_or(10);
    if !(1..=30).contains(&count) {
        return Err(err(StatusCode::BAD_REQUEST, "Nombre de darks : 1 à 30"));
    }
    let last = *state.last_preview.read().await;
    let (iso, shutter_us) = match (req.iso, req.shutter_us, last) {
        (Some(i), Some(s), _) => (i, s),
        (None, None, Some(p)) => (p.iso, p.shutter_us),
        _ => return Err(err(StatusCode::BAD_REQUEST, "Faites d'abord une preview, ou indiquez ISO et temps de pose")),
    };
    if !(1..=25_600).contains(&iso) || !(1..=240_000_000).contains(&shutter_us) {
        return Err(err(StatusCode::BAD_REQUEST, "ISO ou temps de pose hors limites"));
    }
    if state.darks.read().await.as_ref().map(|d| d.running).unwrap_or(false) {
        return Err(err(StatusCode::CONFLICT, "Une série de darks est déjà en cours"));
    }
    let camera = state.camera_lock.clone().try_lock_owned()
        .map_err(|_| err(StatusCode::CONFLICT, "Caméra occupée (preview en cours)"))?;

    let progress = crate::web::DarkProgress { done: 0, total: count, iso, shutter_us, running: true, error: None };
    *state.darks.write().await = Some(progress.clone());
    state.add_log(format!("Darks : {} poses ISO {} / {:.1} s (bouchon en place)", count, iso, shutter_us as f64 / 1e6)).await;

    let st = state.clone();
    tokio::spawn(async move {
        let _camera = camera;
        let mount = FsPath::new(&st.config.read().await.storage.mount_point).join("darks");
        let tmp = st.paths.tmp_dir.join("aurion_dark.jpg");
        let tmp_dng = tmp.with_extension("dng");
        let date = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let mut error = None;
        for n in 0..count {
            let shutter = shutter_us.to_string();
            let gain = format!("{:.2}", iso as f64 / 100.0);
            let out = tmp.to_string_lossy().to_string();
            let _ = std::fs::remove_file(&tmp_dng);
            let timeout = Duration::from_secs(shutter_us.div_ceil(1_000_000) * 3 + 20);
            let res = crate::sys::run(
                "rpicam-still",
                &["--nopreview", "--raw", "-o", &out, "-t", "100", "--shutter", &shutter, "--gain", &gain,
                  "--awb", "daylight", "--denoise", "off", "--thumb", "none"],
                timeout,
            )
            .await
            .and_then(|_| std::fs::read(&tmp_dng).map_err(|e| format!("DNG absent: {}", e)))
            .and_then(|dng| {
                let name = format!("dark_{}_ISO{}_{:.1}s_{:02}.dng", date, iso, shutter_us as f64 / 1e6, n + 1);
                std::fs::create_dir_all(&mount).map_err(|e| e.to_string())?;
                crate::core::config::write_atomic(&mount.join(name), &dng, 0o644).map_err(|e| e.to_string())
            });
            if let Err(e) = res {
                error = Some(e);
                break;
            }
            if let Some(p) = st.darks.write().await.as_mut() {
                p.done = n + 1;
            }
        }
        let msg = match &error {
            None => format!("Darks terminés ({} fichiers dans darks/)", count),
            Some(e) => format!("Darks interrompus : {}", e),
        };
        if let Some(p) = st.darks.write().await.as_mut() {
            p.running = false;
            p.error = error;
        }
        st.add_log(msg).await;
    });

    Ok(Json(progress))
}

pub async fn get_darks(State(state): State<AppState>) -> Json<Option<crate::web::DarkProgress>> {
    Json(state.darks.read().await.clone())
}

// ─── Disconnect ────────────────────────────────────────────

/// Start the night: the orchestrator notices the DISCONNECT phase, waits
/// 15 s, turns the hotspot off and begins the capture.
pub async fn disconnect(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    let mut phase = state.phase.write().await;
    if *phase != Phase::Arm {
        return Err(err(StatusCode::CONFLICT, format!("Impossible depuis la phase {}", *phase)));
    }
    *phase = Phase::Disconnect;
    drop(phase);
    state.add_log("Déconnexion demandée : capture dans 15 s".into()).await;
    Ok(StatusCode::OK)
}

// ─── Storage ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct StorageResponse {
    pub total_gb: f64,
    pub free_gb: f64,
    pub free_percent: f64,
    pub status: String,
    pub summary: String,
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn storage_response(mount_point: &FsPath) -> Option<StorageResponse> {
    let (total, avail) = crate::sys::disk_usage(mount_point)?;
    let total_gb = total as f64 / 1_073_741_824.0;
    let free_gb = avail as f64 / 1_073_741_824.0;
    let pct = if total > 0 { avail as f64 / total as f64 * 100.0 } else { 0.0 };
    Some(StorageResponse {
        total_gb: round1(total_gb),
        free_gb: round1(free_gb),
        free_percent: round1(pct),
        status: if pct > 10.0 { "Ok".into() } else { "Low".into() },
        summary: format!("{:.0} % libre ({:.1} Go / {:.1} Go)", pct, free_gb, total_gb),
    })
}

pub async fn get_storage(State(state): State<AppState>) -> Json<StorageResponse> {
    let mount_point = state.config.read().await.storage.mount_point.clone();
    Json(storage_response(FsPath::new(&mount_point)).unwrap_or_else(|| StorageResponse {
        total_gb: 0.0,
        free_gb: 0.0,
        free_percent: 0.0,
        status: "Error".into(),
        summary: format!("{} non monté", mount_point),
    }))
}

// ─── Logs ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct LogsResponse {
    pub logs: Vec<String>,
}

pub async fn get_logs(State(state): State<AppState>) -> Json<LogsResponse> {
    Json(LogsResponse { logs: state.log_buffer.read().await.clone() })
}

// ─── Diagnostics ──────────────────────────────────────────

#[derive(Serialize)]
pub struct DiagnosticsResponse {
    pub version: String,
    pub platform: String,
    pub hostname: String,
    pub uptime: String,
    pub cpu_temp: Option<f64>,
    pub system_time: String,
    pub system_date: String,
    pub timezone: String,
    pub memory_available_mb: Option<u64>,
    pub throttled: Option<String>,
    /// Seconds after power-on when the Aurion Wi-Fi was ready (boot speed).
    pub hotspot_ready_secs: Option<f64>,
}

fn read_timezone() -> String {
    std::fs::read_to_string("/etc/timezone")
        .map(|s| s.trim().to_string())
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::fs::read_link("/etc/localtime").ok().and_then(|p| {
                let s = p.to_string_lossy().to_string();
                s.split("zoneinfo/").nth(1).map(str::to_string)
            })
        })
        .unwrap_or_else(|| chrono::Local::now().format("UTC%:z").to_string())
}

pub async fn get_diagnostics(State(state): State<AppState>) -> Json<DiagnosticsResponse> {
    let platform = if cfg!(feature = "rpi") { "Raspberry Pi" } else { "PC (dev)" }.to_string();

    let hostname = std::fs::read_to_string("/etc/hostname")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "?".into());

    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()))
        .map(|secs| format!("{}h {:02}min", secs as u64 / 3600, (secs as u64 % 3600) / 60))
        .unwrap_or_else(|| "--".into());

    let cpu_temp = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|t| round1(t / 1000.0));

    let memory_available_mb = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
        s.lines()
            .find(|l| l.starts_with("MemAvailable:"))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|kb| kb.parse::<u64>().ok())
            .map(|kb| kb / 1024)
    });

    let throttled = crate::sys::run("vcgencmd", &["get_throttled"], Duration::from_secs(3))
        .await
        .ok()
        .map(|s| s.trim().trim_start_matches("throttled=").to_string());

    let now = chrono::Local::now();
    Json(DiagnosticsResponse {
        version: crate::VERSION.to_string(),
        platform,
        hostname,
        uptime,
        cpu_temp,
        system_time: now.format("%H:%M:%S").to_string(),
        system_date: now.format("%d/%m/%Y").to_string(),
        timezone: read_timezone(),
        memory_available_mb,
        throttled,
        hotspot_ready_secs: *state.hotspot_ready_at.lock().unwrap(),
    })
}

// ─── Set System Time ──────────────────────────────────────

#[derive(Deserialize)]
pub struct SetTimeRequest {
    /// Phone clock in Unix milliseconds (preferred, unambiguous).
    #[serde(default)]
    pub epoch_ms: Option<i64>,
    /// Legacy: local date/time "YYYY-MM-DD HH:MM[:SS]" (Pi time zone).
    #[serde(default)]
    pub datetime: Option<String>,
    /// Phone IANA time zone, e.g. "Europe/Paris".
    #[serde(default)]
    pub timezone: Option<String>,
    /// Automatic sync sent by the UI on page load.
    #[serde(default)]
    pub auto: bool,
}

#[derive(Serialize)]
pub struct SetTimeResponse {
    pub changed: bool,
    pub message: String,
}

/// Minimum drift before an automatic sync touches the clock.
const AUTO_SYNC_MIN_DRIFT_SECS: i64 = 60;

pub async fn set_system_time(
    State(state): State<AppState>,
    Json(req): Json<SetTimeRequest>,
) -> Result<Json<SetTimeResponse>, ApiError> {
    let target_epoch: i64 = if let Some(ms) = req.epoch_ms {
        ms.div_euclid(1000)
    } else if let Some(ref dt) = req.datetime {
        let naive = validate::parse_local_datetime(dt)
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "Format de date invalide (AAAA-MM-JJ HH:MM)"))?;
        naive
            .and_local_timezone(chrono::Local)
            .earliest()
            .ok_or_else(|| err(StatusCode::BAD_REQUEST, "Date locale inexistante"))?
            .timestamp()
    } else {
        return Err(err(StatusCode::BAD_REQUEST, "epoch_ms ou datetime requis"));
    };

    // 2024-01-01 .. 2100-01-01
    if !(1_704_067_200..4_102_444_800).contains(&target_epoch) {
        return Err(err(StatusCode::BAD_REQUEST, "Date hors limites"));
    }
    if let Some(ref tz) = req.timezone {
        if !validate::is_valid_timezone_name(tz) {
            return Err(err(StatusCode::BAD_REQUEST, "Fuseau horaire invalide"));
        }
    }

    let phase = state.current_phase().await;
    let drift = (target_epoch - chrono::Utc::now().timestamp()).abs();
    let clock_unset = chrono::Utc::now().year() < 2024;
    let tz_change = req.timezone.clone().filter(|tz| *tz != read_timezone());
    let need_time = drift >= if req.auto { AUTO_SYNC_MIN_DRIFT_SECS } else { 1 };

    // Never move the clock under a running capture (unless it was never set).
    if capture_in_progress(phase) && !clock_unset {
        if req.auto {
            return Ok(Json(SetTimeResponse { changed: false, message: "Capture en cours : horloge inchangée".into() }));
        }
        return Err(err(StatusCode::CONFLICT, "Capture en cours : l'heure ne peut pas être modifiée"));
    }
    if !need_time && tz_change.is_none() {
        state.time_synced.store(true, std::sync::atomic::Ordering::Relaxed);
        return Ok(Json(SetTimeResponse { changed: false, message: "Horloge déjà à l'heure".into() }));
    }

    require_system_actions(&state)?;

    if let Some(ref tz) = tz_change {
        crate::sys::helper(&["set-timezone", tz], None, Duration::from_secs(10))
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Fuseau: {}", e)))?;
    }
    if need_time {
        let epoch = target_epoch.to_string();
        crate::sys::helper(&["set-time", &epoch], None, Duration::from_secs(10))
            .await
            .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Heure: {}", e)))?;
    }

    state.time_synced.store(true, std::sync::atomic::Ordering::Relaxed);
    let msg = format!(
        "Heure système réglée: {}{}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        tz_change.map(|tz| format!(" ({})", tz)).unwrap_or_default()
    );
    state.add_log(msg.clone()).await;
    Ok(Json(SetTimeResponse { changed: true, message: msg }))
}

// ─── System Shutdown ──────────────────────────────────────

pub async fn system_shutdown(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    require_system_actions(&state)?;
    state.add_log("Arrêt système demandé".into()).await;
    let _ = crate::sys::run("sync", &[], Duration::from_secs(30)).await;
    // Let the HTTP response reach the phone before the network goes down.
    tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(2)).await;
        if let Err(e) = crate::sys::helper(&["shutdown"], None, Duration::from_secs(30)).await {
            tracing::error!("Shutdown failed: {}", e);
        }
    });
    Ok(StatusCode::OK)
}

// ─── Wi-Fi Client (maintenance mode) ─────────────────────

#[derive(Serialize, Debug, PartialEq)]
pub struct WifiNetwork {
    pub ssid: String,
    /// Signal quality 0-100 %.
    pub signal: i32,
    pub security: String,
}

#[derive(Serialize)]
pub struct WifiScanResponse {
    pub networks: Vec<WifiNetwork>,
}

/// Parse the helper `wifi-scan` output: one `signal<TAB>security<TAB>ssid`
/// line per network. Keeps the strongest entry per SSID.
pub fn parse_wifi_scan(output: &str) -> Vec<WifiNetwork> {
    let mut networks: Vec<WifiNetwork> = output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let signal = parts.next()?.trim().parse::<i32>().ok()?.clamp(0, 100);
            let security = parts.next()?.trim().to_string();
            let ssid = parts.next()?.to_string();
            if ssid.is_empty() || validate::validate_ssid(&ssid).is_err() {
                return None;
            }
            let security = if security.is_empty() || security == "--" { "Ouvert".to_string() } else { security };
            Some(WifiNetwork { ssid, signal, security })
        })
        .collect();
    networks.sort_by(|a, b| a.ssid.cmp(&b.ssid).then(b.signal.cmp(&a.signal)));
    networks.dedup_by(|a, b| a.ssid == b.ssid);
    networks.sort_by_key(|n| std::cmp::Reverse(n.signal));
    networks
}

pub async fn wifi_scan(State(state): State<AppState>) -> Result<Json<WifiScanResponse>, ApiError> {
    require_system_actions(&state)?;
    let output = crate::sys::helper(&["wifi-scan"], None, Duration::from_secs(30))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Scan impossible: {}", e)))?;
    Ok(Json(WifiScanResponse { networks: parse_wifi_scan(&output) }))
}

#[derive(Deserialize)]
pub struct WifiConnectRequest {
    pub ssid: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct WifiConnectResponse {
    pub success: bool,
    pub message: String,
    pub ip_address: Option<String>,
}

pub async fn wifi_connect(
    State(state): State<AppState>,
    Json(req): Json<WifiConnectRequest>,
) -> Result<Json<WifiConnectResponse>, ApiError> {
    validate::validate_ssid(&req.ssid).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    validate::validate_wpa_passphrase(&req.password, 8).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    if capture_in_progress(state.current_phase().await) {
        return Err(err(StatusCode::CONFLICT, "Capture en cours"));
    }
    require_system_actions(&state)?;

    state.add_log(format!("Connexion Wi-Fi à {}…", req.ssid)).await;
    let result = crate::sys::helper(&["wifi-connect", &req.ssid], Some(&req.password), Duration::from_secs(60)).await;

    let (success, message, ip) = match result {
        Ok(out) => {
            let ip = out.lines().rev().map(str::trim).find(|l| l.parse::<std::net::Ipv4Addr>().is_ok()).map(str::to_string);
            let msg = format!("Connecté à {} (IP: {})", req.ssid, ip.as_deref().unwrap_or("?"));
            (true, msg, ip)
        }
        Err(e) => (false, format!("Échec de connexion à {}: {}", req.ssid, e), None),
    };
    state.add_log(message.clone()).await;
    Ok(Json(WifiConnectResponse { success, message, ip_address: ip }))
}

#[derive(Serialize)]
pub struct WifiStatusResponse {
    pub mode: String,
    pub ssid: Option<String>,
    pub ip_address: Option<String>,
}

async fn wlan_ip() -> Option<String> {
    let out = crate::sys::run("ip", &["-4", "-o", "addr", "show", "wlan0"], Duration::from_secs(3)).await.ok()?;
    out.split_whitespace()
        .skip_while(|w| *w != "inet")
        .nth(1)
        .map(|s| s.split('/').next().unwrap_or(s).to_string())
}

/// Parse `iw dev wlan0 info`: (is_access_point, connected_ssid).
pub fn parse_iw_info(output: &str) -> (bool, Option<String>) {
    let mut ap = false;
    let mut ssid = None;
    for line in output.lines().map(str::trim) {
        if line == "type AP" {
            ap = true;
        } else if let Some(s) = line.strip_prefix("ssid ") {
            if !s.is_empty() {
                ssid = Some(s.to_string());
            }
        }
    }
    (ap, ssid)
}

pub async fn wifi_status() -> Json<WifiStatusResponse> {
    let info = crate::sys::run("iw", &["dev", "wlan0", "info"], Duration::from_secs(3)).await.unwrap_or_default();
    let (is_ap, ssid) = parse_iw_info(&info);
    let ip = wlan_ip().await;
    let mode = if is_ap { "hotspot" } else if ssid.is_some() { "client" } else { "disconnected" };
    Json(WifiStatusResponse { mode: mode.into(), ssid, ip_address: ip })
}

/// Restart the Aurion hotspot (leave maintenance/client mode).
pub async fn wifi_hotspot(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    require_system_actions(&state)?;
    let net = state.config.read().await.network.clone();
    let channel = net.channel.to_string();
    crate::sys::helper(&["ap-start", &net.ssid, &channel], Some(&net.password), Duration::from_secs(60))
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Hotspot: {}", e)))?;
    state.add_log(format!("Hotspot '{}' démarré", net.ssid)).await;
    Ok(StatusCode::OK)
}

// ─── Sécurité ─────────────────────────────────────────────

/// Indique si le mot de passe Wi-Fi est encore la valeur par défaut.
pub async fn is_default_password(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(serde_json::json!({ "default": config.network.password == DEFAULT_WIFI_PASSWORD }))
}

// ─── Mise à jour OTA ──────────────────────────────────────

/// ELF `e_machine` expected for the running architecture.
fn expected_elf_machine() -> Option<u16> {
    match std::env::consts::ARCH {
        "aarch64" => Some(183),
        "arm" => Some(40),
        "x86_64" => Some(62),
        _ => None,
    }
}

/// Check that `data` is an executable ELF for this machine.
pub fn validate_update_binary(data: &[u8]) -> Result<(), String> {
    if data.len() < 64 || &data[..4] != b"\x7fELF" {
        return Err("Le fichier n'est pas un binaire Linux (ELF)".into());
    }
    let class_ok = match std::mem::size_of::<usize>() {
        8 => data[4] == 2,
        _ => data[4] == 1,
    };
    if !class_ok || data[5] != 1 {
        return Err("Binaire incompatible (32/64 bits ou endianness)".into());
    }
    let e_type = u16::from_le_bytes([data[16], data[17]]);
    if e_type != 2 && e_type != 3 {
        return Err("Le fichier ELF n'est pas un exécutable".into());
    }
    let machine = u16::from_le_bytes([data[18], data[19]]);
    match expected_elf_machine() {
        Some(expected) if expected != machine => Err(format!(
            "Architecture incompatible (reçu e_machine={}, attendu {} pour {})",
            machine,
            expected,
            std::env::consts::ARCH
        )),
        _ => Ok(()),
    }
}

/// Upload a new Aurion binary from the phone.
///
/// 1. multipart field `binary`, ELF + architecture check
/// 2. written next to the current binary (`.new`), made executable and
///    test-run with `--version`
/// 3. current binary kept as `.prev`, new binary atomically renamed in place
/// 4. the process exits: systemd (`Restart=always`) starts the new version
///
/// No root access is needed: the binary belongs to the service user.
pub async fn system_update(
    State(state): State<AppState>,
    mut multipart: axum::extract::Multipart,
) -> Result<String, ApiError> {
    if capture_in_progress(state.current_phase().await) {
        return Err(err(StatusCode::CONFLICT, "Capture en cours : mise à jour impossible"));
    }

    let mut binary: Option<Vec<u8>> = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| err(StatusCode::BAD_REQUEST, format!("Envoi invalide: {}", e)))?
    {
        if field.name() == Some("binary") {
            let bytes = field
                .bytes()
                .await
                .map_err(|e| err(StatusCode::BAD_REQUEST, format!("Lecture du fichier impossible: {}", e)))?;
            binary = Some(bytes.to_vec());
        }
    }
    let data = binary.filter(|d| !d.is_empty()).ok_or_else(|| err(StatusCode::BAD_REQUEST, "Aucun binaire reçu"))?;
    install_binary(&state, &data).await.map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
    Ok("Mise à jour appliquée. Rechargez la page dans 15 secondes.".to_string())
}

/// Check, stage, try and install a new binary (upload or online update).
/// The previous binary is kept as `.prev` (rollback). Returns the version
/// printed by the new binary; the service restarts 2 s later.
pub(crate) async fn install_binary(state: &AppState, data: &[u8]) -> Result<String, String> {
    validate_update_binary(data)?;
    let target = update_target(state)?;
    let staged = target.with_extension("new");
    let backup = target.with_extension("prev");

    crate::core::config::write_atomic(&staged, data, 0o755).map_err(|e| format!("Écriture impossible: {}", e))?;

    // Refuse a binary that does not even start.
    let staged_str = staged.to_string_lossy().to_string();
    let version = match crate::sys::run(&staged_str, &["--version"], Duration::from_secs(15)).await {
        Ok(v) => v.trim().to_string(),
        Err(e) => {
            let _ = std::fs::remove_file(&staged);
            return Err(format!("Le nouveau binaire ne démarre pas: {}", e));
        }
    };

    if target.exists() {
        std::fs::copy(&target, &backup).map_err(|e| format!("Sauvegarde de l'ancien binaire impossible: {}", e))?;
    }
    std::fs::rename(&staged, &target).map_err(|e| format!("Remplacement impossible: {}", e))?;

    tracing::info!("OTA: {} remplacé ({} octets, {}), ancienne version dans {:?}", target.display(), data.len(), version, backup);
    state.add_log(format!("Mise à jour installée : {} ({} Mo). Redémarrage…", version, data.len() / 1_048_576)).await;
    schedule_restart(state);
    Ok(version)
}

/// Binary replaced by updates (the running executable in production).
pub(crate) fn update_target(state: &AppState) -> Result<std::path::PathBuf, String> {
    match &state.update.target {
        Some(t) => Ok(t.clone()),
        None => std::env::current_exe().map_err(|e| format!("Binaire courant introuvable: {}", e)),
    }
}

/// Exit 2 s later so that systemd starts the new binary (`Restart=always`).
pub(crate) fn schedule_restart(state: &AppState) {
    if state.update.restart {
        tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(2)).await;
            tracing::info!("OTA: exiting so that systemd restarts the new binary");
            std::process::exit(0);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wifi_scan_parsing() {
        let out = "80\tWPA2\tMaison\n40\tWPA2\tMaison\n55\t--\tCafé Libre\nbad line\n70\tWPA1 WPA2\t\n30\tWPA2\tevil\u{7}ssid\n";
        let nets = parse_wifi_scan(out);
        assert_eq!(
            nets,
            vec![
                WifiNetwork { ssid: "Maison".into(), signal: 80, security: "WPA2".into() },
                WifiNetwork { ssid: "Café Libre".into(), signal: 55, security: "Ouvert".into() },
            ]
        );
    }

    #[test]
    fn planned_night_duration() {
        let mut c = AppConfig::default(); // 21:00 → 06:00
        assert_eq!(planned_hours(&c), 9.0);
        c.time_range.duration_hours = Some(4.5);
        assert_eq!(planned_hours(&c), 4.5);
        c.time_range.duration_hours = None;
        c.time_range.start = chrono::NaiveTime::from_hms_opt(18, 30, 0).unwrap();
        c.time_range.end = chrono::NaiveTime::from_hms_opt(23, 0, 0).unwrap();
        assert_eq!(planned_hours(&c), 4.5);
    }

    #[test]
    fn hours_wording() {
        assert_eq!(fmt_hours(9.0), "9 h");
        assert_eq!(fmt_hours(4.8), "4 h 48");
        assert_eq!(fmt_hours(0.05), "0 h 03");
        assert_eq!(fmt_hours(-1.0), "0 h");
    }

    #[test]
    fn tiny_files_do_not_skew_the_estimate() {
        let c = AppConfig { capture: crate::core::config::CaptureConfig { output_format: crate::core::models::OutputFormat::Jpg, ..AppConfig::default().capture }, ..AppConfig::default() };
        let images = vec![("aurora_20260101_000000_00000.jpg".to_string(), 0u64)];
        assert_eq!(bytes_per_capture(&c, &images), 4_000_000 + 20_000, "typical size used instead of 0");
    }

    #[test]
    fn capacity_estimation() {
        let mut c = AppConfig::default();
        c.capture.output_format = crate::core::models::OutputFormat::RawDng;
        c.capture.capture_interval_secs = 0;
        c.exposure.shutter_max_us = 28_000_000;
        // No image yet: typical night DNG (14 MB); back to back, 28 s + 2 s overhead
        let per = bytes_per_capture(&c, &[]);
        assert_eq!(per, 14_000_000);
        let hours = capacity_hours(&c, 14_000_000 * 120, per);
        assert!((hours - 1.0).abs() < 0.05, "{}", hours);
        // Real sizes from the key replace the estimate
        let imgs = vec![("aurora_x.dng".to_string(), 10_000_000), ("aurora_y.dng".to_string(), 12_000_000)];
        assert_eq!(bytes_per_capture(&c, &imgs), 11_000_000);
    }

    #[test]
    fn iw_info_parsing() {
        let ap = "Interface wlan0\n\tifindex 3\n\tssid Aurion\n\ttype AP\n\tchannel 6";
        assert_eq!(parse_iw_info(ap), (true, Some("Aurion".into())));
        let client = "Interface wlan0\n\tssid Maison\n\ttype managed";
        assert_eq!(parse_iw_info(client), (false, Some("Maison".into())));
        assert_eq!(parse_iw_info(""), (false, None));
    }

    #[test]
    fn update_binary_validation() {
        assert!(validate_update_binary(b"not an elf at all").is_err());
        let own = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        assert!(validate_update_binary(&own).is_ok(), "the running test binary must be accepted");
        // Same file with a foreign architecture
        let mut foreign = own.clone();
        let other: u16 = if expected_elf_machine() == Some(183) { 62 } else { 183 };
        foreign[18..20].copy_from_slice(&other.to_le_bytes());
        assert!(validate_update_binary(&foreign).is_err());
        // Relocatable object (not executable)
        let mut obj = own;
        obj[16..18].copy_from_slice(&1u16.to_le_bytes());
        assert!(validate_update_binary(&obj).is_err());
    }
}
