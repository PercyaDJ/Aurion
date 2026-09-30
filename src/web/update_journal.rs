//! Update journal: one text file per update, readable from the phone.
//!
//! Every step writes a line `HH:MM:SS NIVEAU message`, the level being
//! `INFO`, `OK`, `AVERTISSEMENT` (did not stop the update) or `ERREUR`.
//! The file is written on the SD card (next to the settings) and copied on
//! the USB key (`aurion-maj/`), which keeps it even if the card is flashed
//! again. The new version adds its own steps (start, confirmation) to the
//! same file after the restart.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::web::AppState;

/// Folder next to the settings, on the SD card.
pub const SD_DIR: &str = "journaux-maj";
/// Folder at the root of the USB key.
pub const USB_DIR: &str = "aurion-maj";
/// Journals kept on the SD card (the oldest are removed).
const KEEP: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Warning,
    Error,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Ok => "OK",
            Level::Warning => "AVERTISSEMENT",
            Level::Error => "ERREUR",
        }
    }
}

fn sd_dir(state: &AppState) -> PathBuf {
    state.paths.config_file.with_file_name(SD_DIR)
}

async fn usb_dir(state: &AppState) -> Option<PathBuf> {
    let mount = PathBuf::from(&state.config.read().await.storage.mount_point);
    mount.is_dir().then(|| mount.join(USB_DIR))
}

/// A journal name produced by [`new_name`]: nothing else is ever read or
/// served (no path, no traversal).
pub fn valid_name(name: &str) -> bool {
    name.len() <= 80
        && name.ends_with(".log")
        && !name.starts_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.contains("..")
}

/// `2026-10-01_214502_stable.log`
pub fn new_name(channel: &str) -> String {
    let channel: String = channel.chars().filter(|c| c.is_ascii_alphanumeric()).take(10).collect();
    let channel = if channel.is_empty() { "maj".to_string() } else { channel };
    format!("{}_{}.log", chrono::Local::now().format("%Y-%m-%d_%H%M%S"), channel)
}

/// Append a line to the journal (SD card and USB key, best effort: a
/// journal that cannot be written never stops an update).
pub async fn write(state: &AppState, name: &str, level: Level, message: &str) {
    if !valid_name(name) {
        return;
    }
    let line = format!("{} {} {}\n", chrono::Local::now().format("%H:%M:%S"), level.label(), message.replace('\n', " "));
    let mut dirs = vec![sd_dir(state)];
    if let Some(usb) = usb_dir(state).await {
        dirs.push(usb);
    }
    for dir in dirs {
        let _ = append(&dir, name, &line);
    }
}

fn append(dir: &Path, name: &str, line: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(name))?;
    f.write_all(line.as_bytes())?;
    f.sync_data()
}

/// Content of a journal (SD card first, else the USB key).
pub async fn read(state: &AppState, name: &str) -> Option<String> {
    if !valid_name(name) {
        return None;
    }
    if let Ok(text) = std::fs::read_to_string(sd_dir(state).join(name)) {
        return Some(text);
    }
    let usb = usb_dir(state).await?;
    std::fs::read_to_string(usb.join(name)).ok()
}

/// (warnings, errors) in a journal.
pub fn counts(text: &str) -> (u32, u32) {
    let mut warnings = 0;
    let mut errors = 0;
    for line in text.lines() {
        match line.split_whitespace().nth(1) {
            Some("AVERTISSEMENT") => warnings += 1,
            Some("ERREUR") => errors += 1,
            _ => {}
        }
    }
    (warnings, errors)
}

/// Journals on the SD card, newest first, with their counts.
pub fn list(state: &AppState) -> Vec<serde_json::Value> {
    let mut names: Vec<String> = std::fs::read_dir(sd_dir(state))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| valid_name(n))
        .collect();
    names.sort_unstable_by(|a, b| b.cmp(a));
    names
        .into_iter()
        .map(|name| {
            let text = std::fs::read_to_string(sd_dir(state).join(&name)).unwrap_or_default();
            let (warnings, errors) = counts(&text);
            serde_json::json!({ "name": name, "warnings": warnings, "errors": errors })
        })
        .collect()
}

/// Remove the oldest journals from the SD card (the USB key keeps them all).
pub fn prune(state: &AppState) {
    let dir = sd_dir(state);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| valid_name(n))
        .collect();
    if names.len() <= KEEP {
        return;
    }
    names.sort_unstable();
    for old in &names[..names.len() - KEEP] {
        let _ = std::fs::remove_file(dir.join(old));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_journal_names_are_accepted() {
        assert!(valid_name("2026-10-01_214502_stable.log"));
        assert!(valid_name(&new_name("dev")));
        assert!(new_name("../x").ends_with("_x.log"));
        for bad in ["../aurion.json", "a/b.log", ".log", "x.txt", "..log", "a b.log", "%2e%2e.log", ""] {
            assert!(!valid_name(bad), "{bad}");
        }
    }

    #[test]
    fn warnings_and_errors_are_counted() {
        let text = "21:45:02 INFO début\n21:46:10 AVERTISSEMENT coupure\n21:46:40 AVERTISSEMENT coupure\n21:47:00 ERREUR refus\n21:47:01 OK fin\n";
        assert_eq!(counts(text), (2, 1));
        assert_eq!(counts(""), (0, 0));
    }
}
