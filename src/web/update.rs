//! Updates without a computer: the Pi downloads the new version itself
//! from the GitHub releases of the project.
//!
//! - **stable**: the latest release (`vX.Y.Z`);
//! - **dev**: the `edge` pre-release, rebuilt by the CI on every push to
//!   `main` (a fix pushed from the phone reaches the camera in minutes).
//!
//! The Pi has no internet on its own hotspot: it joins the phone's hotspot
//! (or the home Wi-Fi) for the download, then comes back to its hotspot
//! (restart of the service). Everything runs in the background, the phone
//! only has to reconnect to the Aurion Wi-Fi afterwards; the result is kept
//! in `update_status.json` and shown on the Diagnostics page.
//!
//! What is installed (1.13+): the signed package `aurion-update.tar.gz`,
//! the application AND the system files it needs (root helper, systemd
//! units, udev rule), put in place by `aurion-helper app-update`, which
//! checks the signature again as root. Older helpers, or releases without
//! the package: the bare binary `aurion-arm64`, as before.

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::core::config::PASSWORD_MASK;
use crate::core::validate;
use crate::web::api::{capture_in_progress, err, install_binary, require_system_actions, ApiError};
use crate::web::update_journal::{self as journal, Level};
use crate::web::AppState;

/// GitHub repository of the project (public: no token needed).
pub const REPO: &str = "PercyaDJ/Aurion";
/// Release asset holding the bare arm64 binary.
pub const ASSET: &str = "aurion-arm64";
/// Signed update package: application and system files.
pub const BUNDLE_ASSET: &str = "aurion-update.tar.gz";
/// `aurion-helper version` this application expects. From version 5 the
/// helper installs update packages, so it is updated from the phone too;
/// an older one needs the SD image or the .deb once.
pub const EXPECTED_HELPER_VERSION: &str = "5";
/// First helper version able to install an update package.
pub const BUNDLE_HELPER_VERSION: u32 = 5;
/// Largest binary or package accepted, uploaded or downloaded (the real
/// ones are a few MB).
pub const MAX_BINARY_BYTES: u64 = 64 * 1024 * 1024;
/// Download attempts: a phone hotspot may drop for a few minutes, the
/// download then resumes where it stopped.
const DOWNLOAD_ATTEMPTS: u32 = 10;
const DOWNLOAD_RETRY_DELAY: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UpdateStatus {
    pub running: bool,
    /// Local time of the last change.
    pub at: String,
    pub ok: Option<bool>,
    pub channel: String,
    pub message: String,
    /// Journal of this update (`update_journal`), empty before 1.13.
    #[serde(default)]
    pub log: String,
    /// "essai" (installed, on trial), "validee" (confirmed after its first
    /// minutes) or "echec" (refused, failed or rolled back); empty while running.
    #[serde(default)]
    pub outcome: String,
    /// The result window was closed on the phone.
    #[serde(default)]
    pub seen: bool,
    #[serde(default)]
    pub from: String,
    #[serde(default)]
    pub to: String,
}

fn status_path(state: &AppState) -> std::path::PathBuf {
    state.paths.config_file.with_file_name("update_status.json")
}

