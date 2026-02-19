use axum::{
    extract::State,
    extract::Path,
    http::StatusCode,
    Json,
    response::{IntoResponse, Redirect, Html},
};
use serde::{Deserialize, Serialize};

use crate::core::config::{AppConfig, Preset};
use crate::core::models::Phase;
use crate::web::AppState;

// ─── Status ────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatusResponse {
    pub phase: String,
    pub time: String,
    pub date: String,
    pub storage: Option<StorageResponse>,
    pub warnings: Vec<String>,
}

pub async fn get_status(State(state): State<AppState>) -> Json<StatusResponse> {
    let phase = state.phase.read().await;
    let config = state.config.read().await;
    let now = chrono::Local::now();

    let mut warnings = Vec::new();

    // Check ISO warning
    if config.exposure.iso_max > 1600 {
        warnings.push("ISO élevé : risque de bruit capteur".into());
    }

    // Check star trail rule (500 rule with crop factor 5.6 for RPi HQ)
    let max_shutter_secs = config.capture.focal_length_mm * 5.6;
    let max_shutter_rule = (500.0 / max_shutter_secs) as u64 * 1_000_000;
    if config.exposure.shutter_max_us > max_shutter_rule {
        warnings.push("Règle 500 dépassée : risque d'étoiles filées".into());
    }

    Json(StatusResponse {
        phase: phase.to_string(),
        time: now.format("%H:%M:%S").to_string(),
        date: now.format("%d/%m/%Y").to_string(),
        storage: None, // Will be filled when storage adapter is wired
        warnings,
    })
}

// ─── Preview ───────────────────────────────────────────────

