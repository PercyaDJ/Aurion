use axum::{
    extract::State,
    extract::Path,
    http::StatusCode,
    Json,
    response::{IntoResponse, Redirect},
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

    // Read real storage stats
    let storage_info = {
        let mount_point = &config.storage.mount_point;
        match std::process::Command::new("df")
            .args(["--output=size,avail", "-B1", mount_point])
            .output()
        {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = stdout.lines().collect();
                lines.get(1).and_then(|line| {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let total: u64 = parts[0].parse().ok()?;
                        let avail: u64 = parts[1].parse().ok()?;
                        let total_gb = total as f64 / 1_073_741_824.0;
                        let free_gb = avail as f64 / 1_073_741_824.0;
                        let pct = if total > 0 { avail as f64 / total as f64 * 100.0 } else { 0.0 };
                        Some(StorageResponse {
                            total_gb: (total_gb * 10.0).round() / 10.0,
                            free_gb: (free_gb * 10.0).round() / 10.0,
                            free_percent: (pct * 10.0).round() / 10.0,
                            status: if pct > 10.0 { "Ok".into() } else { "Low".into() },
                            summary: format!("{:.0} % libre ({:.1} Go / {:.1} Go)", pct, free_gb, total_gb),
                        })
                    } else {
                        None
                    }
                })
            }
            _ => None,
        }
    };

    Json(StatusResponse {
        phase: phase.to_string(),
        time: now.format("%H:%M:%S").to_string(),
        date: now.format("%d/%m/%Y").to_string(),
        storage: storage_info,
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

/// Capture a single preview image from the camera and store it.
/// Uses current config exposure settings. Returns JPEG with metadata headers.
pub async fn capture_preview(State(state): State<AppState>) -> impl IntoResponse {
    let tmp_path = "/tmp/aurion_preview.jpg";
    let config = state.config.read().await;

    // Use current exposure config for the preview
    let shutter_us = config.exposure.shutter_min_us.to_string();
    let iso_gain = (config.exposure.iso_min as f64 / 100.0).to_string();

    let result = std::process::Command::new("rpicam-still")
        .args([
            "--nopreview",
            "-o", tmp_path,
            "-t", "1",
            "--shutter", &shutter_us,
            "--gain", &iso_gain,
        ])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            match std::fs::read(tmp_path) {
                Ok(data) => {
                    let mut preview = state.latest_preview.write().await;
                    *preview = Some(data.clone());

                    let shutter_display = if config.exposure.shutter_min_us >= 1_000_000 {
                        format!("{}s", config.exposure.shutter_min_us / 1_000_000)
                    } else {
                        format!("1/{}s", 1_000_000 / config.exposure.shutter_min_us.max(1))
                    };

                    state.add_log(format!(
                        "Preview: ISO {} / {}", config.exposure.iso_min, shutter_display
                    )).await;

                    (
                        StatusCode::OK,
                        [
                            ("content-type", "image/jpeg".to_string()),
                            ("x-aurion-iso", config.exposure.iso_min.to_string()),
                            ("x-aurion-shutter-us", config.exposure.shutter_min_us.to_string()),
                        ],
                        data,
                    ).into_response()
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Lecture échouée: {}", e),
                ).into_response(),
            }
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("rpicam-still échouée: {}", stderr),
            ).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("rpicam-still non trouvé: {}", e),
        ).into_response(),
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

pub async fn get_storage(State(state): State<AppState>) -> Json<StorageResponse> {
    let config = state.config.read().await;
    let mount_point = &config.storage.mount_point;

    // Read real disk stats from the configured mount point via df
    let storage = match std::process::Command::new("df")
        .args(["--output=size,avail", "-B1", mount_point])
        .output()
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // df output: header line + data line (values in bytes with -B1)
            let lines: Vec<&str> = stdout.lines().collect();
            if let Some(data_line) = lines.get(1) {
                let parts: Vec<&str> = data_line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let total_bytes: u64 = parts[0].parse().unwrap_or(0);
                    let avail_bytes: u64 = parts[1].parse().unwrap_or(0);
                    let total_gb = total_bytes as f64 / 1_073_741_824.0;
                    let free_gb = avail_bytes as f64 / 1_073_741_824.0;
                    let free_percent = if total_bytes > 0 {
                        (avail_bytes as f64 / total_bytes as f64) * 100.0
                    } else {
                        0.0
                    };

                    StorageResponse {
                        total_gb: (total_gb * 10.0).round() / 10.0,
                        free_gb: (free_gb * 10.0).round() / 10.0,
                        free_percent: (free_percent * 10.0).round() / 10.0,
                        status: if free_percent > 10.0 { "Ok".into() } else { "Low".into() },
                        summary: format!("{:.0} % libre ({:.1} Go / {:.1} Go)", free_percent, free_gb, total_gb),
                    }
                } else {
                    default_storage_error("Lecture df échouée")
                }
            } else {
                default_storage_error("Lecture df échouée")
            }
        }
        _ => default_storage_error(&format!("{} non monté", mount_point)),
    };

    Json(storage)
}