/// The file as written, without the power-cut interpretation.
fn read_raw_status(state: &AppState) -> UpdateStatus {
    std::fs::read(status_path(state))
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

pub fn read_status(state: &AppState) -> UpdateStatus {
    let mut status = read_raw_status(state);
    // "Running" in the file but not in this process: interrupted (power cut)
    if status.running && !state.online_update_running.load(Ordering::SeqCst) {
        status.running = false;
        status.ok = Some(false);
        status.message = format!("interrompue ({})", status.message);
    }
    status
}

fn write_status(state: &AppState, status: &UpdateStatus) {
    if let Ok(json) = serde_json::to_vec(status) {
        let _ = crate::core::config::write_atomic(&status_path(state), &json, 0o600);
    }
}

async fn set_status(state: &AppState, channel: &str, running: bool, ok: Option<bool>, message: impl Into<String>) {
    let message = message.into();
    state.add_log(format!("Mise à jour en ligne : {}", message)).await;
    let mut status = read_raw_status(state);
    status.running = running;
    status.at = chrono::Local::now().format("%d/%m %H:%M").to_string();
    status.ok = ok;
    if !channel.is_empty() {
        status.channel = channel.to_string();
    }
    status.message = message.clone();
    write_status(state, &status);
    let level = match (running, ok) {
        (false, Some(true)) => Level::Ok,
        (false, Some(false)) => Level::Error,
        _ => Level::Info,
    };
    note(state, level, &message).await;
}

/// Final result of the update, shown once in a window on the phone.
fn set_outcome(state: &AppState, outcome: &str, to: Option<&str>) {
    let mut status = read_raw_status(state);
    status.outcome = outcome.to_string();
    status.seen = false;
    if let Some(v) = to {
        status.to = v.to_string();
    }
    write_status(state, &status);
}

/// A line in the journal of the current update.
async fn note(state: &AppState, level: Level, message: &str) {
    let log = read_raw_status(state).log;
    if !log.is_empty() {
        journal::write(state, &log, level, message).await;
    }
}

/// A freshly installed version runs "on trial" (marker `aurion.trial` next to
/// the binary) until it has run this long: if it fails to start meanwhile,
/// systemd's OnFailure puts the previous version back (aurion-helper
/// app-rollback).
pub const TRIAL_CONFIRM_SECS: u64 = 180;

/// Marker of a version on trial (content: its version).
pub fn trial_marker(target: &std::path::Path) -> std::path::PathBuf {
    target.with_extension("trial")
}

/// Written by the helper after an automatic rollback (content: failed version).
pub fn rollback_marker(target: &std::path::Path) -> std::path::PathBuf {
    target.with_extension("rolled-back")
}

/// Why the application itself asked for a rollback (shown with it).
pub fn rollback_reason(target: &std::path::Path) -> std::path::PathBuf {
    target.with_extension("rollback-reason")
}

/// At startup: report an automatic rollback, then confirm the running
/// version once it has run [`TRIAL_CONFIRM_SECS`]. A version whose Aurion
/// Wi-Fi did not start is not confirmed but undone: in a closed box, the
/// phone could never reach the camera again.
pub async fn startup_checks(state: AppState, confirm_after: Duration) {
    let Ok(target) = crate::web::api::update_target(&state) else { return };
    let rolled = rollback_marker(&target);
    if let Ok(failed) = std::fs::read_to_string(&rolled) {
        let _ = std::fs::remove_file(&rolled);
        let reason = std::fs::read_to_string(rollback_reason(&target)).ok();
        let _ = std::fs::remove_file(rollback_reason(&target));
        let why = match reason.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
            Some(r) => r.to_string(),
            None => "elle ne démarrait pas".to_string(),
        };
        let msg = format!("la version {} a été retirée ({}) : retour automatique à la version précédente", failed.trim(), why);
        set_status(&state, "", false, Some(false), msg).await;
        set_outcome(&state, "echec", None);
    }
    let trial = trial_marker(&target);
    if !trial.exists() {
        return;
    }
    if read_raw_status(&state).outcome == "essai" {
        note(&state, Level::Info, &format!("Version {} démarrée, à l'essai pendant {} s", crate::VERSION, confirm_after.as_secs())).await;
    }
    tokio::time::sleep(confirm_after).await;
    if !trial.exists() {
        return;
    }
    let hotspot_error = state.hotspot_error.lock().unwrap().clone();
    if let (true, Some(e)) = (state.system_actions, hotspot_error) {
        let reason = format!("son Wi-Fi Aurion ne démarrait pas : {}", first_line(&e));
        state.add_log(format!("Version {} non confirmée : {}", crate::VERSION, reason)).await;
        note(&state, Level::Error, &format!("Version {} non confirmée : {}", crate::VERSION, reason)).await;
        let _ = std::fs::write(rollback_reason(&target), &reason);
        match crate::sys::helper(&["app-rollback"], None, Duration::from_secs(120)).await {
            Ok(_) => crate::web::api::schedule_restart(&state),
            Err(e) => state.add_log(format!("Retour arrière impossible : {}", first_line(&e))).await,
        }
        return;
    }
    if std::fs::remove_file(&trial).is_ok() {
        state.add_log(format!("Version {} confirmée : elle démarre et fonctionne", crate::VERSION)).await;
        if read_raw_status(&state).outcome == "essai" {
            let msg = format!("{} validée : elle fonctionne depuis {} s, Wi-Fi Aurion compris", crate::VERSION, confirm_after.as_secs());
            set_status(&state, "", false, Some(true), msg).await;
            set_outcome(&state, "validee", Some(crate::VERSION));
            journal::prune(&state);
        }
    }
}

