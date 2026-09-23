use async_trait::async_trait;
use tracing::{info, error};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use crate::core::models::{CaptureFormat, CaptureFrame, ExposureSettings, FrameMetadata};
use crate::ports::camera::{CameraError, CameraPort};

/// Raspberry Pi camera adapter using rpicam-still.
/// Uses async tokio::process::Command so captures never block the Tokio runtime.
///
/// Requires: rpicam-apps installed on the Pi.
pub struct CameraRpi {
    /// Whether the camera has been detected
    connected: bool,
    /// `rpicam-still --denoise` mode (ISP noise reduction)
    isp_denoise: String,
    /// `rpicam-still --awb` mode
    awb: String,
    /// `rpicam-still --immediate` (experimental)
    immediate: bool,
}

/// Width used when a capture has no EXIF thumbnail and must be decoded.
const ANALYSIS_MAX_WIDTH: u32 = 640;

impl Default for CameraRpi {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraRpi {
    pub fn new() -> Self {
        // Synchronous check at startup only (before the async runtime is in full use)
        let connected = std::process::Command::new("rpicam-still")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        if connected {
            info!("CameraRpi: rpicam-still detected");
        } else {
            error!("CameraRpi: rpicam-still NOT found — camera unavailable");
        }

        Self { connected, isp_denoise: "cdn_hq".into(), awb: "daylight".into(), immediate: false }
    }

    /// Choose the white balance mode (validated by the config).
    pub fn with_awb(mut self, mode: &str) -> Self {
        if crate::core::config::AWB_MODES.contains(&mode) {
            self.awb = mode.to_string();
        }
        self
    }

    /// Capture without preview phase (experimental, see `CaptureConfig::immediate`).
    pub fn with_immediate(mut self, on: bool) -> Self {
        self.immediate = on;
        self
    }

    /// Choose the ISP denoise mode (validated by the config).
    pub fn with_isp_denoise(mut self, mode: &str) -> Self {
        if crate::core::config::ISP_DENOISE_MODES.contains(&mode) {
            self.isp_denoise = mode.to_string();
        }
        self
    }

    fn build_capture_command(
        &self,
        exposure: &ExposureSettings,
        output_path: &str,
        raw: bool,
    ) -> Command {
        let mut cmd = Command::new("rpicam-still");
        cmd.arg("--nopreview");
        cmd.arg("-o").arg(output_path);

        // Exposure settings
        let shutter_us = exposure.shutter_us;
        cmd.arg("--shutter").arg(shutter_us.to_string());

        // ISO → analogue gain (ISO 100 = gain 1.0, ISO 800 = gain 8.0)
        let gain = exposure.iso as f64 / 100.0;
        cmd.arg("--gain").arg(format!("{:.1}", gain));

        // Fixed white balance by default: same colours on every frame
        cmd.arg("--awb").arg(&self.awb);
        cmd.arg("--ev").arg("0");

        // Colour noise reduction of the ISP (applies to the JPEG, not to the DNG)
        cmd.arg("--denoise").arg(&self.isp_denoise);
        // Small JPEG embedded in the EXIF block: analysed instead of the
        // 12 MP image (≈150× less CPU per frame, i.e. battery life)
        cmd.arg("--thumb").arg("320:240:70");

        if raw {
            cmd.arg("--raw");
        }

        if self.immediate {
            cmd.arg("--immediate");
        } else {
            // Minimal warmup time (ms) — just enough for the ISP pipeline
            cmd.arg("-t").arg("100");
        }

        cmd
    }

    /// Compute a safe async timeout. With long exposures libcamera needs
    /// several frames of the requested duration before the capture, so
    /// allow three times the shutter time plus 20 s of headroom.
    fn capture_timeout_secs(exposure: &ExposureSettings) -> u64 {
        let shutter_secs = exposure.shutter_us.div_ceil(1_000_000);
        shutter_secs * 3 + 20
    }
}

#[async_trait]
impl CameraPort for CameraRpi {
    async fn capture_jpg(&self, exposure: &ExposureSettings, _tmp_dir: &std::path::Path) -> Result<CaptureFrame, CameraError> {
        if !self.connected {
            return Err(CameraError::NotConnected);
        }

        // Always use /tmp for intermediate work files — NOT the USB mount point.
        // The USB is slow and may not be writable during early boot. The orchestrator
        // calls storage.save_file() separately to persist the final output.
        let pid = std::process::id();
        let tmp_path = std::path::Path::new("/tmp").join(format!("aurion_{}_capture.jpg", pid));
        let tmp_path_str = tmp_path.to_string_lossy().to_string();
        let _ = std::fs::remove_file(&tmp_path); // never re-read a previous frame

        let timeout_secs = Self::capture_timeout_secs(exposure);
        let output = timeout(
            Duration::from_secs(timeout_secs),
            self.build_capture_command(exposure, &tmp_path_str, false).kill_on_drop(true).output(),
        )
        .await
        .map_err(|_| CameraError::CaptureFailed(format!("rpicam-still timeout after {}s", timeout_secs)))?
        .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CameraError::CaptureFailed(format!(
                "rpicam-still failed: {}",
                stderr
            )));
        }

