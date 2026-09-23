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

use std::sync::atomic::Ordering;
use std::time::Duration;

use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::core::config::PASSWORD_MASK;
use crate::core::validate;
use crate::web::api::{capture_in_progress, err, install_binary, require_system_actions, ApiError};
use crate::web::AppState;

/// GitHub repository of the project (public: no token needed).
pub const REPO: &str = "PercyaDJ/Aurion";
/// Release asset holding the bare arm64 binary.
pub const ASSET: &str = "aurion-arm64";
/// `aurion-helper version` this application expects. An online update
/// replaces the application only: a new helper (root) needs the SD image or
/// the .deb once.
pub const EXPECTED_HELPER_VERSION: &str = "3";
/// Largest binary accepted (the real one is a few MB).
const MAX_BINARY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UpdateStatus {
    pub running: bool,
    /// Local time of the last change.
    pub at: String,
    pub ok: Option<bool>,
    pub channel: String,
    pub message: String,
}

fn status_path(state: &AppState) -> std::path::PathBuf {
    state.paths.config_file.with_file_name("update_status.json")
}

pub fn read_status(state: &AppState) -> UpdateStatus {
    let mut status: UpdateStatus = std::fs::read(status_path(state))
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default();
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
    write_status(state, &UpdateStatus {
        running,
        at: chrono::Local::now().format("%d/%m %H:%M").to_string(),
        ok,
        channel: channel.to_string(),
        message,
    });
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
    let url = v["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| a["name"] == ASSET)
        .and_then(|a| a["browser_download_url"].as_str())
        .ok_or_else(|| format!("La version {} ne contient pas le fichier {}", tag, ASSET))?;
    if !url.starts_with(allowed_prefix) || url.contains("..") || url.chars().any(|c| c.is_whitespace()) {
        return Err("Adresse de téléchargement inattendue : refusée".into());
    }
    Ok((tag, url.to_string()))
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
    state.online_update_running.store(false, Ordering::SeqCst);
    match result {
        Ok(version) => {
            set_status(&state, &channel, false, Some(true), format!("{} installée, redémarrage", version)).await;
            // The restart (install_binary) brings the hotspot back.
            if !state.update.restart {
                back_to_hotspot(&state, wifi.is_some()).await;
            }
        }
        Err(e) => {
            set_status(&state, &channel, false, Some(false), e).await;
            back_to_hotspot(&state, wifi.is_some()).await;
        }
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
        &["-fsSL", "--max-time", "60", "-H", "Accept: application/vnd.github+json", "-A", "aurion-updater", &url],
        Duration::from_secs(70),
    )
    .await
    .map_err(|e| format!("GitHub injoignable (internet disponible ?) : {}", first_line(&e)))?;
    let (tag, binary_url) = asset_url(&json, &state.update.download_prefix)?;

    let tmp = state.paths.tmp_dir.join(format!("aurion-download-{}", std::process::id()));
    let tmp_str = tmp.to_string_lossy().to_string();
    let max = MAX_BINARY_BYTES.to_string();
    let downloaded = crate::sys::run(
        "curl",
        &["-fsSL", "--max-time", "900", "--max-filesize", &max, "-A", "aurion-updater", "-o", &tmp_str, &binary_url],
        Duration::from_secs(910),
    )
    .await;
    let data = downloaded.and_then(|_| std::fs::read(&tmp).map_err(|e| e.to_string()));
    let _ = std::fs::remove_file(&tmp);
    let data = data.map_err(|e| format!("Téléchargement de {} interrompu : {}", tag, first_line(&e)))?;
    install_binary(state, &data).await
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
    Json(serde_json::json!({
        "version": crate::VERSION,
        "last": read_status(&state),
        "rollback_available": has_previous,
        "helper_version": helper,
        "helper_outdated": helper_outdated,
    }))
}

/// Swap the current binary with the previous one (`.prev`) and restart:
/// a failed trial is undone in one tap, and can be redone the same way.
pub async fn rollback(State(state): State<AppState>) -> Result<String, ApiError> {
    if capture_in_progress(state.current_phase().await) {
        return Err(err(StatusCode::CONFLICT, "Nuit en cours : retour arrière impossible"));
    }
    let target = crate::web::api::update_target(&state).map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e))?;
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
    state.add_log("Retour à la version précédente, redémarrage…".into()).await;
    crate::web::api::schedule_restart(&state);
    Ok("Version précédente rétablie. Rechargez la page dans 15 secondes.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "https://github.com/PercyaDJ/Aurion/releases/download/";

    #[test]
    fn channels() {
        assert_eq!(release_url("https://api.github.com", "stable").unwrap(), "https://api.github.com/repos/PercyaDJ/Aurion/releases/latest");
        assert!(release_url("x", "dev").unwrap().ends_with("/releases/tags/edge"));
        assert!(release_url("x", "nightly; rm").is_none());
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