/// API address of the release for a channel.
pub fn release_url(api: &str, channel: &str) -> Option<String> {
    match channel {
        "stable" => Some(format!("{}/repos/{}/releases/latest", api, REPO)),
        "dev" => Some(format!("{}/repos/{}/releases/tags/edge", api, REPO)),
        _ => None,
    }
}

/// Download address of the binary in a release description (GitHub API
/// JSON), refused unless it points to the project's own releases.
pub fn asset_url(release_json: &str, allowed_prefix: &str) -> Result<(String, String), String> {
    let v: serde_json::Value = serde_json::from_str(release_json).map_err(|_| "Réponse GitHub illisible".to_string())?;
    let tag = v["tag_name"].as_str().unwrap_or("?").to_string();
    let url = named_asset(&v, ASSET, allowed_prefix)?
        .ok_or_else(|| format!("La version {} ne contient pas le fichier {}", tag, ASSET))?;
    Ok((tag, url))
}

/// Update package, its checksum and its signature, when the release has
/// them (releases before 1.13 only publish the bare binary).
pub fn bundle_urls(release_json: &str, allowed_prefix: &str) -> Result<Option<(String, String, String)>, String> {
    let v: serde_json::Value = serde_json::from_str(release_json).map_err(|_| "Réponse GitHub illisible".to_string())?;
    let bundle = named_asset(&v, BUNDLE_ASSET, allowed_prefix)?;
    let sum = named_asset(&v, &format!("{}.sha256", BUNDLE_ASSET), allowed_prefix)?;
    let sig = named_asset(&v, &format!("{}.sig", BUNDLE_ASSET), allowed_prefix)?;
    match (bundle, sum, sig) {
        (Some(b), Some(h), Some(s)) => Ok(Some((b, h, s))),
        (None, _, _) => Ok(None),
        _ => Err(format!("{} publié sans son empreinte ou sa signature : mise à jour refusée", BUNDLE_ASSET)),
    }
}

/// URL of the checksum published next to the binary (`aurion-arm64.sha256`).
pub fn checksum_url(release_json: &str, allowed_prefix: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(release_json).map_err(|_| "Réponse GitHub illisible".to_string())?;
    named_asset(&v, &format!("{}.sha256", ASSET), allowed_prefix)?
        .ok_or_else(|| format!("Empreinte {}.sha256 absente de la version : mise à jour refusée", ASSET))
}

/// URL of the signature published next to the binary (`aurion-arm64.sig`).
pub fn signature_url(release_json: &str, allowed_prefix: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(release_json).map_err(|_| "Réponse GitHub illisible".to_string())?;
    named_asset(&v, crate::web::signing::SIGNATURE_ASSET, allowed_prefix)?.ok_or_else(|| {
        format!("Signature {} absente de la version (non signée) : mise à jour refusée", crate::web::signing::SIGNATURE_ASSET)
    })
}

