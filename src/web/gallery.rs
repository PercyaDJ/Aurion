//! Gallery / recovery mode: list, preview, download (ZIP) and delete the
//! captured images stored on the USB drive.
//!
//! Layout on the drive (kept compatible with existing captures):
//! ```text
//! /mnt/capture/aurora_YYYYMMDD_HHMMSS_NNNNN[_AURORA].jpg|dng
//! /mnt/capture/thumbs/<image>.jpg                 (320×240, JPG only)
//! /mnt/capture/sessions/YYYY-MM-DD_HH-MM/event.jsonl, session.log
//! ```
//! Every name received from the network is checked with
//! [`validate::is_safe_image_name`] / [`validate::is_safe_name`]: no path
//! can escape the capture directory.

use std::collections::BTreeMap;
use std::path::{Path as FsPath, PathBuf};
use std::sync::OnceLock;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use crate::core::validate;
use crate::web::AppState;

/// Margin before a session start during which images still belong to it.
const SESSION_START_MARGIN_MIN: i64 = 5;
/// A session never spans more than this.
const SESSION_MAX_HOURS: i64 = 16;
/// Width of thumbnails generated on demand.
const THUMB_WIDTH: u32 = 320;
const THUMB_HEIGHT: u32 = 240;

async fn mount_point(state: &AppState) -> PathBuf {
    PathBuf::from(&state.config.read().await.storage.mount_point)
}

fn bad_request(msg: &'static str) -> Response {
    (StatusCode::BAD_REQUEST, msg).into_response()
}

// ─── Listing ───────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct GalleryImage {
    pub filename: String,
    pub size_bytes: u64,
    pub size_display: String,
    pub modified: String,
    pub aurora: bool,
}

#[derive(Serialize)]
pub struct GalleryResponse {
    pub images: Vec<GalleryImage>,
    pub total_count: usize,
    pub total_size_mb: f64,
}

fn size_display(size: u64) -> String {
    if size > 1_048_576 {
        format!("{:.1} Mo", size as f64 / 1_048_576.0)
    } else {
        format!("{:.0} Ko", size as f64 / 1024.0)
    }
}

/// Images at the root of the capture directory, sorted by name.
/// Files with unsafe names are ignored (they could not be served safely).
pub fn list_images(mount: &FsPath) -> Vec<(GalleryImage, std::time::SystemTime)> {
    let mut images = Vec::new();
    let Ok(entries) = std::fs::read_dir(mount) else {
        return images;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if !validate::is_safe_image_name(&name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified_time = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let modified: chrono::DateTime<chrono::Local> = modified_time.into();
        images.push((
            GalleryImage {
                aurora: name.contains("_AURORA"),
                size_display: size_display(meta.len()),
                size_bytes: meta.len(),
                modified: modified.format("%d/%m/%Y %H:%M").to_string(),
                filename: name,
            },
            modified_time,
        ));
    }
    images.sort_by(|a, b| a.0.filename.cmp(&b.0.filename));
    images
}

pub async fn get_gallery(State(state): State<AppState>) -> Json<GalleryResponse> {
    let mount = mount_point(&state).await;
    let images: Vec<GalleryImage> = tokio::task::spawn_blocking(move || list_images(&mount))
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(i, _)| i)
        .collect();
    let total_size: u64 = images.iter().map(|i| i.size_bytes).sum();
    Json(GalleryResponse {
        total_count: images.len(),
        total_size_mb: total_size as f64 / 1_048_576.0,
        images,
    })
}

#[derive(Serialize)]
pub struct GalleryStats {
    pub total_images: usize,
    pub total_size_mb: f64,
    pub last_session_date: Option<String>,
}

pub async fn get_gallery_stats(State(state): State<AppState>) -> Json<GalleryStats> {
    let mount = mount_point(&state).await;
    let images = tokio::task::spawn_blocking(move || list_images(&mount)).await.unwrap_or_default();
    let total_size: u64 = images.iter().map(|(i, _)| i.size_bytes).sum();
    let last = images.iter().map(|(_, t)| *t).max().map(|t| {
        let dt: chrono::DateTime<chrono::Local> = t.into();
        dt.format("%d/%m/%Y").to_string()
    });
    Json(GalleryStats {
        total_images: images.len(),
        total_size_mb: total_size as f64 / 1_048_576.0,
        last_session_date: last,
    })
}