pub async fn get_preview(State(state): State<AppState>) -> impl IntoResponse {
    let preview = state.latest_preview.read().await;
    match preview.as_ref() {
        Some(data) => (
            StatusCode::OK,
            [("content-type", "image/jpeg")],
            data.clone(),
        )
            .into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

// ─── Config ────────────────────────────────────────────────

pub async fn get_config(State(state): State<AppState>) -> Json<AppConfig> {
    let config = state.config.read().await;
    Json(config.clone())
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    #[serde(flatten)]
    pub config: AppConfig,
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(update): Json<AppConfig>,
) -> Result<Json<AppConfig>, (StatusCode, String)> {
    if let Err(e) = update.validate() {
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    let mut config = state.config.write().await;
    *config = update;

    // Persist
    if let Err(e) = config.save(&AppConfig::default_path()) {
        tracing::error!("Failed to persist config: {}", e);
    }

    state
        .add_log(format!("Configuration mise à jour"))
        .await;

    Ok(Json(config.clone()))
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

pub async fn get_presets(State(_state): State<AppState>) -> Json<PresetsResponse> {
    let mut presets: Vec<PresetInfo> = AppConfig::builtin_presets()
        .iter()
        .map(|p| PresetInfo {
            name: p.name.clone(),
            is_builtin: p.is_builtin,
        })
        .collect();

    // Load user presets from disk
    let presets_dir = AppConfig::presets_dir();
    if presets_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&presets_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Some(name) = path.file_stem() {
                        let name = name.to_string_lossy().to_string();
                        if !presets.iter().any(|p| p.name == name) {
                            presets.push(PresetInfo {
                                name,
                                is_builtin: false,
                            });
                        }
                    }
                }
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
) -> Result<StatusCode, (StatusCode, String)> {
    let config = state.config.read().await;
    let preset = Preset {
        name: req.name.clone(),
        config: config.clone(),
        is_builtin: false,
    };

    let presets_dir = AppConfig::presets_dir();
    std::fs::create_dir_all(&presets_dir).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    let path = presets_dir.join(format!("{}.json", req.name));
    let content = serde_json::to_string_pretty(&preset).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    std::fs::write(&path, content).map_err(|e| {
        (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    })?;

    state
        .add_log(format!("Preset '{}' sauvegardé", req.name))
        .await;

    Ok(StatusCode::CREATED)
}

pub async fn apply_preset(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<AppConfig>, (StatusCode, String)> {
    // Check builtin presets first
    let builtin = AppConfig::builtin_presets();
    if let Some(preset) = builtin.iter().find(|p| p.name == name) {
        let mut config = state.config.write().await;
        *config = preset.config.clone();
        state
            .add_log(format!("Preset '{}' appliqué", name))
            .await;
        return Ok(Json(config.clone()));
    }

    // Check user presets
    let path = AppConfig::presets_dir().join(format!("{}.json", name));
    if path.exists() {
        let content = std::fs::read_to_string(&path).map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        })?;
        let preset: Preset = serde_json::from_str(&content).map_err(|e| {
            (StatusCode::BAD_REQUEST, e.to_string())
        })?;

        let mut config = state.config.write().await;
        *config = preset.config;
        state
            .add_log(format!("Preset '{}' appliqué", name))
            .await;
        return Ok(Json(config.clone()));
    }

    Err((StatusCode::NOT_FOUND, format!("Preset '{}' introuvable", name)))
}

// ─── Disconnect ────────────────────────────────────────────

pub async fn disconnect(State(state): State<AppState>) -> StatusCode {
    state
        .add_log("Déconnexion demandée — compte à rebours 15s".into())
        .await;

    // The actual disconnect logic (15s timer, AP shutdown) will be handled
    // by the orchestrator, not the web handler.
    let mut phase = state.phase.write().await;
    *phase = Phase::Disconnect;

    StatusCode::OK
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

pub async fn get_storage(State(_state): State<AppState>) -> Json<StorageResponse> {
    // Placeholder — will be connected to StoragePort
    Json(StorageResponse {
        total_gb: 128.0,
        free_gb: 90.0,
        free_percent: 70.3,
        status: "Ok".into(),
        summary: "70 % libre (90 Go / 128 Go)".into(),
    })
}

// ─── Logs ──────────────────────────────────────────────────

#[derive(Serialize)]
pub struct LogsResponse {
    pub logs: Vec<String>,
}

pub async fn get_logs(State(state): State<AppState>) -> Json<LogsResponse> {
    let logs = state.log_buffer.read().await;
    Json(LogsResponse {
        logs: logs.clone(),
    })
}

// ─── Captive Portal Detection ─────────────────────────────
//
// When a device connects to the AuroraCam Wi-Fi, the OS tries to
// reach known URLs to check internet connectivity. If it gets a
// redirect instead of the expected response, it opens a captive
// portal browser window automatically.
//
// iOS/macOS:  GET /hotspot-detect.html → expects "Success"
// Android:    GET /generate_204       → expects 204
// Windows:    GET /connecttest.txt     → expects "Microsoft Connect Test"
// Firefox:    GET /canonical.html      → expects 200 with specific content
//
// We redirect them all to http://192.168.4.1:8080/

const PORTAL_REDIRECT: &str = "http://192.168.4.1:8080/";

/// iOS / macOS captive portal detection
pub async fn captive_apple() -> impl IntoResponse {
    Redirect::temporary(PORTAL_REDIRECT)
}

/// Android captive portal detection (expects 204, gets 302)
pub async fn captive_android() -> impl IntoResponse {
    Redirect::temporary(PORTAL_REDIRECT)
}

/// Windows NCSI captive portal detection
pub async fn captive_windows() -> impl IntoResponse {
    Redirect::temporary(PORTAL_REDIRECT)
}

/// Firefox captive portal detection
pub async fn captive_firefox() -> impl IntoResponse {
    Redirect::temporary(PORTAL_REDIRECT)
}

/// Fallback: any unknown host request gets redirected if it's
/// a connectivity check (common Android/Samsung variants)
pub async fn captive_fallback() -> impl IntoResponse {
    Redirect::temporary(PORTAL_REDIRECT)
}
