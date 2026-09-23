//! Gallery / recovery mode: list, preview, download (ZIP) and delete the
//! captured images stored on the USB drive.
//!
//! Layout on the drive (see [`crate::core::layout`]): one folder per night
//! `sessions/<night>/{JPG,RAW,thumbs}`, plus images at the root of the key
//! written by versions before 1.7 (still listed, grouped by time).
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

use crate::core::{layout, validate};
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
    /// Night folder holding the image (`None`: root of the key, before 1.7).
    pub session: Option<String>,
}

#[derive(Serialize)]
pub struct GalleryResponse {
    pub images: Vec<GalleryImage>,
    pub total_count: usize,
    pub total_size_mb: f64,
    /// Only the most recent images are listed (see `limit`).
    pub truncated: bool,
}

fn size_display(size: u64) -> String {
    if size > 1_048_576 {
        format!("{:.1} Mo", size as f64 / 1_048_576.0)
    } else {
        format!("{:.0} Ko", size as f64 / 1024.0)
    }
}

/// One image file on the key.
#[derive(Clone)]
pub struct ImageFile {
    pub info: GalleryImage,
    pub modified: std::time::SystemTime,
    pub path: PathBuf,
}

/// Images directly inside `dir`. Files with unsafe names are ignored (they
/// could not be served safely).
fn scan_images(dir: &FsPath, session: Option<&str>) -> Vec<ImageFile> {
    let mut images = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
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
        images.push(ImageFile {
            info: GalleryImage {
                aurora: name.contains("_AURORA"),
                size_display: size_display(meta.len()),
                size_bytes: meta.len(),
                modified: modified.format("%d/%m/%Y %H:%M").to_string(),
                session: session.map(str::to_string),
                filename: name,
            },
            modified: modified_time,
            path: entry.path(),
        });
    }
    images
}

/// Images stored in a night folder (`JPG/` and `RAW/`).
fn night_folder_images(mount: &FsPath, session: &str) -> Vec<ImageFile> {
    let dir = layout::night_dir(mount, session);
    let mut v = scan_images(&dir.join(layout::JPG_DIR), Some(session));
    v.extend(scan_images(&dir.join(layout::RAW_DIR), Some(session)));
    v
}

/// Every image on the key (night folders, then the root for captures made
/// before 1.7), sorted by name, i.e. by capture time.
pub fn all_images(mount: &FsPath) -> Vec<ImageFile> {
    let mut images = scan_images(mount, None);
    for s in session_names(mount) {
        images.extend(night_folder_images(mount, &s));
    }
    images.sort_by(|a, b| a.info.filename.cmp(&b.info.filename));
    images
}

/// Compatibility view used by the listing tests and tools.
pub fn list_images(mount: &FsPath) -> Vec<(GalleryImage, std::time::SystemTime)> {
    all_images(mount).into_iter().map(|f| (f.info, f.modified)).collect()
}

/// Sizes of up to `max` images of the most recent night (capacity estimate,
/// cheap enough to be computed on every home screen refresh).
pub fn recent_image_sizes(mount: &FsPath, max: usize) -> Vec<(String, u64)> {
    let sample = |files: Vec<ImageFile>| -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = files.into_iter().map(|f| (f.info.filename, f.info.size_bytes)).collect();
        v.sort();
        v.into_iter().rev().take(max).collect()
    };
    if let Some(last) = session_names(mount).into_iter().rev().find(|s| layout::night_dir(mount, s).join(layout::JPG_DIR).is_dir()
        || layout::night_dir(mount, s).join(layout::RAW_DIR).is_dir())
    {
        return sample(night_folder_images(mount, &last));
    }
    sample(scan_images(mount, None))
}

/// Find an image by name: root of the key, then night folders.
pub fn resolve_image(mount: &FsPath, name: &str) -> Option<ImageFile> {
    if !validate::is_safe_image_name(name) {
        return None;
    }
    let sub = if layout::is_raw(name) { layout::RAW_DIR } else { layout::JPG_DIR };
    let candidates = std::iter::once((mount.join(name), None))
        .chain(session_names(mount).into_iter().rev().map(|s| (layout::night_dir(mount, &s).join(sub).join(name), Some(s))));
    for (path, session) in candidates {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.is_file() {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                let m: chrono::DateTime<chrono::Local> = modified.into();
                return Some(ImageFile {
                    info: GalleryImage {
                        aurora: name.contains("_AURORA"),
                        size_display: size_display(meta.len()),
                        size_bytes: meta.len(),
                        modified: m.format("%d/%m/%Y %H:%M").to_string(),
                        session,
                        filename: name.to_string(),
                    },
                    modified,
                    path,
                });
            }
        }
    }
    None
}