// ─── Sessions ──────────────────────────────────────────────

/// `aurora_YYYYMMDD_HHMMSS_...` → timestamp.
pub fn parse_image_timestamp(name: &str) -> Option<NaiveDateTime> {
    let rest = name.strip_prefix("aurora_")?;
    let ts = rest.get(..15)?;
    NaiveDateTime::parse_from_str(ts, "%Y%m%d_%H%M%S").ok()
}

/// `YYYY-MM-DD_HH-MM` → session start.
pub fn parse_session_start(name: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(&format!("{}-00", name.get(..16)?), "%Y-%m-%d_%H-%M-%S").ok()
}

/// Group image names by session. An image belongs to the latest session
/// that started (minus a small margin) before it, as long as the next
/// session has not started and the session is not older than 16 h.
pub fn assign_images_to_sessions(sessions: &[String], images: &[String]) -> BTreeMap<String, Vec<String>> {
    let margin = chrono::TimeDelta::minutes(SESSION_START_MARGIN_MIN);
    let max_len = chrono::TimeDelta::hours(SESSION_MAX_HOURS);

    let mut starts: Vec<(NaiveDateTime, &String)> = sessions
        .iter()
        .filter_map(|s| parse_session_start(s).map(|t| (t, s)))
        .collect();
    starts.sort();

    let mut result: BTreeMap<String, Vec<String>> = sessions.iter().map(|s| (s.clone(), Vec::new())).collect();
    for image in images {
        let Some(ts) = parse_image_timestamp(image) else { continue };
        let idx = starts.iter().rposition(|(start, _)| *start - margin <= ts);
        if let Some(i) = idx {
            let (start, name) = starts[i];
            if ts <= start + max_len {
                result.get_mut(name.as_str()).expect("session present").push(image.clone());
            }
        }
    }
    for files in result.values_mut() {
        files.sort();
    }
    result
}

fn session_names(mount: &FsPath) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(mount.join("sessions")) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| validate::is_safe_name(n))
        .collect();
    names.sort();
    names
}

/// Images belonging to one session.
pub fn session_files(mount: &FsPath, session: &str) -> Vec<String> {
    let sessions = session_names(mount);
    let images: Vec<String> = list_images(mount).into_iter().map(|(i, _)| i.filename).collect();
    assign_images_to_sessions(&sessions, &images).remove(session).unwrap_or_default()
}

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

fn compute_sessions(mount: &FsPath) -> Vec<SessionInfo> {
    let names = session_names(mount);
    let images = list_images(mount);
    let sizes: BTreeMap<String, u64> = images.iter().map(|(i, _)| (i.filename.clone(), i.size_bytes)).collect();
    let image_names: Vec<String> = sizes.keys().cloned().collect();
    let grouped = assign_images_to_sessions(&names, &image_names);

    let mut sessions: Vec<SessionInfo> = grouped
        .into_iter()
        .map(|(name, files)| {
            let dir = mount.join("sessions").join(&name);
            let has_log = dir.join("event.jsonl").exists() || dir.join("session.log").exists();
            let stamps: Vec<NaiveDateTime> = files.iter().filter_map(|f| parse_image_timestamp(f)).collect();
            let duration_minutes = match (stamps.iter().min(), stamps.iter().max()) {
                (Some(a), Some(b)) => (*b - *a).num_minutes().max(0) as u64,
                _ => 0,
            };
            // Count one aurora per capture even with RAW+JPG pairs.
            let mut aurora_bases: Vec<&str> = files
                .iter()
                .filter(|f| f.contains("_AURORA"))
                .map(|f| f.rsplit_once('.').map(|(b, _)| b).unwrap_or(f))
                .collect();
            aurora_bases.dedup();
            SessionInfo {
                date: name.get(..10).unwrap_or(&name).replace('-', "/"),
                image_count: files.len(),
                aurora_count: aurora_bases.len(),
                total_size_mb: files.iter().map(|f| sizes.get(f).copied().unwrap_or(0)).sum::<u64>() as f64 / 1_048_576.0,
                duration_minutes,
                has_log,
                name,
            }
        })
        // A folder with neither images nor logs carries no information.
        .filter(|s| s.image_count > 0 || s.has_log)
        .collect();
    sessions.sort_by(|a, b| b.name.cmp(&a.name));
    sessions
}

