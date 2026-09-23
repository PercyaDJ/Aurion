use serde::{Deserialize, Serialize};
use chrono::NaiveTime;
use std::fmt;

// ─── Phases ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Boot,
    Arm,
    Disconnect,
    Calibration,
    Watch,
    Run,
    Shutdown,
    SafeMode,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Phase::Boot => write!(f, "BOOT"),
            Phase::Arm => write!(f, "ARM"),
            Phase::Disconnect => write!(f, "DISCONNECT"),
            Phase::Calibration => write!(f, "CALIBRATION"),
            Phase::Watch => write!(f, "WATCH"),
            Phase::Run => write!(f, "RUN"),
            Phase::Shutdown => write!(f, "SHUTDOWN"),
            Phase::SafeMode => write!(f, "SAFE MODE"),
        }
    }
}

// ─── Image Format ──────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputFormat {
    RawDng,
    Jpg,
    RawAndJpg,
    /// JPEG all night (timelapse), RAW only while an aurora is detected and
    /// for [`crate::core::orchestrator::AURORA_RAW_HOLD_SECS`] after it:
    /// several nights of capture fit on one key.
    JpgAuroraRaw,
}

impl OutputFormat {
    /// The camera must produce a DNG for this format.
    pub fn captures_raw(self) -> bool {
        !matches!(self, OutputFormat::Jpg)
    }
    pub fn saves_jpg(self) -> bool {
        !matches!(self, OutputFormat::RawDng)
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::RawDng => write!(f, "RAW (DNG)"),
            OutputFormat::Jpg => write!(f, "JPG"),
            OutputFormat::RawAndJpg => write!(f, "RAW + JPG"),
            OutputFormat::JpgAuroraRaw => write!(f, "JPG + RAW pendant les aurores"),
        }
    }
}

// ─── Capture Frame ─────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CaptureFrame {
    /// Decoded RGB pixels for analysis (exposure, detection, gallery
    /// thumbnail). On the Pi this is the small EXIF thumbnail of the
    /// capture, NOT the full-resolution image (which stays in `raw_bytes`).
    pub data: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub format: CaptureFormat,
    pub metadata: FrameMetadata,
    /// Original compressed bytes from rpicam-still (JPEG or DNG).
    /// If non-empty, used as-is for the final saved file (no re-encode).
    pub raw_bytes: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureFormat {
    Jpg,
    RawDng,
}

#[derive(Debug, Clone)]
pub struct FrameMetadata {
    pub iso: u32,
    pub shutter_us: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

// ─── Exposure ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExposureSettings {
    pub iso: u32,
    pub shutter_us: u64,
}

impl ExposureSettings {
    pub fn new(iso: u32, shutter_us: u64) -> Self {
        Self { iso, shutter_us }
    }

    /// Exposure value in EV (relative, log2-based)
    pub fn ev(&self) -> f64 {
        let iso_factor = self.iso as f64 / 100.0;
        let shutter_secs = self.shutter_us as f64 / 1_000_000.0;
        (iso_factor * shutter_secs).log2()
    }
}

// ─── Storage ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageInfo {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub mount_point: String,
}

impl StorageInfo {
    pub fn used_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.free_bytes)
    }

    pub fn free_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        (self.free_bytes as f64 / self.total_bytes as f64) * 100.0
    }

    pub fn status(&self, warning_pct: f64, critical_pct: f64) -> StorageStatus {
        let free = self.free_percent();
        if free <= critical_pct {
            StorageStatus::Critical
        } else if free <= warning_pct {
            StorageStatus::Warning
        } else {
            StorageStatus::Ok
        }
    }

    /// Human-readable summary, e.g. "23 % libre (29 Go / 128 Go)"
    pub fn display_summary(&self) -> String {
        let free_gb = self.free_bytes as f64 / 1_073_741_824.0;
        let total_gb = self.total_bytes as f64 / 1_073_741_824.0;
        let pct = self.free_percent();
        format!("{:.0} % libre ({:.0} Go / {:.0} Go)", pct, free_gb, total_gb)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageStatus {
    Ok,
    Warning,
    Critical,
}

// ─── Detection ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuroraColor {
    None,
    Green,
    Red,
    Violet,
    Unknown,
}

