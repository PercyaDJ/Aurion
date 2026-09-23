//! Layout of the photos on the USB key: one folder per night.
//!
//! ```text
//! sessions/2026-01-15_21-00/
//!     JPG/aurora_20260115_213000_00000.jpg
//!     RAW/aurora_20260115_213000_00000.dng
//!     thumbs/aurora_20260115_213000_00000.jpg
//!     event.jsonl  session.log  aurores.csv
//! ```
//!
//! A folder per night keeps directories small (creating a file in a huge
//! exFAT directory gets slower with every file: two weeks of RAW + JPG would
//! put more than 100 000 files in one place) and matches the photographer's
//! workflow: one folder to import per night, RAW separated for the timelapse
//! software. Before 1.7 the images were written at the root of the key; the
//! gallery still reads that layout.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;

pub const SESSIONS_DIR: &str = "sessions";
pub const JPG_DIR: &str = "JPG";
pub const RAW_DIR: &str = "RAW";
pub const THUMBS_DIR: &str = "thumbs";
pub const AURORA_INDEX: &str = "aurores.csv";

/// Where the files of one night go, relative to the key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NightLayout {
    /// `sessions/<night>`; `None` = root of the key (no session folder).
    dir: Option<String>,
}

impl NightLayout {
    pub fn night(name: &str) -> Self {
        Self { dir: Some(format!("{}/{}", SESSIONS_DIR, name)) }
    }

    /// Root of the key (fallback when the night folder cannot be created).
    pub fn root() -> Self {
        Self { dir: None }
    }

    /// Relative path of an image: RAW files in `RAW/`, the others in `JPG/`.
    pub fn image(&self, filename: &str) -> String {
        match &self.dir {
            Some(d) => format!("{}/{}/{}", d, if is_raw(filename) { RAW_DIR } else { JPG_DIR }, filename),
            None => filename.to_string(),
        }
    }

    /// Relative path of the gallery thumbnail of an image.
    pub fn thumb(&self, filename: &str) -> String {
        match &self.dir {
            Some(d) => format!("{}/{}/{}", d, THUMBS_DIR, filename),
            None => format!("{}/{}", THUMBS_DIR, filename),
        }
    }
}

pub fn is_raw(name: &str) -> bool {
    matches!(crate::core::validate::extension_lower(name).as_deref(), Some("dng") | Some("raw"))
}

/// `aurora_YYYYMMDD_HHMMSS_...` → timestamp.
pub fn parse_image_timestamp(name: &str) -> Option<NaiveDateTime> {
    let rest = name.strip_prefix("aurora_")?;
    let ts = rest.get(..15)?;
    NaiveDateTime::parse_from_str(ts, "%Y%m%d_%H%M%S").ok()
}

/// `aurora_YYYYMMDD_HHMMSS_NNNNN...` → frame number NNNNN.
pub fn parse_frame_number(name: &str) -> Option<u64> {
    name.get(23..28).filter(|_| name.starts_with("aurora_")).and_then(|n| n.parse().ok())
}

fn file_names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// Highest frame number already saved in a night folder (a resumed night
/// continues the numbering, so timelapse sequences stay in order).
pub fn max_frame_number(night_dir: &Path) -> Option<u64> {
    [JPG_DIR, RAW_DIR]
        .iter()
        .flat_map(|d| file_names(&night_dir.join(d)))
        .filter_map(|n| parse_frame_number(&n))
        .max()
}