/// List capture sessions (most recent first). Read-only: listing never
/// deletes anything (a session of a night without aurora only has logs).
pub async fn get_gallery_sessions(State(state): State<AppState>) -> Json<Vec<SessionInfo>> {
    let mount = mount_point(&state).await;
    Json(tokio::task::spawn_blocking(move || compute_sessions(&mount)).await.unwrap_or_default())
}

// ─── Strongest auroras ─────────────────────────────────────

/// `aurora_YYYYMMDD_HHMMSS_NNNNN...` → frame number NNNNN.
pub fn parse_frame_number(name: &str) -> Option<u64> {
    name.get(23..28).filter(|_| name.starts_with("aurora_")).and_then(|n| n.parse().ok())
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct BestAurora {
    pub session: String,
    /// JPEG to display (may be absent in RAW-only mode).
    pub jpg: Option<String>,
    /// DNG to edit.
    pub dng: Option<String>,
    pub score: f64,
    pub color: String,
    pub timestamp: String,
}

#[derive(Deserialize)]
pub struct BestQuery {
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Rank the saved frames of every session by aurora score (read from the
/// `event.jsonl` log): the photographer picks the strongest ones to edit.
pub fn best_auroras(mount: &FsPath, limit: usize) -> Vec<BestAurora> {
    let sessions = session_names(mount);
    let images: Vec<String> = list_images(mount).into_iter().map(|(i, _)| i.filename).collect();
    let grouped = assign_images_to_sessions(&sessions, &images);
    let mut out = Vec::new();

    for (session, files) in grouped {
        let Ok(log) = std::fs::read_to_string(mount.join("sessions").join(&session).join("event.jsonl")) else {
            continue;
        };
        // frame number → (score, colour, timestamp)
        let mut scores: BTreeMap<u64, (f64, String, String)> = BTreeMap::new();
        for line in log.lines() {
            let Ok(ev) = serde_json::from_str::<crate::core::models::SessionEvent>(line) else { continue };
            if let Some(n) = ev.frame_number {
                scores.insert(n, (ev.aurora_score, ev.aurora_color, ev.timestamp));
            }
        }
        // Group the JPG and DNG of the same frame (stacked images excluded)
        let mut frames: BTreeMap<u64, (Option<String>, Option<String>)> = BTreeMap::new();
        for f in files.iter().filter(|f| !f.contains("_STACK")) {
            let Some(n) = parse_frame_number(f) else { continue };
            let entry = frames.entry(n).or_default();
            match validate::extension_lower(f).as_deref() {
                Some("dng") | Some("raw") => entry.1 = Some(f.clone()),
                _ => entry.0 = Some(f.clone()),
            }
        }
        for (n, (jpg, dng)) in frames {
            if let Some((score, color, timestamp)) = scores.get(&n) {
                if *score > 0.0 {
                    out.push(BestAurora {
                        session: session.clone(),
                        jpg,
                        dng,
                        score: *score,
                        color: color.clone(),
                        timestamp: timestamp.clone(),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    out
}

pub async fn get_best_auroras(State(state): State<AppState>, Query(q): Query<BestQuery>) -> Json<Vec<BestAurora>> {
    let mount = mount_point(&state).await;
    let limit = q.limit.unwrap_or(30).clamp(1, 500);
    Json(tokio::task::spawn_blocking(move || best_auroras(&mount, limit)).await.unwrap_or_default())
}

// ─── Single image ──────────────────────────────────────────

fn content_type_for(name: &str) -> &'static str {
    match validate::extension_lower(name).as_deref() {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("dng") => "image/x-adobe-dng",
        _ => "application/octet-stream",
    }
}

#[derive(Deserialize, Default)]
pub struct ImageQuery {
    /// Display in the browser instead of downloading.
    #[serde(default)]
    pub inline: Option<u8>,
}

/// Stream a captured image (never loaded fully in memory).
pub async fn get_gallery_image(
    State(state): State<AppState>,
    Path(filename): Path<String>,
    Query(q): Query<ImageQuery>,
) -> Response {
    if !validate::is_safe_image_name(&filename) {
        return bad_request("Nom de fichier invalide");
    }
    let path = mount_point(&state).await.join(&filename);
    let file = match tokio::fs::File::open(&path).await {
        Ok(f) => f,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    let len = file.metadata().await.map(|m| m.len()).unwrap_or(0);
    let disposition = if q.inline.unwrap_or(0) == 1 { "inline" } else { "attachment" };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type_for(&filename).to_string()),
            (header::CONTENT_LENGTH, len.to_string()),
            (header::CONTENT_DISPOSITION, format!("{}; filename=\"{}\"", disposition, filename)),
        ],
        Body::from_stream(tokio_util::io::ReaderStream::new(file)),
    )
        .into_response()
}

fn thumb_semaphore() -> &'static Semaphore {
    // Decoding a 12 MP JPEG takes ~1 s and ~40 MB on a Pi 4: limit concurrency.
    static SEM: OnceLock<Semaphore> = OnceLock::new();
    SEM.get_or_init(|| Semaphore::new(2))
}

fn make_thumbnail(source: &FsPath) -> Option<Vec<u8>> {
    let img = image::open(source).ok()?;
    let thumb = img.thumbnail(THUMB_WIDTH, THUMB_HEIGHT);
    let mut buffer = std::io::Cursor::new(Vec::new());
    thumb.to_rgb8().write_to(&mut buffer, image::ImageFormat::Jpeg).ok()?;
    Some(buffer.into_inner())
}

fn jpeg(data: Vec<u8>) -> Response {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "public, max-age=86400")],
        data,
    )
        .into_response()
}

/// Thumbnail of a captured image: the 320×240 file written during the
/// capture if present, otherwise generated once and cached.
pub async fn get_gallery_thumbnail(State(state): State<AppState>, Path(filename): Path<String>) -> Response {
    if !validate::is_safe_image_name(&filename) {
        return bad_request("Nom de fichier invalide");
    }
    if matches!(validate::extension_lower(&filename).as_deref(), Some("dng") | Some("raw")) {
        return StatusCode::NO_CONTENT.into_response();
    }
    let mount = mount_point(&state).await;

    if let Ok(data) = tokio::fs::read(mount.join("thumbs").join(&filename)).await {
        return jpeg(data);
    }
    let cache_path = state.paths.thumb_cache_dir.join(&filename);
    if let Ok(data) = tokio::fs::read(&cache_path).await {
        return jpeg(data);
    }

    let source = mount.join(&filename);
    if !source.is_file() {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(_permit) = thumb_semaphore().acquire().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    let cache_dir = state.paths.thumb_cache_dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        let data = make_thumbnail(&source)?;
        let _ = std::fs::create_dir_all(&cache_dir);
        let _ = std::fs::write(&cache_path, &data);
        Some(data)
    })
    .await;

    match result {
        Ok(Some(data)) => jpeg(data),
        _ => (StatusCode::UNPROCESSABLE_ENTITY, "Image illisible").into_response(),
    }
}

// ─── Delete ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct GalleryDeleteRequest {
    pub filenames: Vec<String>,
}