fn named_asset(release: &serde_json::Value, name: &str, allowed_prefix: &str) -> Result<Option<String>, String> {
    let Some(url) = release["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == name)
        .and_then(|a| a["browser_download_url"].as_str())
    else {
        return Ok(None);
    };
    if !url.starts_with(allowed_prefix) || url.contains("..") || url.chars().any(|c| c.is_whitespace()) {
        return Err("Adresse de téléchargement inattendue : refusée".into());
    }
    Ok(Some(url.to_string()))
}

/// The downloaded file matches the published SHA-256 (`<hex>  <name>`): a
/// truncated or altered download is never installed.
pub fn check_sha256(data: &[u8], sha256_file: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let expected = sha256_file.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    if expected.len() != 64 || !expected.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Empreinte publiée illisible : mise à jour refusée".into());
    }
    let actual: String = Sha256::digest(data).iter().map(|b| format!("{:02x}", b)).collect();
    if actual != expected {
        return Err("Empreinte SHA-256 différente de celle publiée (téléchargement incomplet ou modifié) : mise à jour refusée".into());
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct OnlineUpdateRequest {
    /// "stable" or "dev".
    pub channel: String,
    /// Wi-Fi to join for the download; empty: current connection.
    #[serde(default)]
    pub ssid: Option<String>,
    /// Its password (the mask keeps the saved one).
    #[serde(default)]
    pub password: Option<String>,
}

/// Start an update from GitHub in the background.
pub async fn start_online_update(
    State(state): State<AppState>,
    Json(req): Json<OnlineUpdateRequest>,
) -> Result<Json<UpdateStatus>, ApiError> {
    if release_url("", &req.channel).is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "Canal inconnu (stable ou dev)"));
    }
    if capture_in_progress(state.current_phase().await) {
        return Err(err(StatusCode::CONFLICT, "Nuit en cours : mise à jour impossible"));
    }
    if state.online_update_running.swap(true, Ordering::SeqCst) {
        return Err(err(StatusCode::CONFLICT, "Une mise à jour est déjà en cours"));
    }
    let started = scopeguard_flag(&state);

    // Wi-Fi for the download (saved for the next time)
    let wifi = {
        let mut config = state.config.write().await;
        let ssid = req.ssid.clone().unwrap_or_default();
        let mut password = req.password.clone().unwrap_or_default();
        if password == PASSWORD_MASK {
            password = config.online_update.password.clone();
        }
        if !ssid.is_empty() {
            validate::validate_ssid(&ssid).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
            validate::validate_wpa_passphrase(&password, 8).map_err(|e| err(StatusCode::BAD_REQUEST, e))?;
            if config.online_update.ssid != ssid || config.online_update.password != password {
                let mut updated = config.clone();
                updated.online_update = crate::core::config::OnlineUpdateConfig { ssid: ssid.clone(), password: password.clone() };
                updated
                    .save(&state.paths.config_file)
                    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Sauvegarde impossible: {}", e)))?;
                *config = updated;
            }
            Some((ssid, password))
        } else {
            None
        }
    };
    if wifi.is_some() {
        require_system_actions(&state)?;
    }
    started.keep();

    let message = match &wifi {
        Some((ssid, _)) => format!(
            "Aurion va couper son Wi-Fi et rejoindre « {} » pendant 2 à 5 min. Activez ce partage de connexion maintenant, \
             puis reconnectez-vous au Wi-Fi Aurion.",
            ssid
        ),
        None => "Téléchargement avec la connexion actuelle du Pi…".to_string(),
    };
    let log = journal::new_name(&req.channel);
    write_status(&state, &UpdateStatus {
        channel: req.channel.clone(),
        log: log.clone(),
        from: crate::VERSION.to_string(),
        ..Default::default()
    });
    journal::write(&state, &log, Level::Info, &format!("Mise à jour demandée depuis la version {}, canal {}", crate::VERSION, req.channel)).await;
    set_status(&state, &req.channel, true, None, message).await;
    let st = state.clone();
    let channel = req.channel.clone();
    tokio::spawn(async move { run_online_update(st, channel, wifi).await });
    Ok(Json(read_status(&state)))
}

/// Clears the "running" flag if the request is refused before the task starts.
struct FlagGuard {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    keep: bool,
}

impl FlagGuard {
    fn keep(mut self) {
        self.keep = true;
    }
}

impl Drop for FlagGuard {
    fn drop(&mut self) {
        if !self.keep {
            self.flag.store(false, Ordering::SeqCst);
        }
    }
}

fn scopeguard_flag(state: &AppState) -> FlagGuard {
    FlagGuard { flag: state.online_update_running.clone(), keep: false }
}

async fn run_online_update(state: AppState, channel: String, wifi: Option<(String, String)>) {
    tokio::time::sleep(Duration::from_secs(3)).await; // let the answer reach the phone
    let result = download_and_install(&state, &channel, wifi.as_ref()).await;
    // Final status written BEFORE the flag drops: in between, a status still
    // "running" without the flag reads as "interrompue" (power cut) and the
    // phone would stop following an update that actually worked.
    match &result {
        Ok(version) => {
            let msg = format!("{} installée, redémarrage : validation après {} s de fonctionnement", version, TRIAL_CONFIRM_SECS);
            set_status(&state, &channel, false, Some(true), msg).await;
            set_outcome(&state, "essai", Some(version.trim_start_matches("aurion ")));
        }
        Err(e) => {
            set_status(&state, &channel, false, Some(false), e.clone()).await;
            set_outcome(&state, "echec", None);
        }
    }
    state.online_update_running.store(false, Ordering::SeqCst);
    match result {
        Ok(_) => {
            // The restart (install_binary) brings the hotspot back.
            if !state.update.restart {
                back_to_hotspot(&state, wifi.is_some()).await;
            }
        }
        Err(_) => back_to_hotspot(&state, wifi.is_some()).await,
    }
}

