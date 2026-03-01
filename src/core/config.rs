use serde::{Deserialize, Serialize};
use chrono::NaiveTime;
use std::path::{Path, PathBuf};
use crate::core::models::OutputFormat;

// ─── Main Config ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub exposure: ExposureConfig,
    pub detection: DetectionConfig,
    pub capture: CaptureConfig,
    pub time_range: TimeRangeConfig,
    pub storage: StorageConfig,
    pub network: NetworkConfig,
    pub web: WebConfig,
    pub preset_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExposureConfig {
    pub iso_min: u32,
    pub iso_max: u32,
    pub shutter_min_us: u64,
    pub shutter_max_us: u64,
    pub ev_step_max: f64,
    /// EMA alpha for Detect/Calibration phase (default 0.08)
    #[serde(default = "default_detect_alpha")]
    pub detect_alpha: f64,
    /// Max +% change per frame in Detect (default 0.15)
    #[serde(default = "default_detect_rate_up")]
    pub detect_rate_up: f64,
    /// Max -% change per frame in Detect (default 0.20)
    #[serde(default = "default_detect_rate_down")]
    pub detect_rate_down: f64,
    /// EMA alpha for Capture/Run phase (default 0.03)
    #[serde(default = "default_capture_alpha")]
    pub capture_alpha: f64,
    /// Max +% change per frame in Capture (default 0.03)
    #[serde(default = "default_capture_rate_up")]
    pub capture_rate_up: f64,
    /// Max -% change per frame in Capture (default 0.05)
    #[serde(default = "default_capture_rate_down")]
    pub capture_rate_down: f64,
    /// Histogram percentile for metering (default 0.80 = P80)
    #[serde(default = "default_target_percentile")]
    pub target_percentile: f64,
    /// Target brightness value at that percentile (default 60.0)
    #[serde(default = "default_target_brightness")]
    pub target_brightness: f64,
    /// Reject pixels above this value (0-255) from metering (default 250.0)
    #[serde(default = "default_saturation_reject")]
    pub saturation_reject: f64,
}

fn default_detect_alpha() -> f64 { 0.08 }
fn default_detect_rate_up() -> f64 { 0.15 }
fn default_detect_rate_down() -> f64 { 0.20 }
fn default_capture_alpha() -> f64 { 0.03 }
fn default_capture_rate_up() -> f64 { 0.03 }
fn default_capture_rate_down() -> f64 { 0.05 }
fn default_target_percentile() -> f64 { 0.80 }
fn default_target_brightness() -> f64 { 60.0 }
fn default_saturation_reject() -> f64 { 250.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionConfig {
    pub roi_top_percent: u32,
    pub green_threshold: f64,
    pub luminosity_threshold: f64,
    pub variation_threshold: f64,
    pub consecutive_required: u32,
    /// FILTER=true (detect triggers capture), SAFE=false (capture all, detect tags only)
    #[serde(default)]
    pub detection_capture_enabled: bool,
    /// Min % of ROI pixels above color threshold to consider aurora (default 1.0)
    #[serde(default = "default_area_min")]
    pub area_min_percent: f64,
    /// Aurora score threshold to trigger detection ON (default 1.0)
    #[serde(default = "default_hysteresis_on")]
    pub hysteresis_on: f64,
    /// Aurora score threshold to release detection OFF (default 0.7)
    #[serde(default = "default_hysteresis_off")]
    pub hysteresis_off: f64,
    /// Red channel dominance threshold (default 10.0)
    #[serde(default = "default_red_threshold")]
    pub red_threshold: f64,
    /// Blue/violet channel threshold (default 8.0)
    #[serde(default = "default_blue_threshold")]
    pub blue_threshold: f64,
    /// Enable automatic moon masking (default true)
    #[serde(default = "default_moon_mask_enabled")]
    pub moon_mask_enabled: bool,
    /// Pixel luminance above which it is considered "moon" (default 240.0)
    #[serde(default = "default_moon_lum")]
    pub moon_luminance_threshold: f64,
}

fn default_area_min() -> f64 { 1.0 }
fn default_hysteresis_on() -> f64 { 1.0 }
fn default_hysteresis_off() -> f64 { 0.7 }
fn default_red_threshold() -> f64 { 10.0 }
fn default_blue_threshold() -> f64 { 8.0 }
fn default_moon_mask_enabled() -> bool { true }
fn default_moon_lum() -> f64 { 240.0 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureConfig {
    pub watch_interval_secs: u64,
    pub preview_interval_secs: u64,
    pub output_format: OutputFormat,
    pub focal_length_mm: f64,
    #[serde(default = "default_capture_interval")]
    pub capture_interval_secs: u32,
}

fn default_capture_interval() -> u32 { 10 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRangeConfig {
    pub start: NaiveTime,
    pub end: NaiveTime,
    /// If set, use timer mode: detect for N hours then shutdown. Overrides start/end.
    #[serde(default)]
    pub duration_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub mount_point: String,
    pub warning_percent: u32,
    pub critical_percent: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub ssid: String,
    pub password: String,
    pub channel: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebConfig {
    pub port: u16,
}

// ─── Presets ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub config: AppConfig,
    pub is_builtin: bool,
}

// ─── Config Operations ────────────────────────────────────

impl AppConfig {
    /// Load config from a JSON file. Falls back to default if file doesn't exist.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(path)
            .map_err(|e| ConfigError::IoError(e.to_string()))?;
        let config: Self = serde_json::from_str(&content)
            .map_err(|e| ConfigError::ParseError(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Save config to a JSON file.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ConfigError::IoError(e.to_string()))?;
        }
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ConfigError::SerializeError(e.to_string()))?;
        std::fs::write(path, content)
            .map_err(|e| ConfigError::IoError(e.to_string()))?;
        Ok(())
    }

    /// Validate configuration bounds.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.exposure.iso_min > self.exposure.iso_max {
            return Err(ConfigError::ValidationError(
                "ISO min must be <= ISO max".into(),
            ));
        }
        if self.exposure.shutter_min_us > self.exposure.shutter_max_us {
            return Err(ConfigError::ValidationError(
                "Shutter min must be <= shutter max".into(),
            ));
        }
        if self.detection.roi_top_percent < 50 || self.detection.roi_top_percent > 99 {
            return Err(ConfigError::ValidationError(
                "ROI must be between 50% and 99%".into(),
            ));
        }
        if self.exposure.ev_step_max <= 0.0 || self.exposure.ev_step_max > 2.0 {
            return Err(ConfigError::ValidationError(
                "EV step max must be between 0 and 2".into(),
            ));
        }
        Ok(())
    }

    /// Get the config file path (next to the executable or at a known location).
    pub fn default_path() -> PathBuf {
        PathBuf::from("config/aurion.json")
    }

    /// Get the presets directory path.
    pub fn presets_dir() -> PathBuf {
        PathBuf::from("config/presets")
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            exposure: ExposureConfig {
                iso_min: 100,
                iso_max: 3200,
                shutter_min_us: 1_000_000,
                shutter_max_us: 30_000_000,
                ev_step_max: 0.33,
                detect_alpha: 0.08,
                detect_rate_up: 0.15,
                detect_rate_down: 0.20,
                capture_alpha: 0.03,
                capture_rate_up: 0.03,
                capture_rate_down: 0.05,
                target_percentile: 0.80,
                target_brightness: 60.0,
                saturation_reject: 250.0,
            },
            detection: DetectionConfig {
                roi_top_percent: 65,
                green_threshold: 15.0,
                luminosity_threshold: 30.0,
                variation_threshold: 10.0,
                consecutive_required: 2,
                detection_capture_enabled: false, // SAFE by default
                area_min_percent: 1.0,
                hysteresis_on: 1.0,
                hysteresis_off: 0.7,
                red_threshold: 10.0,
                blue_threshold: 8.0,
                moon_mask_enabled: true,
                moon_luminance_threshold: 240.0,
            },
            capture: CaptureConfig {
                watch_interval_secs: 60,
                preview_interval_secs: 30,
                output_format: OutputFormat::RawDng,
                focal_length_mm: 2.7,
                capture_interval_secs: 10,
            },
            time_range: TimeRangeConfig {
                start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
                end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
                duration_hours: None,
            },
            storage: StorageConfig {
                mount_point: "/mnt/capture".into(),
                warning_percent: 15,
                critical_percent: 5,
            },
            network: NetworkConfig {
                ssid: "Aurion".into(),
                password: "aurora2024".into(),
                channel: 6,
            },
            web: WebConfig { port: 8080 },
            preset_name: "FullDark".into(),
        }
    }
}