impl fmt::Display for AuroraColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuroraColor::None => write!(f, "none"),
            AuroraColor::Green => write!(f, "green"),
            AuroraColor::Red => write!(f, "red"),
            AuroraColor::Violet => write!(f, "violet"),
            AuroraColor::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub detected: bool,
    pub aurora_score: f64,
    pub aurora_color: AuroraColor,
    pub green_score: f64,
    pub red_score: f64,
    pub violet_score: f64,
    pub area_percent: f64,
    pub luminosity: f64,
    pub confidence: f64,
    pub moon_masked: bool,
}

impl DetectionResult {
    pub fn negative() -> Self {
        Self {
            detected: false,
            aurora_score: 0.0,
            aurora_color: AuroraColor::None,
            green_score: 0.0,
            red_score: 0.0,
            violet_score: 0.0,
            area_percent: 0.0,
            luminosity: 0.0,
            confidence: 0.0,
            moon_masked: false,
        }
    }
}

// ─── Session Event (NDJSON logging) ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    pub timestamp: String,
    pub phase: String,
    pub capture_mode: String,
    pub exposure_us: u64,
    pub iso: u32,
    pub format: String,
    pub roi_excluded_percent: u32,
    pub aurora_score: f64,
    pub aurora_detected: bool,
    pub aurora_color: String,
    pub consecutive_hits: u32,
    pub moon_mask_active: bool,
    /// Number of the saved image (`aurora_..._NNNNN`) when this frame was
    /// written to the key. Lets the gallery rank images by aurora strength.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_number: Option<u64>,
    /// Wall time of the capture (camera start, exposure, files), to measure
    /// the real overhead of `rpicam-still` beyond the exposure itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capture_ms: Option<u64>,
}

// ─── Time Range ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: NaiveTime,
    pub end: NaiveTime,
}

impl TimeRange {
    /// Check if a given time is within this range.
    /// Handles overnight ranges where start > end (e.g., 21:00 → 06:00).
    pub fn contains(&self, time: NaiveTime) -> bool {
        if self.start <= self.end {
            time >= self.start && time <= self.end
        } else {
            // Overnight: 21:00 → 06:00
            time >= self.start || time <= self.end
        }
    }
}

// ─── Warnings ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    HighIso,
    StarTrails,
    StorageWarning,
    StorageCritical,
    CameraAbsent,
}

/// Check 300/500 rule for star trails.
/// Returns true if shutter is too long (stars will trail).
pub fn check_star_trail_rule(focal_mm: f64, shutter_us: u64, crop_factor: f64) -> bool {
    let shutter_secs = shutter_us as f64 / 1_000_000.0;
    let rule_500 = 500.0 / (focal_mm * crop_factor);
    shutter_secs > rule_500
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_info_display() {
        let info = StorageInfo {
            total_bytes: 128_000_000_000,
            free_bytes: 29_000_000_000,
            mount_point: "/mnt/capture".into(),
        };
        assert!(info.display_summary().contains("Go"));
        assert!(info.free_percent() > 20.0);
        assert!(info.free_percent() < 25.0);
    }

    #[test]
    fn test_storage_status() {
        let info = StorageInfo {
            total_bytes: 100,
            free_bytes: 10,
            mount_point: "/mnt/capture".into(),
        };
        assert_eq!(info.status(15.0, 5.0), StorageStatus::Warning);

        let critical = StorageInfo {
            total_bytes: 100,
            free_bytes: 3,
            mount_point: "/mnt/capture".into(),
        };
        assert_eq!(critical.status(15.0, 5.0), StorageStatus::Critical);
    }

    #[test]
    fn test_time_range_overnight() {
        let range = TimeRange {
            start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
            end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
        };
        assert!(range.contains(NaiveTime::from_hms_opt(23, 0, 0).unwrap()));
        assert!(range.contains(NaiveTime::from_hms_opt(3, 0, 0).unwrap()));
        assert!(!range.contains(NaiveTime::from_hms_opt(12, 0, 0).unwrap()));
    }

    #[test]
    fn test_star_trail_rule() {
        // 500 / (2.7 * 5.6) ≈ 33s max
        // 2.7mm lens, crop factor 5.6, 40s → should trail
        assert!(check_star_trail_rule(2.7, 40_000_000, 5.6));
        // 2.7mm lens, crop factor 5.6, 10s → should be fine
        assert!(!check_star_trail_rule(2.7, 10_000_000, 5.6));
    }

    #[test]
    fn test_exposure_ev() {
        let exp = ExposureSettings::new(100, 1_000_000); // ISO 100, 1s
        let ev = exp.ev();
        // log2(1.0 * 1.0) = 0.0
        assert!((ev - 0.0).abs() < 0.001);
    }
}