async fn back_to_hotspot(state: &AppState, needed: bool) {
    if !needed || !state.system_actions {
        return;
    }
    let net = state.config.read().await.network.clone();
    let _ = crate::sys::helper(&["ap-start", &net.ssid, &net.channel.to_string()], Some(&net.password), Duration::from_secs(60)).await;
}

async fn download_and_install(state: &AppState, channel: &str, wifi: Option<&(String, String)>) -> Result<String, String> {
    if let Some((ssid, password)) = wifi {
        // The phone may need a minute to switch its hotspot on
        let mut joined = false;
        for attempt in 1..=6 {
            match crate::sys::helper(&["wifi-connect", ssid], Some(password), Duration::from_secs(60)).await {
                Ok(_) => {
                    joined = true;
                    break;
                }
                Err(e) => {
                    state.add_log(format!("Wi-Fi « {} » pas encore disponible ({}/6) : {}", ssid, attempt, e)).await;
                    note(state, Level::Warning, &format!("Wi-Fi « {} » pas encore disponible ({}/6) : {}", ssid, attempt, first_line(&e))).await;
                    tokio::time::sleep(Duration::from_secs(20)).await;
                }
            }
        }
        if !joined {
            return Err(format!("Impossible de rejoindre le Wi-Fi « {} » : partage de connexion activé ? mot de passe ?", ssid));
        }
    }

    let api = &state.update.github_api;
    let url = release_url(api, channel).ok_or("Canal inconnu")?;
    let json = crate::sys::run(
        "curl",
        &["-fsSL", "--retry", "3", "--retry-delay", "10", "--max-time", "60", "-H", "Accept: application/vnd.github+json", "-A", "aurion-updater", &url],
        Duration::from_secs(120),
    )
    .await
    .map_err(|e| format!("GitHub injoignable (internet disponible ?) : {}", first_line(&e)))?;
    let tag = serde_json::from_str::<serde_json::Value>(&json)
        .ok()
        .and_then(|v| v["tag_name"].as_str().map(str::to_string))
        .unwrap_or_else(|| "?".into());

    // Settings copied to the USB key before anything changes
    let backup = state.config.read().await.save_usb_backup();
    match backup {
        Ok(()) => note(state, Level::Ok, "Réglages copiés sur la clé USB").await,
        Err(e) => {
            state.add_log(format!("Copie des réglages sur la clé USB impossible avant la mise à jour : {}", e)).await;
            note(state, Level::Warning, &format!("Copie des réglages sur la clé USB impossible : {}", e)).await;
        }
    }
    note(state, Level::Info, &format!("Version trouvée sur GitHub : {}", tag)).await;

    if helper_version(state).await.is_some_and(|v| v >= BUNDLE_HELPER_VERSION) {
        if let Some((bundle_url, sum_url, sig_url)) = bundle_urls(&json, &state.update.download_prefix)? {
            return install_bundle(state, &tag, &bundle_url, &sum_url, &sig_url).await;
        }
        state.add_log(format!("{} ne contient pas de {} : programme seul", tag, BUNDLE_ASSET)).await;
        note(state, Level::Warning, &format!("{} ne contient pas de {} : programme seul, sans ses fichiers système", tag, BUNDLE_ASSET)).await;
    }

    let (tag, binary_url) = asset_url(&json, &state.update.download_prefix)?;
    let sum_url = checksum_url(&json, &state.update.download_prefix)?;
    let sig_url = signature_url(&json, &state.update.download_prefix)?;
    let published_sum = fetch_small(&sum_url).await.map_err(|e| format!("Empreinte de {} introuvable : {}", tag, e))?;
    let data = download(state, &binary_url, &tag).await?;
    check_sha256(&data, &published_sum)?;
    let signature = fetch_small_bytes(state, &sig_url).await.map_err(|e| format!("Signature de {} introuvable : {}", tag, e))?;
    install_binary(state, &data, &signature).await
}