fn default_storage_error(msg: &str) -> StorageResponse {
    StorageResponse {
        total_gb: 0.0,
        free_gb: 0.0,
        free_percent: 0.0,
        status: "Error".into(),
        summary: msg.into(),
    }
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

// ─── Gallery (Recovery Mode) ──────────────────────────────

#[derive(Serialize)]
pub struct GalleryResponse {
    pub images: Vec<GalleryImage>,
    pub total_count: usize,
    pub total_size_mb: f64,
}

#[derive(Serialize)]
pub struct GalleryImage {
    pub filename: String,
    pub size_bytes: u64,
    pub size_display: String,
    pub modified: String,
}

/// List all captured images from the storage mount point.
pub async fn get_gallery(State(state): State<AppState>) -> Json<GalleryResponse> {
    let config = state.config.read().await;
    let mount_point = &config.storage.mount_point;

    let mut images = Vec::new();
    let mut total_size: u64 = 0;

    if let Ok(entries) = std::fs::read_dir(mount_point) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !["jpg", "jpeg", "png", "dng", "raw"].contains(&ext.as_str()) {
                continue;
            }

            if let Ok(metadata) = entry.metadata() {
                let size = metadata.len();
                total_size += size;
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|t| {
                        let dt: chrono::DateTime<chrono::Utc> = t.into();
                        Some(dt.format("%d/%m/%Y %H:%M").to_string())
                    })
                    .unwrap_or_else(|| "--".into());

                let size_display = if size > 1_048_576 {
                    format!("{:.1} Mo", size as f64 / 1_048_576.0)
                } else {
                    format!("{:.0} Ko", size as f64 / 1024.0)
                };

                images.push(GalleryImage {
                    filename: path.file_name().unwrap().to_string_lossy().to_string(),
                    size_bytes: size,
                    size_display,
                    modified,
                });
            }
        }
    }

    // Sort by filename (which includes sequence number)
    images.sort_by(|a, b| a.filename.cmp(&b.filename));

    let total_count = images.len();
    let total_size_mb = total_size as f64 / 1_048_576.0;

    Json(GalleryResponse {
        images,
        total_count,
        total_size_mb,
    })
}