#[derive(Serialize)]
pub struct GalleryDeleteResponse {
    pub deleted: usize,
    pub errors: Vec<String>,
}

/// Remove an image and its thumbnails. Returns Ok(true) if it existed.
fn remove_image(mount: &FsPath, cache_dir: &FsPath, name: &str) -> std::io::Result<bool> {
    let path = mount.join(name);
    if !path.is_file() {
        return Ok(false);
    }
    std::fs::remove_file(&path)?;
    let _ = std::fs::remove_file(mount.join("thumbs").join(name));
    let _ = std::fs::remove_file(cache_dir.join(name));
    Ok(true)
}

pub async fn delete_gallery_images(
    State(state): State<AppState>,
    Json(req): Json<GalleryDeleteRequest>,
) -> Json<GalleryDeleteResponse> {
    let mount = mount_point(&state).await;
    let mut deleted = 0;
    let mut errors = Vec::new();

    for filename in &req.filenames {
        if !validate::is_safe_image_name(filename) {
            tracing::warn!("Delete rejected: invalid filename {:?}", filename);
            errors.push(format!("{}: nom de fichier invalide", filename));
            continue;
        }
        match remove_image(&mount, &state.paths.thumb_cache_dir, filename) {
            Ok(true) => deleted += 1,
            Ok(false) => errors.push(format!("{}: fichier introuvable", filename)),
            Err(e) => errors.push(format!("{}: {}", filename, e)),
        }
    }

    state.add_log(format!("{} image(s) supprimée(s)", deleted)).await;
    Json(GalleryDeleteResponse { deleted, errors })
}