/// Capture-time thumbnail of an image.
fn thumb_path(mount: &FsPath, image: &ImageFile) -> PathBuf {
    match &image.info.session {
        Some(s) => layout::night_dir(mount, s).join(layout::THUMBS_DIR).join(&image.info.filename),
        None => mount.join(layout::THUMBS_DIR).join(&image.info.filename),
    }
}

#[derive(Deserialize, Default)]
pub struct GalleryQuery {
    /// Only the images of this night.
    #[serde(default)]
    pub session: Option<String>,
    /// Most recent images returned (default 1000, at most 5000).
    #[serde(default)]
    pub limit: Option<usize>,
}

pub async fn get_gallery(State(state): State<AppState>, Query(q): Query<GalleryQuery>) -> Response {
    if let Some(ref s) = q.session {
        if !validate::is_safe_name(s) {
            return bad_request("Nom de session invalide");
        }
    }
    let mount = mount_point(&state).await;
    let limit = q.limit.unwrap_or(1000).clamp(1, 5000);
    let session = q.session.clone();
    let images: Vec<GalleryImage> = tokio::task::spawn_blocking(move || match session {
        Some(s) => session_images(&mount, &s).into_iter().map(|f| f.info).collect(),
        None => all_images(&mount).into_iter().map(|f| f.info).collect(),
    })
    .await
    .unwrap_or_default();
    let total_size: u64 = images.iter().map(|i| i.size_bytes).sum();
    let total_count = images.len();
    let skip = total_count.saturating_sub(limit);
    Json(GalleryResponse {
        total_count,
        total_size_mb: total_size as f64 / 1_048_576.0,
        truncated: skip > 0,
        images: images.into_iter().skip(skip).collect(),
    })
    .into_response()
}

#[derive(Serialize)]
pub struct GalleryStats {
    pub total_images: usize,
    pub total_size_mb: f64,
    pub last_session_date: Option<String>,
}

