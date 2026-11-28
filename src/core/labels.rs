//! Labels given to the images in the gallery, one file per night:
//! `sessions/<night>/labels.csv` (material for a future data set and
//! classifier, step A of the detection plan).
//!
//! ```text
//! image;etiquette;date_utc
//! aurora_20260305_223100_00001_AURORA.jpg;aurore;2026-03-06T08:12:40Z
//! ```
//!
//! Semicolon separated, UTF-8, one line per image (the last label given
//! wins, an empty label removes the line), sorted by image name. Labels:
//! [`LABELS`].

use std::collections::BTreeMap;
use std::path::Path;

use crate::core::layout::LABELS_FILE;

/// Accepted labels: aurora, no aurora, clouds, Moon, stray light.
pub const LABELS: [&str; 5] = ["aurore", "pas_aurore", "nuages", "lune", "lumiere_parasite"];

pub const HEADER: &str = "image;etiquette;date_utc";

/// One label: (label, date UTC).
pub type Label = (String, String);

/// Labels of a night folder, by image name (empty without a file).
pub fn read(night_dir: &Path) -> BTreeMap<String, Label> {
    let text = std::fs::read_to_string(night_dir.join(LABELS_FILE)).unwrap_or_default();
    text.lines()
        .skip_while(|l| *l == HEADER)
        .filter_map(|l| {
            let mut f = l.split(';');
            let (image, label, date) = (f.next()?, f.next()?, f.next().unwrap_or(""));
            (crate::core::validate::is_safe_image_name(image) && LABELS.contains(&label))
                .then(|| (image.to_string(), (label.to_string(), date.to_string())))
        })
        .collect()
}

/// Serialises the read-modify-write of the label files (two phones, or a
/// fast double tap).
static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Give `label` to `image` (empty: remove its label), with the date `now`
/// (RFC 3339, UTC). The file is rewritten atomically.
pub fn set(night_dir: &Path, image: &str, label: &str, now: &str) -> Result<(), String> {
    if !crate::core::validate::is_safe_image_name(image) {
        return Err("Nom d'image invalide".into());
    }
    if !label.is_empty() && !LABELS.contains(&label) {
        return Err(format!("Étiquette inconnue (valeurs : {})", LABELS.join(", ")));
    }
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut labels = read(night_dir);
    if label.is_empty() {
        labels.remove(image);
    } else {
        labels.insert(image.to_string(), (label.to_string(), now.to_string()));
    }
    let mut out = String::from(HEADER);
    out.push('\n');
    for (image, (label, date)) in &labels {
        out.push_str(&format!("{};{};{}\n", image, label, date));
    }
    crate::core::config::write_atomic(&night_dir.join(LABELS_FILE), out.as_bytes(), 0o644).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_are_written_replaced_and_removed() {
        let d = tempfile::tempdir().unwrap();
        assert!(read(d.path()).is_empty());
        set(d.path(), "aurora_20260305_223100_00001_AURORA.jpg", "aurore", "2026-03-06T08:12:40Z").unwrap();
        set(d.path(), "aurora_20260305_213100_00000.jpg", "nuages", "2026-03-06T08:12:45Z").unwrap();
        set(d.path(), "aurora_20260305_213100_00000.jpg", "lune", "2026-03-06T08:13:00Z").unwrap();
        let text = std::fs::read_to_string(d.path().join(LABELS_FILE)).unwrap();
        assert_eq!(
            text,
            "image;etiquette;date_utc\n\
             aurora_20260305_213100_00000.jpg;lune;2026-03-06T08:13:00Z\n\
             aurora_20260305_223100_00001_AURORA.jpg;aurore;2026-03-06T08:12:40Z\n"
        );
        set(d.path(), "aurora_20260305_213100_00000.jpg", "", "x").unwrap();
        let labels = read(d.path());
        assert_eq!(labels.len(), 1);
        assert_eq!(labels["aurora_20260305_223100_00001_AURORA.jpg"].0, "aurore");
    }

    #[test]
    fn unknown_labels_and_unsafe_names_are_refused() {
        let d = tempfile::tempdir().unwrap();
        assert!(set(d.path(), "aurora_1.jpg", "soleil", "x").is_err());
        assert!(set(d.path(), "../aurora_1.jpg", "aurore", "x").is_err());
        // A hand-edited file: invalid lines are ignored
        std::fs::write(d.path().join(LABELS_FILE), "image;etiquette;date_utc\na/b.jpg;aurore;x\nok.jpg;bof;x\nok.jpg;lune\n").unwrap();
        let labels = read(d.path());
        assert_eq!(labels.len(), 1);
        assert_eq!(labels["ok.jpg"], ("lune".to_string(), String::new()));
    }
}
