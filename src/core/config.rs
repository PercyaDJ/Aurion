use serde::{Deserialize, Serialize};
use chrono::NaiveTime;
use std::path::{Path, PathBuf};
use crate::core::models::OutputFormat;
use crate::core::validate;

/// Factory Wi-Fi password. The installer replaces it by a random one; the UI
/// warns while it is still in use.
pub const DEFAULT_WIFI_PASSWORD: &str = "aurora2024";

/// Placeholder returned by the API instead of the real Wi-Fi password.
pub const PASSWORD_MASK: &str = "********";

/// Minimum Wi-Fi password length enforced by Aurion (WPA2 allows 8).
pub const MIN_WIFI_PASSWORD_LEN: usize = 10;

/// Write `data` to `path` atomically with the given Unix permissions.
pub fn write_atomic(path: &Path, data: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = parent.join(format!(".{}.tmp.{}", file_name, std::process::id()));
    {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(mode)
            .open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    // Best effort: persist the rename itself.
    if let Ok(dir) = std::fs::File::open(parent) {
        let _ = dir.sync_all();
    }
    Ok(())
}

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
    /// Several nights in a row without anyone touching the camera.
    #[serde(default)]
    pub expedition: ExpeditionConfig,
}

/// Expedition mode: at power-on the night starts on its own once nobody has
/// used the interface for a few minutes (the clock must be reliable), and at
/// the end of the night a Raspberry Pi 5 programs its own wake-up for the
/// next evening.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ExpeditionConfig {
    pub enabled: bool,
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
    /// Freeze the exposure once the capture (RUN) has started: no flicker
    /// in timelapses. Default false (slow adaptation, 3-5 % per frame).
    #[serde(default)]
    pub lock_in_run: bool,
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
    /// Noise reduction applied to the saved JPEG images.
    #[serde(default)]
    pub denoise: DenoiseConfig,
    /// White balance of the camera. A fixed mode ("daylight") keeps the
    /// same colours on every frame: no colour flicker in timelapses, and
    /// the DNG "as shot" white balance is identical across the sequence.
    #[serde(default = "default_awb")]
    pub awb: String,
}

fn default_awb() -> String { "daylight".into() }

/// Accepted values for `rpicam-still --awb`.
pub const AWB_MODES: [&str; 7] = ["daylight", "cloudy", "auto", "incandescent", "tungsten", "fluorescent", "indoor"];

fn default_capture_interval() -> u32 { 10 }

/// Noise reduction settings (see `core::denoise`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DenoiseConfig {
    /// Remove isolated hot pixels from the saved JPEG (default false: it
    /// costs a full decode + re-encode per frame, i.e. battery and sensor
    /// heat; the DNG is never modified).
    #[serde(default)]
    pub hot_pixels: bool,
    /// Brightness excess over the neighbours that marks a hot pixel (default 40).
    #[serde(default = "default_hot_pixel_threshold")]
    pub hot_pixel_threshold: u8,
    /// Also save an average of N consecutive frames (`_STACKn.jpg`), 0 = off.
    /// 4 to 8 frames divide the noise by 2 to 3 (fast-moving aurora gets blurred).
    #[serde(default)]
    pub stack_frames: u32,
    /// Denoise of the camera ISP: "cdn_hq" (best), "cdn_fast", "cdn_off", "off", "auto".
    #[serde(default = "default_isp_denoise")]
    pub isp_denoise: String,
}

fn default_hot_pixel_threshold() -> u8 { 40 }
fn default_isp_denoise() -> String { "cdn_hq".into() }

/// Accepted values for `rpicam-still --denoise`.
pub const ISP_DENOISE_MODES: [&str; 5] = ["auto", "off", "cdn_off", "cdn_fast", "cdn_hq"];

impl Default for DenoiseConfig {
    fn default() -> Self {
        Self {
            hot_pixels: false,
            hot_pixel_threshold: default_hot_pixel_threshold(),
            stack_frames: 0,
            isp_denoise: default_isp_denoise(),
        }
    }
}