pub async fn delete_gallery_session(State(state): State<AppState>, Path(session): Path<String>) -> Response {
    if !validate::is_safe_name(&session) {
        return bad_request("Nom de session invalide");
    }
    let mount = mount_point(&state).await;
    let session_dir = mount.join("sessions").join(&session);
    if !session_dir.is_dir() {
        return (StatusCode::NOT_FOUND, "Session introuvable").into_response();
    }

    let mut deleted = 0;
    for name in session_files(&mount, &session) {
        if let Ok(true) = remove_image(&mount, &state.paths.thumb_cache_dir, &name) {
            deleted += 1;
        }
    }
    if let Err(e) = std::fs::remove_dir_all(&session_dir) {
        state.add_log(format!("Suppression du dossier de session impossible: {}", e)).await;
    }
    state.add_log(format!("Session {} et {} image(s) supprimées", session, deleted)).await;
    StatusCode::OK.into_response()
}

// ─── ZIP download ──────────────────────────────────────────

/// Stream a ZIP (no compression: images are already compressed) built from
/// `(name in archive, file on disk)` pairs. Nothing is buffered in memory
/// besides a small pipe, so multi-GB nights can be downloaded on a Pi.
fn zip_stream(entries: Vec<(String, PathBuf)>) -> Body {
    let (tx, rx) = tokio::io::duplex(1024 * 1024);

    tokio::spawn(async move {
        use tokio_util::compat::{FuturesAsyncWriteCompatExt, TokioAsyncWriteCompatExt};
        let mut zip = async_zip::base::write::ZipFileWriter::new(tx.compat_write());
        for (entry_name, path) in entries {
            let Ok(mut file) = tokio::fs::File::open(&path).await else { continue };
            let builder = async_zip::ZipEntryBuilder::new(entry_name.into(), async_zip::Compression::Stored);
            let Ok(writer) = zip.write_entry_stream(builder).await else { break };
            let mut writer = writer.compat_write();
            if tokio::io::copy(&mut file, &mut writer).await.is_err() {
                break; // client went away
            }
            if writer.into_inner().close().await.is_err() {
                break;
            }
        }
        let _ = zip.close().await;
    });

    Body::from_stream(tokio_util::io::ReaderStream::new(rx))
}

fn zip_response(zip_name: &str, body: Body) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/zip".to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", zip_name)),
        ],
        body,
    )
        .into_response()
}

#[derive(Deserialize)]
pub struct GalleryZipRequest {
    /// Comma separated file names (HTML form field).
    pub filenames: String,
}

