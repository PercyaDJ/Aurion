use async_trait::async_trait;
use tracing::{info, error};
use tokio::process::Command;
use tokio::time::{timeout, Duration};

use image::GenericImageView;
use crate::core::models::{CaptureFormat, CaptureFrame, ExposureSettings, FrameMetadata};
use crate::ports::camera::{CameraError, CameraPort};

/// Raspberry Pi camera adapter using rpicam-still.
/// Uses async tokio::process::Command so captures never block the Tokio runtime.
///
/// Requires: rpicam-apps installed on the Pi.
pub struct CameraRpi {
    /// Whether the camera has been detected
    connected: bool,
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

        Self { connected }
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

        // Auto white balance (safe default)
        cmd.arg("--awb").arg("auto");
        cmd.arg("--ev").arg("0");

        if raw {
            cmd.arg("--raw");
        }

        // Minimal warmup time (ms) — just enough for the ISP pipeline
        cmd.arg("-t").arg("100");

        cmd
    }

    /// Compute a safe async timeout: shutter duration + 15s headroom.
    fn capture_timeout_secs(exposure: &ExposureSettings) -> u64 {
        let shutter_secs = exposure.shutter_us / 1_000_000;
        shutter_secs + 15
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

        let timeout_secs = Self::capture_timeout_secs(exposure);
        let output = timeout(
            Duration::from_secs(timeout_secs),
            self.build_capture_command(exposure, &tmp_path_str, false).output(),
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

        // Decode to get dimensions
        let img = image::load_from_memory(&data)
            .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

        let rgb = img.to_rgb8();
        let (w, h) = rgb.dimensions();

        info!("CameraRpi: captured JPG {}x{}", w, h);

        Ok(CaptureFrame {
            data: rgb.into_raw(),
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

        let timeout_secs = Self::capture_timeout_secs(exposure);

        // rpicam-still --raw produces a DNG alongside the JPG
        let output = timeout(
            Duration::from_secs(timeout_secs),
            self.build_capture_command(exposure, &tmp_jpg_str, true).output(),
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

        // Read actual dimensions from the accompanying JPG
        let jpg_data = std::fs::read(&tmp_jpg).ok();
        let (w, h) = jpg_data
            .and_then(|d| image::load_from_memory(&d).ok())
            .map(|img| img.dimensions())
            .unwrap_or((4056, 3040)); // fallback: IMX477 full res

        info!("CameraRpi: captured RAW DNG ({} bytes, {}x{})", dng_data.len(), w, h);

        Ok(CaptureFrame {
            data: dng_data.clone(),
            width: w,
            height: h,
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

        let img = image::load_from_memory(&jpg_data)
            .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;
        let rgb = img.to_rgb8();
        let (w, h) = rgb.dimensions();

        let jpg = CaptureFrame {
            data: rgb.into_raw(),
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