        let data = std::fs::read(&tmp_path)
            .map_err(|e| CameraError::CaptureFailed(format!("JPG read failed from {:?}: {}", tmp_path, e)))?;

        let (rgb, w, h, from_thumb) = crate::core::jpeg::decode_for_analysis(&data, ANALYSIS_MAX_WIDTH)
            .ok_or_else(|| CameraError::CaptureFailed("JPEG illisible".into()))?;
        info!("CameraRpi: captured JPG ({} bytes), analysis {}x{}{}", data.len(), w, h,
            if from_thumb { " (EXIF thumbnail)" } else { "" });

        Ok(CaptureFrame {
            data: rgb,
            width: w,
            height: h,
            format: CaptureFormat::Jpg,
            metadata: FrameMetadata {
                iso: exposure.iso,
                shutter_us: exposure.shutter_us,
                timestamp: chrono::Utc::now(),
            },
            raw_bytes: data, // original JPEG bytes — used for lossless save
        })
    }

    async fn capture_raw(&self, exposure: &ExposureSettings, _tmp_dir: &std::path::Path) -> Result<CaptureFrame, CameraError> {
        if !self.connected {
            return Err(CameraError::NotConnected);
        }

        // Always use /tmp for intermediate work files.
        let pid = std::process::id();
        let tmp_jpg = std::path::Path::new("/tmp").join(format!("aurion_{}_raw.jpg", pid));
        let tmp_dng = std::path::Path::new("/tmp").join(format!("aurion_{}_raw.dng", pid));
        let tmp_jpg_str = tmp_jpg.to_string_lossy().to_string();
        // Never re-read the files of a previous frame if this capture fails.
        let _ = std::fs::remove_file(&tmp_jpg);
        let _ = std::fs::remove_file(&tmp_dng);

        let timeout_secs = Self::capture_timeout_secs(exposure);

        // rpicam-still --raw produces a DNG alongside the JPG
        let output = timeout(
            Duration::from_secs(timeout_secs),
            self.build_capture_command(exposure, &tmp_jpg_str, true).kill_on_drop(true).output(),
        )
        .await
        .map_err(|_| CameraError::CaptureFailed(format!("rpicam-still RAW timeout after {}s", timeout_secs)))?
        .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CameraError::CaptureFailed(format!(
                "rpicam-still raw failed: {}",
                stderr
            )));
        }

        // Read the DNG file
        let dng_data = std::fs::read(&tmp_dng)
            .map_err(|e| CameraError::CaptureFailed(format!("DNG read failed from {:?}: {}", tmp_dng, e)))?;

        info!("CameraRpi: captured RAW DNG ({} bytes)", dng_data.len());

        // No pixel decoding here: the DNG is saved as is, analysis uses the JPG.
        Ok(CaptureFrame {
            data: Vec::new(),
            width: 0,
            height: 0,
            format: CaptureFormat::RawDng,
            metadata: FrameMetadata {
                iso: exposure.iso,
                shutter_us: exposure.shutter_us,
                timestamp: chrono::Utc::now(),
            },
            raw_bytes: dng_data, // original DNG bytes — used for lossless save
        })
    }

    async fn capture_raw_and_jpg(
        &self,
        exposure: &ExposureSettings,
        _tmp_dir: &std::path::Path,
    ) -> Result<(CaptureFrame, CaptureFrame), CameraError> {
        // Single capture: raw() produces both DNG + JPG in one rpicam-still call
        // Pass /tmp explicitly — capture_raw ignores its _tmp_dir anyway
        let tmp = std::path::Path::new("/tmp");
        let raw = self.capture_raw(exposure, tmp).await?;

        // The JPG was produced alongside the DNG by rpicam-still --raw
        let pid = std::process::id();
        let tmp_jpg = std::path::Path::new("/tmp").join(format!("aurion_{}_raw.jpg", pid));
        let jpg_data = std::fs::read(&tmp_jpg)
            .map_err(|e| CameraError::CaptureFailed(format!("JPG read failed from {:?}: {}", tmp_jpg, e)))?;

        let (rgb, w, h, _) = crate::core::jpeg::decode_for_analysis(&jpg_data, ANALYSIS_MAX_WIDTH)
            .ok_or_else(|| CameraError::CaptureFailed("JPEG illisible".into()))?;

        let jpg = CaptureFrame {
            data: rgb,
            width: w,
            height: h,
            format: CaptureFormat::Jpg,
            metadata: raw.metadata.clone(),
            raw_bytes: jpg_data, // original JPEG bytes — used for lossless save
        };

        Ok((raw, jpg))
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
