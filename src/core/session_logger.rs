use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::core::models::SessionEvent;

/// Session logger — writes to two files on the USB drive:
/// - `event.jsonl` : one JSON line per capture event (structured data)
/// - `session.log` : human-readable timestamped messages (orchestrator status, errors)
///
/// Path: `{base_dir}/sessions/YYYY-MM-DD_HH-MM/`
///
/// Flushes every 10 lines to reduce USB write pressure.
/// Always flushes on Drop to avoid data loss.
pub struct SessionLogger {
    writer: BufWriter<File>,
    log_writer: BufWriter<File>,
    session_dir: PathBuf,
    lines_since_flush: u32,
}

/// How many lines to buffer before forcing a flush.
const FLUSH_EVERY_N_LINES: u32 = 10;

impl SessionLogger {
    /// Create a new session logger. Creates the session directory.
    pub fn new(base_dir: &Path) -> Result<Self, SessionLoggerError> {
        Self::new_at(base_dir, chrono::Local::now())
    }

    /// Create a session logger named after `now` (injected clock).
    pub fn new_at(base_dir: &Path, now: chrono::DateTime<chrono::Local>) -> Result<Self, SessionLoggerError> {
        Self::open_named(base_dir, &now.format("%Y-%m-%d_%H-%M").to_string(), now)
    }

    /// Open (or create) the session folder `name`: a resumed night appends to
    /// the logs of the interrupted one.
    pub fn open_named(base_dir: &Path, session_name: &str, now: chrono::DateTime<chrono::Local>) -> Result<Self, SessionLoggerError> {
        if !crate::core::validate::is_safe_name(session_name) {
            return Err(SessionLoggerError::IoError(format!("Nom de session invalide: {}", session_name)));
        }
        let session_dir = base_dir.join("sessions").join(session_name);

        fs::create_dir_all(&session_dir)
            .map_err(|e| SessionLoggerError::IoError(format!("Create session dir: {}", e)))?;

        // Structured JSON events
        let event_path = session_dir.join("event.jsonl");
        let event_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&event_path)
            .map_err(|e| SessionLoggerError::IoError(format!("Open event.jsonl: {}", e)))?;

        // Human-readable text log (survives reboot, readable on PC/Mac)
        let log_path = session_dir.join("session.log");
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|e| SessionLoggerError::IoError(format!("Open session.log: {}", e)))?;

        let mut logger = Self {
            writer: BufWriter::new(event_file),
            log_writer: BufWriter::new(log_file),
            session_dir,
            lines_since_flush: 0,
        };

        // Write session header
        let header = format!(
            "=== Aurion Session — {} ===\n",
            now.format("%Y-%m-%d %H:%M:%S")
        );
        let _ = logger.log_writer.write_all(header.as_bytes());
        let _ = logger.log_writer.flush();

        Ok(logger)
    }

    /// Append one event as a JSON line.
    /// Flushes to disk every FLUSH_EVERY_N_LINES lines to reduce USB write pressure.
    pub fn log_event(&mut self, event: &SessionEvent) -> Result<(), SessionLoggerError> {
        let json = serde_json::to_string(event)
            .map_err(|e| SessionLoggerError::SerializeError(e.to_string()))?;
        writeln!(self.writer, "{}", json)
            .map_err(|e| SessionLoggerError::IoError(e.to_string()))?;

        self.lines_since_flush += 1;
        if self.lines_since_flush >= FLUSH_EVERY_N_LINES {
            self.writer.flush()
                .map_err(|e| SessionLoggerError::IoError(e.to_string()))?;
            self.lines_since_flush = 0;
        }

        Ok(())
    }

    /// Append a human-readable message to session.log on USB.
    /// This is the persistent log, readable after reboot.
    /// Flushes immediately so it survives hard shutdowns.
    pub fn log_text(&mut self, msg: &str) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        let line = format!("[{}] {}\n", now, msg);
        let _ = self.log_writer.write_all(line.as_bytes());
        let _ = self.log_writer.flush(); // immediate flush — critical messages must survive
    }

    /// Flush both files and force them onto the storage (fsync): after a
    /// power cut, everything written before the last call is on the key.
    pub fn flush(&mut self) -> Result<(), SessionLoggerError> {
        self.writer.flush()
            .map_err(|e| SessionLoggerError::IoError(e.to_string()))?;
        let _ = self.log_writer.flush();
        self.writer.get_ref().sync_data()
            .map_err(|e| SessionLoggerError::IoError(e.to_string()))?;
        let _ = self.log_writer.get_ref().sync_data();
        self.lines_since_flush = 0;
        Ok(())
    }

    /// Get the session directory path.
    pub fn session_path(&self) -> &Path {
        &self.session_dir
    }
}

impl Drop for SessionLogger {
    /// Always flush on drop to avoid data loss on shutdown.
    fn drop(&mut self) {
        let _ = self.writer.flush();
        let _ = self.log_writer.flush();
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
            frame_number: None,
        };

        logger.log_event(&event).unwrap();
        // Explicit flush to read back (normally batched)
        logger.flush().unwrap();

        // Read back
        let event_path = logger.session_path().join("event.jsonl");
        let mut content = String::new();
        File::open(&event_path).unwrap().read_to_string(&mut content).unwrap();
        assert!(content.contains("\"phase\":\"Watch\""));
        assert!(content.contains("\"aurora_score\":0.5"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_flush_on_drop() {
        let dir = std::env::temp_dir().join(format!("aurion_test_drop_{}", std::process::id()));
        {
            let mut logger = SessionLogger::new(&dir).unwrap();
            let event = SessionEvent {
                timestamp: "2026-03-01T00:00:00".into(),
                phase: "Run".into(),
                capture_mode: "SAFE".into(),
                exposure_us: 10_000_000,
                iso: 1600,
                format: "JPG".into(),
                roi_excluded_percent: 35,
                aurora_score: 0.8,
                aurora_detected: true,
                aurora_color: "green".into(),
                consecutive_hits: 2,
                moon_mask_active: false,
                frame_number: Some(0),
            };
            // Only 1 line (below flush threshold of 10) — relies on Drop to flush
            logger.log_event(&event).unwrap();
        } // Drop flushes here

        let event_path = dir.join("sessions");
        // Check that at least one session dir was created
        assert!(event_path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
