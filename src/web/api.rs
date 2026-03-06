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
    pub usb_mounted: bool,
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

    let mount_point = config.storage.mount_point.clone();

    // Read real storage stats
    let storage_info = {
        let mount_point = &mount_point;
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

    // Check if USB storage is writable (direct write test — more reliable than mountpoint -q)
    let usb_mounted = {
        let mount_point_path = std::path::Path::new(mount_point.as_str());
        let probe = mount_point_path.join(".aurion_status_probe");
        let writable = std::fs::create_dir_all(mount_point_path).is_ok()
            && std::fs::write(&probe, b"ok").is_ok();
        let _ = std::fs::remove_file(&probe);
        writable
    };

    Json(StatusResponse {
        phase: phase.to_string(),
        time: now.format("%H:%M:%S").to_string(),
        date: now.format("%d/%m/%Y").to_string(),
        storage: storage_info,
        warnings,
        usb_mounted,
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

/// Capture a single preview image from the camera.
/// Uses manual exposure with iterative auto-adjustment (5 rounds):
///   1. Start at midpoint of user's ISO/shutter range
///   2. Capture → histogram → adjust shutter/ISO
///   3. Repeat until properly exposed
///   4. Return final image with metadata overlay
pub async fn capture_preview(State(state): State<AppState>) -> impl IntoResponse {
    let tmp_path = "aurion_preview.jpg";
    let meta_path = "aurion_preview_meta.txt";
    let config = state.config.read().await.clone();

    // Initialize exposure at geometric midpoint of user's range
    let mid_iso = ((config.exposure.iso_min as f64) * (config.exposure.iso_max as f64)).sqrt() as u32;
    let mid_shutter = ((config.exposure.shutter_min_us as f64) * (config.exposure.shutter_max_us as f64)).sqrt() as u64;

    let mut expo = crate::core::exposure::ExposureController::from_config(&config.exposure);
    expo.set(crate::core::models::ExposureSettings::new(mid_iso, mid_shutter));

    // Auto-exposure loop: 5 iterations of capture → analyze → adjust
    for i in 0..5 {
        let settings = expo.current();
        let shutter_us_str = settings.shutter_us.to_string();
        let gain_str = format!("{:.1}", settings.iso as f64 / 100.0);

        let result = std::process::Command::new("rpicam-still")
            .args([
                "--nopreview",
                "-o", tmp_path,
                "-t", "100",
                "--shutter", &shutter_us_str,
                "--gain", &gain_str,
                "--awb", "auto",
                "--metadata", meta_path,
            ])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                // Decode JPEG to RGB for histogram
                if let Ok(jpeg_data) = std::fs::read(tmp_path) {
                    if let Ok(img) = image::load_from_memory(&jpeg_data) {
                        let rgb = img.to_rgb8();
                        let rgb_data = rgb.as_raw();
                        let roi_data = crate::core::orchestrator::crop_roi(
                            rgb_data, rgb.width(), rgb.height(),
                            config.detection.roi_top_percent,
                        );
                        let hist = crate::core::exposure::compute_histogram(&roi_data);
                        let new_settings = expo.update(&hist, crate::core::models::Phase::Calibration);
                        tracing::info!(
                            "Preview auto-expo {}/5: ISO {} / {}µs → next ISO {} / {}µs",
                            i + 1, settings.iso, settings.shutter_us,
                            new_settings.iso, new_settings.shutter_us
                        );
                    }
                }
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                tracing::warn!("Preview capture {}/5 failed: {}", i + 1, stderr);
            }
            Err(e) => {
                tracing::warn!("Preview capture {}/5 error: {}", i + 1, e);
            }
        }
    }

    // Final capture with converged exposure
    let final_settings = expo.current();
    let shutter_us_str = final_settings.shutter_us.to_string();
    let gain_str = format!("{:.1}", final_settings.iso as f64 / 100.0);

    let result = std::process::Command::new("rpicam-still")
        .args([
            "--nopreview",
            "-o", tmp_path,
            "-t", "100",
            "--shutter", &shutter_us_str,
            "--gain", &gain_str,
            "--awb", "auto",
        ])
        .output();

    match result {
        Ok(output) if output.status.success() => {
            match std::fs::read(tmp_path) {
                Ok(data) => {
                    let mut preview = state.latest_preview.write().await;
                    *preview = Some(data.clone());

                    let shutter_display = if final_settings.shutter_us >= 1_000_000 {
                        format!("{}s", final_settings.shutter_us / 1_000_000)
                    } else if final_settings.shutter_us > 0 {
                        format!("1/{}s", 1_000_000 / final_settings.shutter_us)
                    } else {
                        "auto".to_string()
                    };

                    state.add_log(format!(
                        "Preview: ISO {} / {} (auto-expo 5 it.)", final_settings.iso, shutter_display
                    )).await;

                    (
                        StatusCode::OK,
                        [
                            ("content-type", "image/jpeg".to_string()),
                            ("x-aurion-iso", final_settings.iso.to_string()),
                            ("x-aurion-shutter-us", final_settings.shutter_us.to_string()),
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
    let mut cfg = config.clone();
    // Mask WiFi password for security
    cfg.network.password = "********".to_string();
    Json(cfg)
}

/// Returns the real WiFi password (only used by settings page).
pub async fn get_wifi_password(State(state): State<AppState>) -> Json<serde_json::Value> {
    let config = state.config.read().await;
    Json(serde_json::json!({ "password": config.network.password }))
}

#[derive(Deserialize)]
pub struct ConfigUpdate {
    #[serde(flatten)]
    pub config: AppConfig,
}

pub async fn update_config(
    State(state): State<AppState>,
    Json(mut update): Json<AppConfig>,
) -> Result<Json<AppConfig>, (StatusCode, String)> {
    if let Err(e) = update.validate() {
        return Err((StatusCode::BAD_REQUEST, e.to_string()));
    }

    let mut config = state.config.write().await;
    // Preserve real WiFi password if the masked placeholder was sent
    if update.network.password == "********" {
        update.network.password = config.network.password.clone();
    }
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

// ─── Gallery Sessions ────────────────────────────────────

/// One capture session (one night's output).
#[derive(Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub date: String,
    pub image_count: usize,
    pub aurora_count: usize,
    pub total_size_mb: f64,
    pub duration_minutes: u64,
    pub has_log: bool,
}

/// List all sessions from `{mount_point}/sessions/`.
/// Each session is a folder named `YYYY-MM-DD_HH-MM`.
pub async fn get_gallery_sessions(State(state): State<AppState>) -> Json<Vec<SessionInfo>> {
    let config = state.config.read().await;
    let sessions_dir = std::path::Path::new(&config.storage.mount_point).join("sessions");
    drop(config);

    let mut sessions = Vec::new();

    let Ok(entries) = std::fs::read_dir(&sessions_dir) else {
        return Json(sessions);
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        if name.is_empty() {
            continue;
        }

        // Parse date from folder name YYYY-MM-DD_HH-MM
        let date = name.get(..10).unwrap_or(&name).replace('-', "/");

        // Count images and aurora-tagged images
        let mut image_count = 0usize;
        let mut aurora_count = 0usize;
        let mut first_modified: Option<std::time::SystemTime> = None;
        let mut last_modified: Option<std::time::SystemTime> = None;

        // Images are stored at mount_point root, not in session dir
        // session dir contains event.jsonl — count aurora from log
        let has_log = path.join("event.jsonl").exists();

        // Count from event.jsonl if available
        if has_log {
            if let Ok(content) = std::fs::read_to_string(path.join("event.jsonl")) {
                for line in content.lines() {
                    image_count += 1;
                    if line.contains("\"aurora_detected\":true") {
                        aurora_count += 1;
                    }
                }
            }
        }

        // Compute session duration from folder creation + last modified time on event.jsonl
        if has_log {
            if let Ok(meta) = std::fs::metadata(path.join("event.jsonl")) {
                let _ = meta.created().ok().map(|t| first_modified = Some(t));
                let _ = meta.modified().ok().map(|t| last_modified = Some(t));
            }
        }

        let duration_minutes = match (first_modified, last_modified) {
            (Some(start), Some(end)) => {
                end.duration_since(start)
                    .map(|d| d.as_secs() / 60)
                    .unwrap_or(0)
            }
            _ => 0,
        };

        // Estimate total size from image count (rough: ~5 MB/frame for JPG)
        let total_size_mb = image_count as f64 * 5.0;

        sessions.push(SessionInfo {
            name,
            date,
            image_count,
            aurora_count,
            total_size_mb,
            duration_minutes,
            has_log,
        });
    }

    // Sort by session name descending (most recent first)
    sessions.sort_by(|a, b| b.name.cmp(&a.name));

    Json(sessions)
}

// ─── Gallery Delete ───────────────────────────────────────

#[derive(Deserialize)]
pub struct GalleryDeleteRequest {
    pub filenames: Vec<String>,
}

#[derive(Serialize)]
pub struct GalleryDeleteResponse {
    pub deleted: usize,
    pub errors: Vec<String>,
}

pub async fn delete_gallery_images(
    State(state): State<AppState>,
    Json(req): Json<GalleryDeleteRequest>,
) -> Json<GalleryDeleteResponse> {
    let config = state.config.read().await;
    let mount_point = &config.storage.mount_point;
    let mut deleted = 0;
    let mut errors = Vec::new();

    for filename in &req.filenames {
        // Security: reject upward path traversal or absolute paths
        if filename.contains("..") || filename.starts_with('/') || filename.starts_with('\\') {
            errors.push(format!("{}: nom de fichier invalide", filename));
            continue;
        }

        let path = std::path::Path::new(mount_point).join(filename);
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(_) => deleted += 1,
                Err(e) => errors.push(format!("{}: {}", filename, e)),
            }
        } else {
            errors.push(format!("{}: fichier introuvable", filename));
        }
    }

    state.add_log(format!("🗑️ {} image(s) supprimée(s)", deleted)).await;

    Json(GalleryDeleteResponse { deleted, errors })
}

// ─── Gallery ZIP Download ────────────────────────────────

#[derive(Deserialize)]
pub struct GalleryZipRequest {
    pub filenames: String,
}

pub async fn download_gallery_zip(
    State(state): State<AppState>,
    axum::Form(req): axum::Form<GalleryZipRequest>,
) -> impl IntoResponse {
    
    let config = state.config.read().await;
    let mount_point = config.storage.mount_point.clone();
    drop(config);

    let (tx, rx) = tokio::io::duplex(1024 * 1024 * 4); // 4MB buffer pipe

    tokio::spawn(async move {
        // tx is a tokio AsyncWrite. async_zip expects a futures-io AsyncWrite.
        use tokio_util::compat::{TokioAsyncWriteCompatExt, FuturesAsyncWriteCompatExt};
        let compat_tx = tx.compat_write();
        
        let mut zip = async_zip::tokio::write::ZipFileWriter::new(compat_tx);
        
        for filename in req.filenames.split(',') {
            let filename = filename.trim();
            if filename.is_empty() { continue; }
            if filename.contains("..") || filename.contains('/') || filename.contains('\\') {
                continue;
            }
            let path = std::path::Path::new(&mount_point).join(&filename);
            
            if let Ok(mut file) = tokio::fs::File::open(&path).await {
                let builder = async_zip::ZipEntryBuilder::new(filename.into(), async_zip::Compression::Stored);
                if let Ok(entry_writer) = zip.write_entry_stream(builder).await {
                    // entry_writer is a futures-io AsyncWrite. We need a tokio AsyncWrite to use tokio::io::copy
                    let mut tokio_entry_writer = entry_writer.compat_write();
                    let _ = tokio::io::copy(&mut file, &mut tokio_entry_writer).await;
                    let entry_writer = tokio_entry_writer.into_inner();
                    let _ = entry_writer.close().await;
                }
            }
        }

        let _ = zip.close().await;
    });

    let today = chrono::Local::now().format("%Y-%m-%d");
    let zip_name = format!("aurion_{}.zip", today);

    let stream = tokio_util::io::ReaderStream::new(rx);

    (
        StatusCode::OK,
        [
            ("content-type", "application/zip".to_string()),
            ("content-disposition", format!("attachment; filename=\"{}\"", zip_name)),
        ],
        axum::body::Body::from_stream(stream),
    )
}

#[derive(Deserialize)]
pub struct SessionPath {
    pub name: String,
}

/// Download an entire session as a ZIP file
pub async fn download_gallery_session_zip(
    State(state): State<AppState>,
    Path(path): Path<SessionPath>,
) -> impl IntoResponse {
    

    let config = state.config.read().await;
    let mount_point = config.storage.mount_point.clone();
    let session_name = path.name.clone();
    drop(config);

    if session_name.contains("..") || session_name.contains('/') || session_name.contains('\\') {
        return (StatusCode::BAD_REQUEST, [("content-type", "text/plain".to_string())], axum::body::Body::from("Invalid session name")).into_response();
    }

    let session_dir = std::path::Path::new(&mount_point).join("sessions").join(&session_name);
    let event_log_path = session_dir.join("event.jsonl");

    let mut filenames = Vec::new();
    if let Ok(content) = std::fs::read_to_string(&event_log_path) {
        for line in content.lines() {
            if let Some(filename_start) = line.find(r#""filename":""#) {
                let after = &line[filename_start + 12..];
                if let Some(filename_end) = after.find('"') {
                    filenames.push(after[..filename_end].to_string());
                }
            }
        }
    }

    if filenames.is_empty() {
        return (StatusCode::NOT_FOUND, [("content-type", "text/plain".to_string())], axum::body::Body::from("Session is empty or has no logs")).into_response();
    }

    let (tx, rx) = tokio::io::duplex(1024 * 1024 * 4); // 4MB buffer pipe

    tokio::spawn(async move {
        use tokio_util::compat::{TokioAsyncWriteCompatExt, FuturesAsyncWriteCompatExt};
        let compat_tx = tx.compat_write();
        let mut zip = async_zip::tokio::write::ZipFileWriter::new(compat_tx);
        
        // Add images via stream
        for filename in filenames {
            let file_path = std::path::Path::new(&mount_point).join(&filename);
            if let Ok(mut file) = tokio::fs::File::open(&file_path).await {
                let builder = async_zip::ZipEntryBuilder::new(filename.into(), async_zip::Compression::Stored);
                if let Ok(entry_writer) = zip.write_entry_stream(builder).await {
                    let mut tokio_entry_writer = entry_writer.compat_write();
                    let _ = tokio::io::copy(&mut file, &mut tokio_entry_writer).await;
                    let entry_writer = tokio_entry_writer.into_inner();
                    let _ = entry_writer.close().await;
                }
            }
        }

        // Add logs
        for log_file in &["event.jsonl", "session.log"] {
            let log_path = session_dir.join(log_file);
            if let Ok(mut file) = tokio::fs::File::open(&log_path).await {
                let entry_name = format!("sessions/{}/{}", session_name, log_file);
                let builder = async_zip::ZipEntryBuilder::new(entry_name.into(), async_zip::Compression::Stored);
                if let Ok(entry_writer) = zip.write_entry_stream(builder).await {
                    let mut tokio_entry_writer = entry_writer.compat_write();
                    let _ = tokio::io::copy(&mut file, &mut tokio_entry_writer).await;
                    let entry_writer = tokio_entry_writer.into_inner();
                    let _ = entry_writer.close().await;
                }
            }
        }

        let _ = zip.close().await;
    });

    let zip_name = format!("aurion_session_{}.zip", path.name);
    let stream = tokio_util::io::ReaderStream::new(rx);

    (
        StatusCode::OK,
        [
            ("content-type", "application/zip".to_string()),
            ("content-disposition", format!("attachment; filename=\"{}\"", zip_name)),
        ],
        axum::body::Body::from_stream(stream),
    ).into_response()
}

pub async fn delete_gallery_session(
    State(state): State<AppState>,
    Path(session_name): Path<String>,
) -> impl IntoResponse {
    let config = state.config.read().await;
    let mount_point = &config.storage.mount_point;

    // Security: reject any path traversal
    if session_name.contains("..") || session_name.contains('/') || session_name.contains('\\') {
        return (StatusCode::BAD_REQUEST, "Invalid session name").into_response();
    }

    let session_dir = std::path::Path::new(mount_point).join("sessions").join(&session_name);

    if session_dir.exists() {
        match std::fs::remove_dir_all(&session_dir) {
            Ok(_) => {
                state.add_log(format!("🗑️ Session {} supprimée", session_name)).await;
                StatusCode::OK.into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Erreur: {}", e)).into_response()
        }
    } else {
        (StatusCode::NOT_FOUND, "Session introuvable").into_response()
    }
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
        version: env!("CARGO_PKG_VERSION").to_string(),
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

// ─── System Shutdown ──────────────────────────────────────

pub async fn system_shutdown(State(state): State<AppState>) -> StatusCode {
    state.add_log("⏻ Arrêt système demandé…".into()).await;

    // Sync filesystem first
    let _ = std::process::Command::new("sync").output();

    // Schedule shutdown in 3 seconds (gives time for HTTP response)
    let _ = std::process::Command::new("sudo")
        .args(["shutdown", "-h", "+0"])
        .spawn();

    StatusCode::OK
}

// ─── Wi-Fi Client (maintenance mode) ─────────────────────

#[derive(Serialize)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: i32,
    pub security: String,
}

#[derive(Serialize)]
pub struct WifiScanResponse {
    pub networks: Vec<WifiNetwork>,
}

pub async fn wifi_scan() -> Json<WifiScanResponse> {
    let mut networks = Vec::new();

    // Try iwlist first (works without NetworkManager)
    if let Ok(output) = std::process::Command::new("sudo")
        .args(["iwlist", "wlan0", "scan"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut current_ssid = String::new();
        let mut current_signal: i32 = 0;
        let mut current_security = String::from("Open");

        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Cell ") {
                // Save previous
                if !current_ssid.is_empty() {
                    networks.push(WifiNetwork {
                        ssid: current_ssid.clone(),
                        signal: current_signal,
                        security: current_security.clone(),
                    });
                }
                current_ssid.clear();
                current_signal = 0;
                current_security = "Open".into();
            } else if trimmed.starts_with("ESSID:") {
                current_ssid = trimmed
                    .trim_start_matches("ESSID:")
                    .trim_matches('"')
                    .to_string();
            } else if trimmed.starts_with("Signal level=") || trimmed.contains("Signal level=") {
                if let Some(pos) = trimmed.find("Signal level=") {
                    let rest = &trimmed[pos + 13..];
                    current_signal = rest.split_whitespace()
                        .next()
                        .and_then(|s| s.trim_end_matches("dBm").parse().ok())
                        .unwrap_or(0);
                }
            } else if trimmed.contains("WPA") || trimmed.contains("WPA2") {
                current_security = "WPA2".into();
            }
        }
        // Push last
        if !current_ssid.is_empty() {
            networks.push(WifiNetwork {
                ssid: current_ssid,
                signal: current_signal,
                security: current_security,
            });
        }
    }

    // Deduplicate by SSID, keep strongest signal
    networks.sort_by(|a, b| a.ssid.cmp(&b.ssid).then(b.signal.cmp(&a.signal)));
    networks.dedup_by(|a, b| a.ssid == b.ssid);
    // Sort by signal strength (strongest first, closer to 0 is better for negative dBm)
    networks.sort_by(|a, b| b.signal.cmp(&a.signal));

    Json(WifiScanResponse { networks })
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
) -> Json<WifiConnectResponse> {
    state.add_log(format!("📶 Connexion Wi-Fi: {}…", req.ssid)).await;

    // Write wpa_supplicant config
    let wpa_conf = format!(
        "ctrl_interface=DIR=/var/run/wpa_supplicant GROUP=netdev\n\
         update_config=1\n\
         country=FR\n\n\
         network={{\n\
             ssid=\"{}\"\n\
             psk=\"{}\"\n\
             key_mgmt=WPA-PSK\n\
         }}\n",
        req.ssid, req.password
    );

    if let Err(e) = std::fs::write("/tmp/aurion_wpa.conf", &wpa_conf) {
        return Json(WifiConnectResponse {
            success: false,
            message: format!("Erreur écriture config: {}", e),
            ip_address: None,
        });
    }

    // Copy config and reconfigure
    let _ = std::process::Command::new("sudo")
        .args(["cp", "/tmp/aurion_wpa.conf", "/etc/wpa_supplicant/wpa_supplicant.conf"])
        .output();

    // Stop AP mode
    let _ = std::process::Command::new("sudo").args(["killall", "hostapd"]).output();
    let _ = std::process::Command::new("sudo").args(["killall", "dnsmasq"]).output();
    let _ = std::process::Command::new("sudo").args(["ip", "addr", "flush", "dev", "wlan0"]).output();

    // Start wpa_supplicant
    let _ = std::process::Command::new("sudo")
        .args(["wpa_supplicant", "-B", "-i", "wlan0", "-c", "/etc/wpa_supplicant/wpa_supplicant.conf"])
        .output();

    // Request DHCP
    let _ = std::process::Command::new("sudo")
        .args(["dhclient", "wlan0"])
        .output();

    // Wait a bit for IP assignment
    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

    // Get assigned IP
    let ip = get_wlan_ip();

    let success = ip.is_some();
    let message = if success {
        format!("✅ Connecté à {} — IP: {}", req.ssid, ip.as_deref().unwrap_or("?"))
    } else {
        format!("❌ Échec connexion à {}. Vérifiez le mot de passe.", req.ssid)
    };

    state.add_log(message.clone()).await;

    Json(WifiConnectResponse {
        success,
        message,
        ip_address: ip,
    })
}

#[derive(Serialize)]
pub struct WifiStatusResponse {
    pub mode: String,
    pub ssid: Option<String>,
    pub ip_address: Option<String>,
}

pub async fn wifi_status() -> Json<WifiStatusResponse> {
    // Check if hostapd is running (AP mode)
    let ap_active = std::process::Command::new("pgrep")
        .arg("hostapd")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if ap_active {
        return Json(WifiStatusResponse {
            mode: "hotspot".into(),
            ssid: None,
            ip_address: Some("192.168.4.1".into()),
        });
    }

    // Check connected SSID
    let ssid = std::process::Command::new("iwgetid")
        .args(["-r"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    let ip = get_wlan_ip();

    Json(WifiStatusResponse {
        mode: if ssid.is_some() { "client".into() } else { "disconnected".into() },
        ssid,
        ip_address: ip,
    })
}

fn get_wlan_ip() -> Option<String> {
    std::process::Command::new("ip")
        .args(["-4", "addr", "show", "wlan0"])
        .output()
        .ok()
        .and_then(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("inet ") {
                    return trimmed.split_whitespace()
                        .nth(1)
                        .map(|s| s.split('/').next().unwrap_or(s).to_string());
                }
            }
            None
        })
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