/// Delete the `.name.part` files a power cut left behind (writes go to a
/// temporary name first). Returns how many were removed.
pub fn remove_partial_files(dir: &Path) -> usize {
    let mut removed = 0;
    for d in [dir.to_path_buf(), dir.join(JPG_DIR), dir.join(RAW_DIR), dir.join(THUMBS_DIR)] {
        for name in file_names(&d) {
            if name.starts_with('.') && name.ends_with(".part") && std::fs::remove_file(d.join(&name)).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// Write `aurores.csv` in a night folder: one line per frame with an aurora
/// (frame, time, score, colour, JPG and RAW file names), strongest first.
/// Opens in any spreadsheet; lets the photographer pick the frames to edit
/// without the web interface. Returns the number of lines.
pub fn write_aurora_index(night_dir: &Path) -> std::io::Result<usize> {
    let log = std::fs::read_to_string(night_dir.join("event.jsonl")).unwrap_or_default();
    let mut frames: BTreeMap<u64, (Option<String>, Option<String>)> = BTreeMap::new();
    for (sub, raw) in [(JPG_DIR, false), (RAW_DIR, true)] {
        for name in file_names(&night_dir.join(sub)) {
            if name.contains("_STACK") || name.starts_with('.') {
                continue;
            }
            if let Some(n) = parse_frame_number(&name) {
                let e = frames.entry(n).or_default();
                if raw { e.1 = Some(name) } else { e.0 = Some(name) }
            }
        }
    }
    let mut rows: Vec<(f64, String)> = Vec::new();
    for line in log.lines() {
        let Ok(ev) = serde_json::from_str::<crate::core::models::SessionEvent>(line) else { continue };
        let Some(n) = ev.frame_number else { continue };
        if !ev.aurora_detected {
            continue;
        }
        let (jpg, raw) = frames.get(&n).cloned().unwrap_or_default();
        let time = ev.timestamp.get(11..19).unwrap_or(&ev.timestamp).to_string();
        rows.push((
            ev.aurora_score,
            format!(
                "{};{};{:.2};{};{};{}",
                n, time, ev.aurora_score, ev.aurora_color,
                jpg.unwrap_or_default(), raw.unwrap_or_default()
            ),
        ));
    }
    rows.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = String::from("image;heure;score;couleur;jpg;raw\n");
    for (_, r) in &rows {
        out.push_str(r);
        out.push('\n');
    }
    crate::core::config::write_atomic(&night_dir.join(AURORA_INDEX), out.as_bytes(), 0o644)?;
    Ok(rows.len())
}

/// Folder of a night on the key.
pub fn night_dir(mount: &Path, night: &str) -> PathBuf {
    mount.join(SESSIONS_DIR).join(night)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_by_type() {
        let l = NightLayout::night("2026-01-15_21-00");
        assert_eq!(l.image("aurora_20260115_213000_00000.jpg"), "sessions/2026-01-15_21-00/JPG/aurora_20260115_213000_00000.jpg");
        assert_eq!(l.image("aurora_20260115_213000_00000.DNG"), "sessions/2026-01-15_21-00/RAW/aurora_20260115_213000_00000.DNG");
        assert_eq!(l.thumb("a.jpg"), "sessions/2026-01-15_21-00/thumbs/a.jpg");
        assert_eq!(NightLayout::root().image("a.jpg"), "a.jpg");
        assert_eq!(NightLayout::root().thumb("a.jpg"), "thumbs/a.jpg");
    }

    #[test]
    fn frame_and_time_parsing() {
        assert_eq!(parse_frame_number("aurora_20260305_213100_00042_AURORA.dng"), Some(42));
        assert_eq!(parse_frame_number("IMG_20260305_213100_00042.jpg"), None);
        assert!(parse_image_timestamp("aurora_20260305_213500_00001.jpg").is_some());
        assert!(parse_image_timestamp("aurora_2026.jpg").is_none());
    }

    #[test]
    fn max_frame_and_partial_cleanup() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("JPG")).unwrap();
        std::fs::create_dir_all(d.path().join("RAW")).unwrap();
        assert_eq!(max_frame_number(d.path()), None);
        std::fs::write(d.path().join("JPG/aurora_20260115_213000_00007.jpg"), b"x").unwrap();
        std::fs::write(d.path().join("RAW/aurora_20260115_213010_00012.dng"), b"x").unwrap();
        std::fs::write(d.path().join("RAW/.aurora_20260115_213020_00013.dng.part"), b"half").unwrap();
        assert_eq!(max_frame_number(d.path()), Some(12), "partial files are not frames");
        assert_eq!(remove_partial_files(d.path()), 1);
        assert!(d.path().join("RAW/aurora_20260115_213010_00012.dng").exists());
    }

    #[test]
    fn aurora_index_lists_strongest_first() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("JPG")).unwrap();
        std::fs::create_dir_all(d.path().join("RAW")).unwrap();
        for f in ["JPG/aurora_20260115_213000_00000.jpg", "JPG/aurora_20260115_213010_00001_AURORA.jpg",
                  "RAW/aurora_20260115_213010_00001_AURORA.dng", "JPG/aurora_20260115_213020_00002_AURORA.jpg"] {
            std::fs::write(d.path().join(f), b"x").unwrap();
        }
        let ev = |n: u64, score: f64, det: bool| format!(
            "{{\"timestamp\":\"2026-01-15T21:30:{:02}\",\"phase\":\"Run\",\"capture_mode\":\"SAFE\",\"exposure_us\":1,\"iso\":100,\
             \"format\":\"Jpg\",\"roi_excluded_percent\":35,\"aurora_score\":{},\"aurora_detected\":{},\"aurora_color\":\"green\",\
             \"consecutive_hits\":0,\"moon_mask_active\":false,\"frame_number\":{}}}\n", n * 10, score, det, n);
        std::fs::write(d.path().join("event.jsonl"), ev(0, 0.2, false) + &ev(1, 2.5, true) + &ev(2, 4.0, true)).unwrap();
        assert_eq!(write_aurora_index(d.path()).unwrap(), 2);
        let csv = std::fs::read_to_string(d.path().join(AURORA_INDEX)).unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines[0], "image;heure;score;couleur;jpg;raw");
        assert!(lines[1].starts_with("2;21:30:20;4.00;green;aurora_20260115_213020_00002_AURORA.jpg;"), "{}", lines[1]);
        assert!(lines[2].ends_with("aurora_20260115_213010_00001_AURORA.jpg;aurora_20260115_213010_00001_AURORA.dng"));
    }
}