/// Package path: checked here (checksum, signature), then handed over to the
/// root helper, which checks the signature again and installs it.
async fn install_bundle(state: &AppState, tag: &str, url: &str, sum_url: &str, sig_url: &str) -> Result<String, String> {
    if capture_in_progress(state.current_phase().await) {
        return Err("Nuit en cours : mise à jour annulée".into());
    }
    let published_sum = fetch_small(sum_url).await.map_err(|e| format!("Empreinte de {} introuvable : {}", tag, e))?;
    let data = download(state, url, tag).await?;
    check_sha256(&data, &published_sum)?;
    let signature = fetch_small_bytes(state, sig_url).await.map_err(|e| format!("Signature de {} introuvable : {}", tag, e))?;
    crate::web::signing::verify(&data, &signature, &state.update.trusted_keys)?;
    note(state, Level::Ok, &format!("Paquet téléchargé ({} Ko), empreinte et signature vérifiées", data.len() / 1024)).await;

    let dir = &state.paths.tmp_dir;
    let bundle = dir.join(format!("aurion-update-{}.tar.gz", std::process::id()));
    let sig = bundle.with_extension("sig");
    let written = std::fs::write(&bundle, &data).and_then(|_| std::fs::write(&sig, &signature));
    let result = match written {
        Ok(()) => {
            state.add_log(format!("Installation de {} (programme et fichiers système)…", tag)).await;
            crate::sys::helper(
                &["app-update", &bundle.to_string_lossy(), &sig.to_string_lossy()],
                None,
                Duration::from_secs(300),
            )
            .await
            .map_err(|e| format!("Installation de {} refusée : {}", tag, first_line(&e)))
        }
        Err(e) => Err(format!("Écriture du paquet impossible : {}", e)),
    };
    let _ = std::fs::remove_file(&bundle);
    let _ = std::fs::remove_file(&sig);
    let output = result?;
    for line in output.lines().filter(|l| !l.trim().is_empty()) {
        note(state, Level::Info, &format!("installation : {}", line.trim())).await;
    }
    let version = output.lines().last().unwrap_or(tag).trim().to_string();
    state.add_log(format!("Mise à jour installée : {}. Redémarrage…", version)).await;
    crate::web::api::schedule_restart(state);
    Ok(version)
}

/// Installed helper version (None: no helper, or it does not answer).
pub async fn helper_version(state: &AppState) -> Option<u32> {
    if !state.system_actions {
        return None;
    }
    crate::sys::helper(&["version"], None, Duration::from_secs(5)).await.ok()?.trim().parse().ok()
}

/// A small text file (checksum).
async fn fetch_small(url: &str) -> Result<String, String> {
    crate::sys::run(
        "curl",
        &["-fsSL", "--retry", "3", "--retry-delay", "5", "--max-time", "60", "--max-filesize", "4096", "-A", "aurion-updater", url],
        Duration::from_secs(90),
    )
    .await
    .map_err(|e| first_line(&e).to_string())
}

/// A small binary file (signature).
async fn fetch_small_bytes(state: &AppState, url: &str) -> Result<Vec<u8>, String> {
    let tmp = state.paths.tmp_dir.join(format!("aurion-download-{}.sig", std::process::id()));
    let tmp_str = tmp.to_string_lossy().to_string();
    let out = crate::sys::run(
        "curl",
        &["-fsSL", "--retry", "3", "--retry-delay", "5", "--max-time", "60", "--max-filesize", "4096", "-A", "aurion-updater", "-o", &tmp_str, url],
        Duration::from_secs(90),
    )
    .await
    .and_then(|_| std::fs::read(&tmp).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&tmp);
    out.map_err(|e| first_line(&e).to_string())
}