impl DenoiseConfig {
    /// Does any treatment require decoding the full-resolution image?
    pub fn needs_full_decode(&self) -> bool {
        self.hot_pixels || self.stack_frames >= 2
    }
}

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
    ///
    /// The write is atomic (temporary file + fsync + rename) so a power cut
    /// in the field can never leave a truncated config behind, and the file
    /// is created with mode 0600 because it contains the Wi-Fi password.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| ConfigError::SerializeError(e.to_string()))?;
        write_atomic(path, content.as_bytes(), 0o600)
            .map_err(|e| ConfigError::IoError(e.to_string()))
    }

    /// Build a new config from a preset: only the camera/detection/schedule
    /// parameters come from the preset. Network, web and storage settings are
    /// machine specific and always kept from `self` (a preset must never
    /// reset the Wi-Fi password to its default value).
    pub fn with_preset(&self, preset: &AppConfig, preset_name: &str) -> AppConfig {
        AppConfig {
            exposure: preset.exposure.clone(),
            detection: preset.detection.clone(),
            capture: preset.capture.clone(),
            time_range: preset.time_range.clone(),
            storage: self.storage.clone(),
            network: self.network.clone(),
            web: self.web.clone(),
            preset_name: preset_name.to_string(),
            expedition: self.expedition.clone(),
        }
    }

    /// Copy of the config safe to expose or store in a preset file
    /// (Wi-Fi password replaced by the mask).
    pub fn masked(&self) -> AppConfig {
        let mut cfg = self.clone();
        cfg.network.password = PASSWORD_MASK.to_string();
        cfg
    }

    /// Validate configuration bounds.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // ─── Exposure ────────────────────────────────────────
        if self.exposure.iso_min > self.exposure.iso_max {
            return Err(ConfigError::ValidationError(
                "ISO min doit être ≤ ISO max".into(),
            ));
        }
        if self.exposure.shutter_min_us > self.exposure.shutter_max_us {
            return Err(ConfigError::ValidationError(
                "Obturateur min doit être ≤ obturateur max".into(),
            ));
        }
        if self.exposure.iso_min < 1 || self.exposure.iso_max > 25_600 {
            return Err(ConfigError::ValidationError(
                "ISO doit être entre 1 et 25600".into(),
            ));
        }
        if self.exposure.shutter_min_us < 1 || self.exposure.shutter_max_us > 240_000_000 {
            return Err(ConfigError::ValidationError(
                "L'obturateur doit être entre 1 µs et 240 s".into(),
            ));
        }
        if !(0.0..=1.0).contains(&self.exposure.target_percentile)
            || !(1.0..=255.0).contains(&self.exposure.target_brightness)
            || !(1.0..=255.0).contains(&self.exposure.saturation_reject)
        {
            return Err(ConfigError::ValidationError(
                "Paramètres de mesure d'exposition hors bornes".into(),
            ));
        }
        if self.exposure.ev_step_max <= 0.0 || self.exposure.ev_step_max > 2.0 {
            return Err(ConfigError::ValidationError(
                "EV step max doit être entre 0 et 2".into(),
            ));
        }

        // ─── Detection ───────────────────────────────────────
        if self.detection.roi_top_percent < 50 || self.detection.roi_top_percent > 99 {
            return Err(ConfigError::ValidationError(
                "ROI doit être entre 50% et 99%".into(),
            ));
        }
        if self.detection.consecutive_required < 1 || self.detection.consecutive_required > 100 {
            return Err(ConfigError::ValidationError(
                "Le nombre de détections consécutives doit être entre 1 et 100".into(),
            ));
        }
        if self.detection.hysteresis_off >= self.detection.hysteresis_on {
            return Err(ConfigError::ValidationError(
                "Hystérésis OFF doit être < hystérésis ON (sinon la détection ne peut jamais basculer)".into(),
            ));
        }
        if self.detection.green_threshold < 0.0
            || self.detection.red_threshold < 0.0
            || self.detection.blue_threshold < 0.0
        {
            return Err(ConfigError::ValidationError(
                "Les seuils de détection doivent être ≥ 0".into(),
            ));
        }

        // ─── Capture ─────────────────────────────────────────
        if self.capture.capture_interval_secs < 1 {
            return Err(ConfigError::ValidationError(
                "L'intervalle de capture doit être >= 1 seconde".into(),
            ));
        }
        if self.capture.watch_interval_secs < 5 {
            return Err(ConfigError::ValidationError(
                "L'intervalle d'observation doit être >= 5 secondes".into(),
            ));
        }
        if self.capture.capture_interval_secs as u64 > self.capture.watch_interval_secs {
            return Err(ConfigError::ValidationError(
                "L'intervalle de capture ne peut pas dépasser l'intervalle d'observation".into(),
            ));
        }
        if self.capture.denoise.stack_frames == 1 || self.capture.denoise.stack_frames > 16 {
            return Err(ConfigError::ValidationError(
                "L'empilement doit être désactivé (0) ou compter entre 2 et 16 images".into(),
            ));
        }
        if !ISP_DENOISE_MODES.contains(&self.capture.denoise.isp_denoise.as_str()) {
            return Err(ConfigError::ValidationError(format!(
                "Débruitage capteur inconnu (valeurs : {})",
                ISP_DENOISE_MODES.join(", ")
            )));
        }
        if !AWB_MODES.contains(&self.capture.awb.as_str()) {
            return Err(ConfigError::ValidationError(format!(
                "Balance des blancs inconnue (valeurs : {})",
                AWB_MODES.join(", ")
            )));
        }
        if self.capture.denoise.hot_pixel_threshold < 10 {
            return Err(ConfigError::ValidationError(
                "Le seuil des pixels chauds doit être d'au moins 10".into(),
            ));
        }
        if self.capture.focal_length_mm <= 0.0 {
            return Err(ConfigError::ValidationError(
                "La longueur focale doit être > 0 mm".into(),
            ));
        }

        if let Some(hours) = self.time_range.duration_hours {
            if !(hours > 0.0 && hours <= 48.0) {
                return Err(ConfigError::ValidationError(
                    "La durée du minuteur doit être entre 0 et 48 heures".into(),
                ));
            }
        }

        // ─── Stockage ────────────────────────────────────────
        if self.storage.warning_percent > 100
            || self.storage.critical_percent >= self.storage.warning_percent
        {
            return Err(ConfigError::ValidationError(
                "Le seuil critique doit être inférieur au seuil d'alerte (≤ 100 %)".into(),
            ));
        }
        if !self.storage.mount_point.starts_with('/') && !self.storage.mount_point.starts_with('.') {
            return Err(ConfigError::ValidationError(
                "Le point de montage doit être un chemin".into(),
            ));
        }

        // ─── Réseau ──────────────────────────────────────────
        validate::validate_ssid(&self.network.ssid).map_err(ConfigError::ValidationError)?;
        validate::validate_wpa_passphrase(&self.network.password, MIN_WIFI_PASSWORD_LEN)
            .map_err(ConfigError::ValidationError)?;
        validate::validate_channel(self.network.channel).map_err(ConfigError::ValidationError)?;

        // ─── Web ─────────────────────────────────────────────
        if self.web.port < 1024 {
            return Err(ConfigError::ValidationError(
                "Le port web doit être >= 1024 (ports < 1024 sont réservés au système)".into(),
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
                lock_in_run: false,
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
                denoise: DenoiseConfig::default(),
                awb: default_awb(),
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
                password: DEFAULT_WIFI_PASSWORD.into(),
                channel: 6,
            },
            web: WebConfig { port: 8080 },
            preset_name: "FullDark".into(),
            expedition: ExpeditionConfig::default(),
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

// ─── Password reset from the USB key ──────────────────────

/// File that resets the Wi-Fi password when found at the root of the key.
pub const WIFI_RESET_FILE: &str = "aurion-reset-wifi.txt";

/// "Forgot the Wi-Fi password": if `aurion-reset-wifi.txt` is present at the
/// root of the capture drive, restore the factory password, rename the file
/// (`.done`) and return true. The caller saves the config.
pub fn apply_usb_wifi_reset(config: &mut AppConfig) -> bool {
    let path = Path::new(&config.storage.mount_point).join(WIFI_RESET_FILE);
    if !path.is_file() {
        return false;
    }
    config.network.password = DEFAULT_WIFI_PASSWORD.to_string();
    let _ = std::fs::rename(&path, path.with_extension("txt.done"));
    true
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
        config.detection.roi_top_percent = 20; // Below the 30% minimum
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_hysteresis() {
        let mut config = AppConfig::default();
        // hysteresis_off >= hysteresis_on → detection can never flip off
        config.detection.hysteresis_off = 0.8;
        config.detection.hysteresis_on = 0.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_capture_interval() {
        let mut config = AppConfig::default();
        config.capture.capture_interval_secs = 0; // Must be >= 1
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

    #[test]
    fn test_short_wifi_password() {
        let mut config = AppConfig::default();
        config.network.password = "court".into(); // 5 chars < 10
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_privileged_port() {
        let mut config = AppConfig::default();
        config.web.port = 80; // port privilégié
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_zero_focal_length() {
        let mut config = AppConfig::default();
        config.capture.focal_length_mm = 0.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_ssid_injection_rejected() {
        let mut config = AppConfig::default();
        config.network.ssid = "Aurion\nwpa=0".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_password_too_long_or_non_ascii() {
        let mut config = AppConfig::default();
        config.network.password = "x".repeat(64);
        assert!(config.validate().is_err());
        config.network.password = "motdepasseé1".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_channel() {
        let mut config = AppConfig::default();
        config.network.channel = 0;
        assert!(config.validate().is_err());
        config.network.channel = 165;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_storage_thresholds_order() {
        let mut config = AppConfig::default();
        config.storage.warning_percent = 5;
        config.storage.critical_percent = 10;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_timer_bounds() {
        let mut config = AppConfig::default();
        config.time_range.duration_hours = Some(0.0);
        assert!(config.validate().is_err());
        config.time_range.duration_hours = Some(100.0);
        assert!(config.validate().is_err());
        config.time_range.duration_hours = Some(8.0);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_all_builtin_presets_are_valid() {
        for preset in AppConfig::builtin_presets() {
            assert!(preset.config.validate().is_ok(), "preset {} invalide", preset.name);
        }
    }

    #[test]
    fn test_with_preset_keeps_machine_settings() {
        let mut current = AppConfig::default();
        current.network.password = "MonSuperMotDePasse".into();
        current.network.ssid = "Aurion-Nord".into();
        current.storage.mount_point = "/media/usb".into();
        let preset = AppConfig::preset_moonlight();
        let merged = current.with_preset(&preset, "Moonlight");
        assert_eq!(merged.network.password, "MonSuperMotDePasse");
        assert_eq!(merged.network.ssid, "Aurion-Nord");
        assert_eq!(merged.storage.mount_point, "/media/usb");
        assert_eq!(merged.exposure.iso_max, 800);
        assert_eq!(merged.preset_name, "Moonlight");
    }

    #[test]
    fn test_save_is_atomic_and_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/aurion.json");
        let config = AppConfig::default();
        config.save(&path).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let loaded = AppConfig::load(&path).unwrap();
        assert_eq!(loaded.network.ssid, config.network.ssid);
        // No temporary file left behind
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn test_default_json_file_loads() {
        // Non-regression: the shipped config/default.json must stay loadable.
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("config/default.json");
        let cfg = AppConfig::load(&path).expect("config/default.json doit être valide");
        assert_eq!(cfg.web.port, 8080);
    }

    #[test]
    fn test_denoise_validation() {
        let mut config = AppConfig::default();
        assert!(!config.capture.denoise.hot_pixels, "off by default (battery, sensor heat)");
        config.capture.awb = "rainbow".into();
        assert!(config.validate().is_err());
        config.capture.awb = "daylight".into();
        config.capture.denoise.stack_frames = 1;
        assert!(config.validate().is_err());
        config.capture.denoise.stack_frames = 17;
        assert!(config.validate().is_err());
        config.capture.denoise.stack_frames = 4;
        assert!(config.validate().is_ok());
        config.capture.denoise.isp_denoise = "magic; rm -rf /".into();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_usb_wifi_reset() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = AppConfig::default();
        config.storage.mount_point = dir.path().to_string_lossy().to_string();
        config.network.password = "MotDePasseOublie".into();
        assert!(!apply_usb_wifi_reset(&mut config), "no file, no reset");
        std::fs::write(dir.path().join(WIFI_RESET_FILE), b"").unwrap();
        assert!(apply_usb_wifi_reset(&mut config));
        assert_eq!(config.network.password, DEFAULT_WIFI_PASSWORD);
        assert!(!dir.path().join(WIFI_RESET_FILE).exists(), "file consumed");
        assert!(dir.path().join("aurion-reset-wifi.txt.done").exists());
        assert!(!apply_usb_wifi_reset(&mut config), "only once");
    }

    #[test]
    fn test_capture_interval_exceeds_watch() {
        let mut config = AppConfig::default();
        config.capture.watch_interval_secs = 10;
        config.capture.capture_interval_secs = 15; // > watch_interval
        assert!(config.validate().is_err());
    }
}