// ─── Built-in Presets ──────────────────────────────────────

impl AppConfig {
    pub fn preset_full_dark() -> Self {
        Self::default()
    }

    pub fn preset_semi_polluted() -> Self {
        let mut config = Self::default();
        config.exposure.iso_max = 1600;
        config.exposure.shutter_max_us = 15_000_000;
        config.detection.green_threshold = 20.0;
        config.detection.luminosity_threshold = 40.0;
        config.preset_name = "SemiPolluted".into();
        config
    }

    pub fn preset_moonlight() -> Self {
        let mut config = Self::default();
        config.exposure.iso_max = 800;
        config.exposure.shutter_max_us = 10_000_000;
        config.detection.green_threshold = 25.0;
        config.detection.luminosity_threshold = 50.0;
        config.detection.variation_threshold = 15.0;
        config.preset_name = "Moonlight".into();
        config
    }

    pub fn builtin_presets() -> Vec<Preset> {
        vec![
            Preset {
                name: "FullDark".into(),
                config: Self::preset_full_dark(),
                is_builtin: true,
            },
            Preset {
                name: "SemiPolluted".into(),
                config: Self::preset_semi_polluted(),
                is_builtin: true,
            },
            Preset {
                name: "Moonlight".into(),
                config: Self::preset_moonlight(),
                is_builtin: true,
            },
        ]
    }
}

// ─── Errors ────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("IO error: {0}")]
    IoError(String),
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Serialize error: {0}")]
    SerializeError(String),
    #[error("Validation error: {0}")]
    ValidationError(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = AppConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_iso_range() {
        let mut config = AppConfig::default();
        config.exposure.iso_min = 3200;
        config.exposure.iso_max = 100;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_roi() {
        let mut config = AppConfig::default();
        config.detection.roi_top_percent = 30; // Below 50
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let config = AppConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.exposure.iso_min, config.exposure.iso_min);
        assert_eq!(parsed.preset_name, "FullDark");
    }

    #[test]
    fn test_builtin_presets() {
        let presets = AppConfig::builtin_presets();
        assert_eq!(presets.len(), 3);
        assert!(presets.iter().all(|p| p.is_builtin));
    }
}