/// Download that survives a network cut: each new attempt resumes where the
/// previous one stopped (`curl -C -`), for a few minutes.
async fn download(state: &AppState, url: &str, tag: &str) -> Result<Vec<u8>, String> {
    let tmp = state.paths.tmp_dir.join(format!("aurion-download-{}", std::process::id()));
    let tmp_str = tmp.to_string_lossy().to_string();
    let max = MAX_BINARY_BYTES.to_string();
    let _ = std::fs::remove_file(&tmp);
    let mut last_error = String::new();
    let mut done = false;
    for attempt in 1..=DOWNLOAD_ATTEMPTS {
        match crate::sys::run(
            "curl",
            &["-fsSL", "-C", "-", "--max-time", "900", "--max-filesize", &max, "-A", "aurion-updater", "-o", &tmp_str, url],
            Duration::from_secs(910),
        )
        .await
        {
            Ok(_) => {
                done = true;
                break;
            }
            Err(e) => {
                last_error = first_line(&e).to_string();
                if attempt < DOWNLOAD_ATTEMPTS {
                    let got = std::fs::metadata(&tmp).map(|m| m.len() / 1024).unwrap_or(0);
                    let msg = format!(
                        "Téléchargement de {} coupé ({} Ko reçus), reprise dans {} s ({}/{}) : {}",
                        tag,
                        got,
                        DOWNLOAD_RETRY_DELAY.as_secs(),
                        attempt,
                        DOWNLOAD_ATTEMPTS,
                        last_error
                    );
                    state.add_log(msg.clone()).await;
                    note(state, Level::Warning, &msg).await;
                    tokio::time::sleep(DOWNLOAD_RETRY_DELAY).await;
                }
            }
        }
    }
    let data = if done { std::fs::read(&tmp).map_err(|e| e.to_string()) } else { Err(last_error) };
    let _ = std::fs::remove_file(&tmp);
    data.map_err(|e| format!("Téléchargement de {} interrompu : {}", tag, e))
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

/// Last update result (Diagnostics page).
pub async fn get_update_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let target = crate::web::api::update_target(&state).ok();
    let has_previous = target.map(|t| t.with_extension("prev").is_file()).unwrap_or(false);
    let helper = if state.system_actions {
        crate::sys::helper(&["version"], None, Duration::from_secs(5)).await.map(|v| v.trim().to_string()).ok()
    } else {
        None
    };
    let helper_outdated = state.system_actions && helper.as_deref() != Some(EXPECTED_HELPER_VERSION);
    let last = read_status(&state);
    let (warnings, errors) = match last.log.is_empty() {
        true => (0, 0),
        false => journal::read(&state, &last.log).await.map(|t| journal::counts(&t)).unwrap_or((0, 0)),
    };
    Json(serde_json::json!({
        "version": crate::VERSION,
        "last": last,
        "warnings": warnings,
        "errors": errors,
        "rollback_available": has_previous,
        "helper_version": helper,
        "helper_outdated": helper_outdated,
    }))
}

/// The result window was read: not shown again.
pub async fn mark_seen(State(state): State<AppState>) -> Json<UpdateStatus> {
    let mut status = read_raw_status(&state);
    if !status.seen {
        status.seen = true;
        write_status(&state, &status);
    }
    Json(read_status(&state))
}

/// Update journals, newest first.
pub async fn list_journals(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "journals": journal::list(&state) }))
}