pub async fn download_gallery_zip(
    State(state): State<AppState>,
    axum::Form(req): axum::Form<GalleryZipRequest>,
) -> Response {
    let mount = mount_point(&state).await;
    let mut names: Vec<String> = req
        .filenames
        .split(',')
        .map(str::trim)
        .filter(|n| validate::is_safe_image_name(n))
        .map(str::to_string)
        .collect();
    names.sort();
    names.dedup();
    if names.is_empty() {
        return bad_request("Aucun fichier valide sélectionné");
    }
    let entries = names.into_iter().map(|n| (n.clone(), mount.join(n))).collect();
    let zip_name = format!("aurion_{}.zip", chrono::Local::now().format("%Y-%m-%d"));
    zip_response(&zip_name, zip_stream(entries))
}

/// Download an entire session (images + logs) as a ZIP file.
pub async fn download_gallery_session_zip(State(state): State<AppState>, Path(session): Path<String>) -> Response {
    if !validate::is_safe_name(&session) {
        return bad_request("Nom de session invalide");
    }
    let mount = mount_point(&state).await;
    let session_dir = mount.join("sessions").join(&session);
    let files = session_files(&mount, &session);

    let mut entries: Vec<(String, PathBuf)> = files.into_iter().map(|n| (n.clone(), mount.join(n))).collect();
    for log in ["event.jsonl", "session.log"] {
        let p = session_dir.join(log);
        if p.is_file() {
            entries.push((format!("sessions/{}/{}", session, log), p));
        }
    }
    if entries.is_empty() {
        state.add_log(format!("ZIP '{}' : aucun fichier trouvé", session)).await;
        return (StatusCode::NOT_FOUND, "Cette session ne contient plus aucun fichier.").into_response();
    }
    zip_response(&format!("aurion_session_{}.zip", session), zip_stream(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn timestamps() {
        assert!(parse_image_timestamp("aurora_20260305_213500_00001.jpg").is_some());
        assert!(parse_image_timestamp("aurora_2026.jpg").is_none());
        assert!(parse_image_timestamp("IMG_0001.jpg").is_none());
        assert!(parse_session_start("2026-03-05_21-30").is_some());
        assert!(parse_session_start("abc").is_none(), "short names must not panic");
        assert!(parse_session_start("").is_none());
    }

    #[test]
    fn assignment_between_sessions() {
        let sessions = s(&["2026-03-05_21-30", "2026-03-06_22-00", "notes"]);
        let images = s(&[
            "aurora_20260305_213100_00000.jpg",        // session 1
            "aurora_20260305_212700_00000.jpg",        // 3 min early: margin → session 1
            "aurora_20260306_031500_00123_AURORA.jpg", // session 1 (same night)
            "aurora_20260306_215800_00000.jpg",        // 2 min before session 2 → session 2
            "aurora_20260306_230000_00001.dng",        // session 2
            "aurora_20260301_120000_00000.jpg",        // before everything → none
            "aurora_20260308_120000_00000.jpg",        // > 16 h after session 2 → none
            "IMG_1234.jpg",                            // not an Aurion name → none
        ]);
        let g = assign_images_to_sessions(&sessions, &images);
        assert_eq!(g["2026-03-05_21-30"].len(), 3);
        assert_eq!(g["2026-03-06_22-00"], s(&["aurora_20260306_215800_00000.jpg", "aurora_20260306_230000_00001.dng"]));
        assert!(g["notes"].is_empty());
    }

    #[test]
    fn frame_numbers() {
        assert_eq!(parse_frame_number("aurora_20260305_213100_00042_AURORA.dng"), Some(42));
        assert_eq!(parse_frame_number("aurora_20260305_213100_00042.jpg"), Some(42));
        assert_eq!(parse_frame_number("aurora_2026.jpg"), None);
        assert_eq!(parse_frame_number("IMG_20260305_213100_00042.jpg"), None);
    }

    #[test]
    fn listing_ignores_unsafe_and_non_images() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["aurora_20260305_213100_00000.jpg", "readme.txt", "bad name.jpg", ".hidden.jpg"] {
            std::fs::write(dir.path().join(f), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("folder.jpg")).unwrap();
        let names: Vec<String> = list_images(dir.path()).into_iter().map(|(i, _)| i.filename).collect();
        assert_eq!(names, s(&["aurora_20260305_213100_00000.jpg"]));
    }
}
