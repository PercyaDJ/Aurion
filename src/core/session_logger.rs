use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::core::models::SessionEvent;

/// Session logger — writes one NDJSON line per event to event.jsonl.
///
/// Path: `{base_dir}/sessions/YYYY-MM-DD_HH-MM/event.jsonl`
pub struct SessionLogger {
    writer: BufWriter<File>,
    session_dir: PathBuf,
}

impl SessionLogger {
    /// Create a new session logger. Creates the session directory.
    pub fn new(base_dir: &Path) -> Result<Self, SessionLoggerError> {
        let now = chrono::Local::now();
        let session_name = now.format("%Y-%m-%d_%H-%M").to_string();
        let session_dir = base_dir.join("sessions").join(&session_name);

        fs::create_dir_all(&session_dir)
            .map_err(|e| SessionLoggerError::IoError(format!("Create session dir: {}", e)))?;

        let event_path = session_dir.join("event.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&event_path)
            .map_err(|e| SessionLoggerError::IoError(format!("Open event.jsonl: {}", e)))?;

        Ok(Self {
            writer: BufWriter::new(file),
            session_dir,
        })
    }

    /// Append one event as a JSON line.
    pub fn log_event(&mut self, event: &SessionEvent) -> Result<(), SessionLoggerError> {
        let json = serde_json::to_string(event)
            .map_err(|e| SessionLoggerError::SerializeError(e.to_string()))?;
        writeln!(self.writer, "{}", json)
            .map_err(|e| SessionLoggerError::IoError(e.to_string()))?;
        self.writer.flush()
            .map_err(|e| SessionLoggerError::IoError(e.to_string()))?;
        Ok(())
    }

    /// Get the session directory path.
    pub fn session_path(&self) -> &Path {
        &self.session_dir
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SessionLoggerError {
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Serialize error: {0}")]
    SerializeError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn test_session_logger_creates_dir_and_writes() {
        let dir = std::env::temp_dir().join(format!("aurion_test_{}", std::process::id()));
        let mut logger = SessionLogger::new(&dir).unwrap();

        let event = SessionEvent {
            timestamp: "2026-02-28T22:00:00".into(),
            phase: "Watch".into(),
            capture_mode: "SAFE".into(),
            exposure_us: 5_000_000,
            iso: 800,
            format: "RAW".into(),
            roi_excluded_percent: 35,
            aurora_score: 0.5,
            aurora_detected: false,
            aurora_color: "none".into(),
            consecutive_hits: 0,
            moon_mask_active: false,
        };

        logger.log_event(&event).unwrap();

        // Read back
        let event_path = logger.session_path().join("event.jsonl");
        let mut content = String::new();
        File::open(&event_path).unwrap().read_to_string(&mut content).unwrap();
        assert!(content.contains("\"phase\":\"Watch\""));
        assert!(content.contains("\"aurora_score\":0.5"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }
}