/// Serve a captured image file for download.
pub async fn get_gallery_image(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    let file_path = std::path::Path::new(&config.storage.mount_point).join(&filename);

    // Security: prevent path traversal
    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }

    match std::fs::read(&file_path) {
        Ok(data) => {
            let content_type = match file_path.extension().and_then(|e| e.to_str()) {
                Some("jpg") | Some("jpeg") => "image/jpeg",
                Some("png") => "image/png",
                Some("dng") => "image/x-adobe-dng",
                _ => "application/octet-stream",
            };
            (
                StatusCode::OK,
                [
                    ("content-type", content_type),
                    (
                        "content-disposition",
                        &format!("attachment; filename=\"{}\"", filename),
                    ),
                ],
                data,
            )
                .into_response()
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Serve a thumbnail (small JPG preview) of a captured image.
pub async fn get_gallery_thumbnail(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    let file_path = std::path::Path::new(&config.storage.mount_point).join(&filename);

    if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }

    // For DNG/RAW files, we can't generate thumbnails easily → return a placeholder
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    if ext == "dng" || ext == "raw" {
        return StatusCode::NO_CONTENT.into_response();
    }

    match image::open(&file_path) {
        Ok(img) => {
            let thumb = img.thumbnail(320, 240);
            let mut buffer = Vec::new();
            let mut cursor = std::io::Cursor::new(&mut buffer);
            if thumb
                .write_to(&mut cursor, image::ImageFormat::Jpeg)
                .is_ok()
            {
                (StatusCode::OK, [("content-type", "image/jpeg")], buffer).into_response()
            } else {
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Gallery stats for the home screen.
#[derive(Serialize)]
pub struct GalleryStats {
    pub total_images: usize,
    pub total_size_mb: f64,
    pub last_session_date: Option<String>,
}

pub async fn get_gallery_stats(State(state): State<AppState>) -> Json<GalleryStats> {
    let config = state.config.read().await;
    let mount_point = &config.storage.mount_point;

    let mut total_images: usize = 0;
    let mut total_size: u64 = 0;
    let mut latest_modified: Option<std::time::SystemTime> = None;

    if let Ok(entries) = std::fs::read_dir(mount_point) {
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if !["jpg", "jpeg", "png", "dng", "raw"].contains(&ext.as_str()) {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                total_images += 1;
                total_size += metadata.len();
                if let Ok(modified) = metadata.modified() {
                    if latest_modified.map_or(true, |prev| modified > prev) {
                        latest_modified = Some(modified);
                    }
                }
            }
        }
    }

    let last_session_date = latest_modified.map(|t| {
        let dt: chrono::DateTime<chrono::Utc> = t.into();
        dt.format("%d/%m/%Y").to_string()
    });

    Json(GalleryStats {
        total_images,
        total_size_mb: total_size as f64 / 1_048_576.0,
        last_session_date,
    })
}


// ─── Diagnostics ──────────────────────────────────────────

#[derive(Serialize)]
pub struct DiagnosticsResponse {
    pub platform: String,
    pub hostname: String,
    pub uptime: String,
    pub cpu_temp: Option<f64>,
    pub system_time: String,
    pub system_date: String,
}

pub async fn get_diagnostics(State(_state): State<AppState>) -> Json<DiagnosticsResponse> {
    // Platform
    let platform = if cfg!(feature = "rpi") {
        "Raspberry Pi".to_string()
    } else {
        "PC (dev)".to_string()
    };

    // Hostname
    let hostname = std::process::Command::new("hostname")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "?".into());

    // Uptime
    let uptime = std::fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|s| s.split_whitespace().next().map(String::from))
        .and_then(|s| s.parse::<f64>().ok())
        .map(|secs| {
            let h = secs as u64 / 3600;
            let m = (secs as u64 % 3600) / 60;
            format!("{}h {:02}min", h, m)
        })
        .unwrap_or_else(|| "--".into());

    // CPU temperature
    let cpu_temp = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp")
        .ok()
        .and_then(|s| s.trim().parse::<f64>().ok())
        .map(|t| (t / 1000.0 * 10.0).round() / 10.0);

    let now = chrono::Local::now();

    Json(DiagnosticsResponse {
        platform,
        hostname,
        uptime,
        cpu_temp,
        system_time: now.format("%H:%M:%S").to_string(),
        system_date: now.format("%d/%m/%Y").to_string(),
    })
}

// ─── Set System Time ──────────────────────────────────────

#[derive(Deserialize)]
pub struct SetTimeRequest {
    /// Format: "2026-02-22T10:00:00"
    pub datetime: String,
}

pub async fn set_system_time(
    State(state): State<AppState>,
    Json(req): Json<SetTimeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    // Use sudo date to set system time (sudoers must allow /bin/date)
    let result = std::process::Command::new("sudo")
        .args(["date", "-s", &req.datetime])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            state.add_log(format!("Heure système réglée: {}", req.datetime)).await;
            Ok(StatusCode::OK)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Erreur: {}", stderr)))
        }
        Err(e) => Err((StatusCode::INTERNAL_SERVER_ERROR, format!("Erreur: {}", e))),
    }
}

// ─── Captive Portal Detection ─────────────────────────────
//
// When a device connects to the Aurion Wi-Fi, the OS tries to
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