pub async fn get_gallery_stats(State(state): State<AppState>) -> Json<GalleryStats> {
    let mount = mount_point(&state).await;
    let images = tokio::task::spawn_blocking(move || all_images(&mount)).await.unwrap_or_default();
    let total_size: u64 = images.iter().map(|i| i.info.size_bytes).sum();
    let last = images.iter().map(|i| i.modified).max().map(|t| {
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

pub use crate::core::layout::{parse_frame_number, parse_image_timestamp};

/// `YYYY-MM-DD_HH-MM` → session start.
pub fn parse_session_start(name: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(&format!("{}-00", name.get(..16)?), "%Y-%m-%d_%H-%M-%S").ok()
}

/// Group image names by session (images at the root of the key, before
/// 1.7). An image belongs to the latest session that started (minus a small
/// margin) before it, as long as the next session has not started and the
/// session is not older than 16 h.
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
    let Ok(entries) = std::fs::read_dir(mount.join(layout::SESSIONS_DIR)) else {
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

/// Images of every session: its own folder plus the root images of the
/// same night (older layout).
fn images_by_session(mount: &FsPath) -> BTreeMap<String, Vec<ImageFile>> {
    let names = session_names(mount);
    let root: BTreeMap<String, ImageFile> =
        scan_images(mount, None).into_iter().map(|f| (f.info.filename.clone(), f)).collect();
    let root_names: Vec<String> = root.keys().cloned().collect();
    let legacy = assign_images_to_sessions(&names, &root_names);
    names
        .iter()
        .map(|s| {
            let mut files = night_folder_images(mount, s);
            if let Some(old) = legacy.get(s) {
                files.extend(old.iter().filter_map(|n| root.get(n).cloned()));
            }
            files.sort_by(|a, b| a.info.filename.cmp(&b.info.filename));
            (s.clone(), files)
        })
        .collect()
}

/// Images belonging to one session.
pub fn session_images(mount: &FsPath, session: &str) -> Vec<ImageFile> {
    let mut files = night_folder_images(mount, session);
    let names = session_names(mount);
    let root: Vec<ImageFile> = scan_images(mount, None);
    let root_names: Vec<String> = root.iter().map(|f| f.info.filename.clone()).collect();
    if let Some(old) = assign_images_to_sessions(&names, &root_names).remove(session) {
        files.extend(root.into_iter().filter(|f| old.contains(&f.info.filename)));
    }
    files.sort_by(|a, b| a.info.filename.cmp(&b.info.filename));
    files
}

/// Names of the images belonging to one session.
pub fn session_files(mount: &FsPath, session: &str) -> Vec<String> {
    session_images(mount, session).into_iter().map(|f| f.info.filename).collect()
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub date: String,
    pub image_count: usize,
    pub aurora_count: usize,
    pub raw_count: usize,
    pub total_size_mb: f64,
    pub duration_minutes: u64,
    pub has_log: bool,
}

fn compute_sessions(mount: &FsPath) -> Vec<SessionInfo> {
    let mut sessions: Vec<SessionInfo> = images_by_session(mount)
        .into_iter()
        .map(|(name, files)| {
            let dir = layout::night_dir(mount, &name);
            let has_log = dir.join("event.jsonl").exists() || dir.join("session.log").exists();
            let stamps: Vec<NaiveDateTime> = files.iter().filter_map(|f| parse_image_timestamp(&f.info.filename)).collect();
            let duration_minutes = match (stamps.iter().min(), stamps.iter().max()) {
                (Some(a), Some(b)) => (*b - *a).num_minutes().max(0) as u64,
                _ => 0,
            };
            // Count one aurora per capture even with RAW+JPG pairs.
            let mut aurora_bases: Vec<&str> = files
                .iter()
                .map(|f| f.info.filename.as_str())
                .filter(|f| f.contains("_AURORA"))
                .map(|f| f.rsplit_once('.').map(|(b, _)| b).unwrap_or(f))
                .collect();
            aurora_bases.sort();
            aurora_bases.dedup();
            SessionInfo {
                date: name.get(..10).unwrap_or(&name).replace('-', "/"),
                image_count: files.len(),
                aurora_count: aurora_bases.len(),
                raw_count: files.iter().filter(|f| layout::is_raw(&f.info.filename)).count(),
                total_size_mb: files.iter().map(|f| f.info.size_bytes).sum::<u64>() as f64 / 1_048_576.0,
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

#[derive(Serialize)]
pub struct LastNight {
    pub session: SessionInfo,
    pub best_score: Option<f64>,
    pub raw_count: usize,
}

/// Highest aurora score recorded in a session's event log.
fn session_best_score(mount: &FsPath, session: &str) -> Option<f64> {
    let text = std::fs::read_to_string(layout::night_dir(mount, session).join("event.jsonl")).ok()?;
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v.get("aurora_score")?.as_f64())
        .filter(|s| s.is_finite())
        .reduce(f64::max)
}

/// Summary of the most recent night for the home screen.
pub async fn get_last_night(State(state): State<AppState>) -> Json<Option<LastNight>> {
    let mount = mount_point(&state).await;
    Json(
        tokio::task::spawn_blocking(move || {
            let session = compute_sessions(&mount).into_iter().find(|s| s.image_count > 0)?;
            let raw_count = session.raw_count;
            let best_score = session_best_score(&mount, &session.name);
            Some(LastNight { session, best_score, raw_count })
        })
        .await
        .ok()
        .flatten(),
    )
}

/// List capture sessions (most recent first). Read-only: listing never
/// deletes anything (a session of a night without aurora only has logs).
pub async fn get_gallery_sessions(State(state): State<AppState>) -> Json<Vec<SessionInfo>> {
    let mount = mount_point(&state).await;
    Json(tokio::task::spawn_blocking(move || compute_sessions(&mount)).await.unwrap_or_default())
}

// ─── Strongest auroras ─────────────────────────────────────

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
    let mut out = Vec::new();

    for (session, files) in images_by_session(mount) {
        let Ok(log) = std::fs::read_to_string(layout::night_dir(mount, &session).join("event.jsonl")) else {
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
        for f in files.iter().map(|f| &f.info.filename).filter(|f| !f.contains("_STACK")) {
            let Some(n) = parse_frame_number(f) else { continue };
            let entry = frames.entry(n).or_default();
            if layout::is_raw(f) {
                entry.1 = Some(f.clone());
            } else {
                entry.0 = Some(f.clone());
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
    let mount = mount_point(&state).await;
    let name = filename.clone();
    let Some(image) = tokio::task::spawn_blocking(move || resolve_image(&mount, &name)).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let file = match tokio::fs::File::open(&image.path).await {
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
    let (m, name) = (mount.clone(), filename.clone());
    let Some(image) = tokio::task::spawn_blocking(move || resolve_image(&m, &name)).await.ok().flatten() else {
        return StatusCode::NOT_FOUND.into_response();
    };

    if let Ok(data) = tokio::fs::read(thumb_path(&mount, &image)).await {
        return jpeg(data);
    }
    let cache_path = state.paths.thumb_cache_dir.join(&filename);
    if let Ok(data) = tokio::fs::read(&cache_path).await {
        return jpeg(data);
    }

    let source = image.path.clone();
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
    let Some(image) = resolve_image(mount, name) else {
        return Ok(false);
    };
    std::fs::remove_file(&image.path)?;
    let _ = std::fs::remove_file(thumb_path(mount, &image));
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
    let session_dir = layout::night_dir(&mount, &session);
    if !session_dir.is_dir() {
        return (StatusCode::NOT_FOUND, "Session introuvable").into_response();
    }

    let mut deleted = 0;
    for image in session_images(&mount, &session) {
        if std::fs::remove_file(&image.path).is_ok() {
            let _ = std::fs::remove_file(thumb_path(&mount, &image));
            let _ = std::fs::remove_file(state.paths.thumb_cache_dir.join(&image.info.filename));
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
    let entries = tokio::task::spawn_blocking(move || {
        names.into_iter().filter_map(|n| resolve_image(&mount, &n).map(|f| (n, f.path))).collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    let zip_name = format!("aurion_{}.zip", chrono::Local::now().format("%Y-%m-%d"));
    zip_response(&zip_name, zip_stream(entries))
}

#[derive(Deserialize, Default)]
pub struct SessionZipQuery {
    /// "raw" = DNG only, "jpg" = JPEG only; default: everything.
    #[serde(default)]
    pub only: Option<String>,
}

/// Download an entire session (images + logs) as a ZIP file.
pub async fn download_gallery_session_zip(
    State(state): State<AppState>,
    Path(session): Path<String>,
    Query(q): Query<SessionZipQuery>,
) -> Response {
    if !validate::is_safe_name(&session) {
        return bad_request("Nom de session invalide");
    }
    let only = q.only.as_deref();
    if !matches!(only, None | Some("raw") | Some("jpg")) {
        return bad_request("Filtre inconnu : utilisez only=raw ou only=jpg");
    }
    let mount = mount_point(&state).await;
    let session_dir = layout::night_dir(&mount, &session);
    let (m, s_name) = (mount.clone(), session.clone());
    let files: Vec<ImageFile> = tokio::task::spawn_blocking(move || session_images(&m, &s_name))
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|f| match (only, validate::extension_lower(&f.info.filename).as_deref()) {
            (Some("raw"), ext) => matches!(ext, Some("dng") | Some("raw")),
            (Some("jpg"), ext) => matches!(ext, Some("jpg") | Some("jpeg")),
            _ => true,
        })
        .collect();

    // Same tree as on the key: <night>/JPG, <night>/RAW, logs and index.
    let mut entries: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|f| {
            let sub = if layout::is_raw(&f.info.filename) { layout::RAW_DIR } else { layout::JPG_DIR };
            (format!("{}/{}/{}", session, sub, f.info.filename), f.path)
        })
        .collect();
    for log in ["event.jsonl", "session.log", layout::AURORA_INDEX] {
        let p = session_dir.join(log);
        if p.is_file() {
            entries.push((format!("{}/{}", session, log), p));
        }
    }
    if entries.is_empty() {
        state.add_log(format!("ZIP '{}' : aucun fichier trouvé", session)).await;
        return (StatusCode::NOT_FOUND, "Cette session ne contient plus aucun fichier.").into_response();
    }
    let suffix = match only { Some("raw") => "_RAW", Some("jpg") => "_JPG", _ => "" };
    zip_response(&format!("aurion_{}{}.zip", session, suffix), zip_stream(entries))
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