/// One journal, as plain text.
pub async fn get_journal(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Result<([(axum::http::header::HeaderName, &'static str); 1], String), ApiError> {
    match journal::read(&state, &name).await {
        Some(text) => Ok(([(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], text)),
        None => Err(err(StatusCode::NOT_FOUND, "Journal introuvable")),
    }
}

/// Swap the current binary with the previous one (`.prev`) and restart:
/// a failed trial is undone in one tap, and can be redone the same way.
pub async fn rollback(State(state): State<AppState>) -> Result<String, ApiError> {
    if capture_in_progress(state.current_phase().await) {
        return Err(err(StatusCode::CONFLICT, "Nuit en cours : retour arrière impossible"));
    }
    let target = crate::web::api::update_target(&state).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    // Installed from a package: the root helper puts back the previous
    // application AND system files (exit code 3: no such copy, swap below)
    if helper_version(&state).await.is_some_and(|v| v >= BUNDLE_HELPER_VERSION)
        && crate::sys::helper(&["app-rollback", "manual"], None, Duration::from_secs(120)).await.is_ok()
    {
        let _ = std::fs::remove_file(trial_marker(&target));
        state.add_log("Retour à la version précédente (programme et fichiers système), redémarrage…".into()).await;
        crate::web::api::schedule_restart(&state);
        return Ok("Version précédente rétablie. Rechargez la page dans 15 secondes.".into());
    }
    let previous = target.with_extension("prev");
    if !previous.is_file() {
        return Err(err(StatusCode::NOT_FOUND, "Aucune version précédente conservée"));
    }
    let swap = target.with_extension("swap");
    let io = |e: std::io::Error| err(StatusCode::INTERNAL_SERVER_ERROR, format!("Retour arrière impossible: {}", e));
    std::fs::rename(&target, &swap).map_err(io)?;
    if let Err(e) = std::fs::rename(&previous, &target) {
        let _ = std::fs::rename(&swap, &target);
        return Err(io(e));
    }
    std::fs::rename(&swap, &previous).map_err(io)?;
    // Chosen by hand: no automatic swap back to the version just left
    let _ = std::fs::remove_file(trial_marker(&target));
    state.add_log("Retour à la version précédente, redémarrage…".into()).await;
    crate::web::api::schedule_restart(&state);
    Ok("Version précédente rétablie. Rechargez la page dans 15 secondes.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_must_match_the_published_one() {
        // sha256("abc")
        let abc = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(check_sha256(b"abc", &format!("{}  aurion-arm64\n", abc)).is_ok());
        assert!(check_sha256(b"abc", &abc.to_uppercase()).is_ok(), "hex case does not matter");
        assert!(check_sha256(b"abd", abc).is_err(), "altered file");
        assert!(check_sha256(b"abc", "").is_err(), "empty checksum file");
        assert!(check_sha256(b"abc", "<html>Not Found</html>").is_err(), "error page instead of a checksum");
    }

    const PREFIX: &str = "https://github.com/PercyaDJ/Aurion/releases/download/";

    #[test]
    fn channels() {
        assert_eq!(release_url("https://api.github.com", "stable").unwrap(), "https://api.github.com/repos/PercyaDJ/Aurion/releases/latest");
        assert!(release_url("x", "dev").unwrap().ends_with("/releases/tags/edge"));
        assert!(release_url("x", "nightly; rm").is_none());
    }

    #[test]
    fn update_package_with_its_checksum_and_signature() {
        let a = |n: &str| format!(r#"{{"name":"{n}","browser_download_url":"https://github.com/PercyaDJ/Aurion/releases/download/v1.13.0/{n}"}}"#);
        let full = format!(r#"{{"tag_name":"v1.13.0","assets":[{},{},{},{}]}}"#,
            a("aurion-arm64"), a("aurion-update.tar.gz"), a("aurion-update.tar.gz.sha256"), a("aurion-update.tar.gz.sig"));
        let (bundle, sum, sig) = bundle_urls(&full, PREFIX).unwrap().unwrap();
        assert!(bundle.ends_with("/aurion-update.tar.gz") && sum.ends_with(".sha256") && sig.ends_with(".sig"));
        // Release before 1.13: no package, the bare binary is used
        let old = format!(r#"{{"tag_name":"v1.12.1","assets":[{}]}}"#, a("aurion-arm64"));
        assert_eq!(bundle_urls(&old, PREFIX).unwrap(), None);
        // A package without its signature is never used
        let unsigned = format!(r#"{{"tag_name":"x","assets":[{},{}]}}"#, a("aurion-update.tar.gz"), a("aurion-update.tar.gz.sha256"));
        assert!(bundle_urls(&unsigned, PREFIX).is_err());
        let evil = r#"{"tag_name":"x","assets":[{"name":"aurion-update.tar.gz","browser_download_url":"https://evil.example/p"}]}"#;
        assert!(bundle_urls(evil, PREFIX).is_err());
    }

    #[test]
    fn asset_selection_and_origin_check() {
        let ok = r#"{"tag_name":"edge","assets":[{"name":"aurion-1.8.0.deb","browser_download_url":"https://github.com/PercyaDJ/Aurion/releases/download/edge/a.deb"},
                   {"name":"aurion-arm64","browser_download_url":"https://github.com/PercyaDJ/Aurion/releases/download/edge/aurion-arm64"}]}"#;
        assert_eq!(asset_url(ok, PREFIX).unwrap().1, "https://github.com/PercyaDJ/Aurion/releases/download/edge/aurion-arm64");
        let evil = r#"{"tag_name":"x","assets":[{"name":"aurion-arm64","browser_download_url":"https://evil.example/aurion-arm64"}]}"#;
        assert!(asset_url(evil, PREFIX).is_err());
        let missing = r#"{"tag_name":"v1.5.0","assets":[]}"#;
        assert!(asset_url(missing, PREFIX).unwrap_err().contains("v1.5.0"));
        assert!(asset_url("<html>", PREFIX).is_err());
    }
}
