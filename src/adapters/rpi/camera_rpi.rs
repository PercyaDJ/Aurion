use async_trait::async_trait;
use tracing::{info, error};
use std::process::Command;

use crate::core::models::{CaptureFormat, CaptureFrame, ExposureSettings, FrameMetadata};
use crate::ports::camera::{CameraError, CameraPort};

/// Raspberry Pi camera adapter using rpicam-still.
/// Captures images via the rpicam command-line tools.
///
/// Requires: rpicam-apps installed on the Pi.
pub struct CameraRpi {
    /// Whether the camera has been detected
    connected: bool,
}

impl CameraRpi {
    pub fn new() -> Self {
        // Check if rpicam-still is available
        let connected = Command::new("rpicam-still")
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

        // Disable auto-exposure, use fixed greyworld WB for night sky
        cmd.arg("--awb").arg("greyworld");
        cmd.arg("--ev").arg("0");

        if raw {
            cmd.arg("--raw");
        }

        // Immediate capture (no preview delay)
        cmd.arg("--immediate");
        cmd.arg("-t").arg("1");

        cmd
    }
}

#[async_trait]
impl CameraPort for CameraRpi {
    async fn capture_jpg(&self, exposure: &ExposureSettings) -> Result<CaptureFrame, CameraError> {
        if !self.connected {
            return Err(CameraError::NotConnected);
        }

        let tmp_path = "/tmp/aurora_capture.jpg";
        let output = self
            .build_capture_command(exposure, tmp_path, false)
            .output()
            .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CameraError::CaptureFailed(format!(
                "rpicam-still failed: {},",
                stderr
            )));
        }

        let data = std::fs::read(tmp_path)
            .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

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
        })
    }

    async fn capture_raw(&self, exposure: &ExposureSettings) -> Result<CaptureFrame, CameraError> {
        if !self.connected {
            return Err(CameraError::NotConnected);
        }

        let tmp_jpg = "/tmp/aurora_capture_raw.jpg";
        let tmp_dng = "/tmp/aurora_capture_raw.dng";

        // rpicam-still --raw produces a DNG alongside the JPG
        let output = self
            .build_capture_command(exposure, tmp_jpg, true)
            .output()
            .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(CameraError::CaptureFailed(format!(
                "rpicam-still raw failed: {},",
                stderr
            )));
        }

        // Read the DNG file
        let dng_data = std::fs::read(tmp_dng)
            .map_err(|e| CameraError::CaptureFailed(format!("DNG read failed: {}", e)))?;

        info!("CameraRpi: captured RAW DNG ({} bytes)", dng_data.len());

        Ok(CaptureFrame {
            data: dng_data,
            width: 4056, // IMX477 full res
            height: 3040,
            format: CaptureFormat::RawDng,
            metadata: FrameMetadata {
                iso: exposure.iso,
                shutter_us: exposure.shutter_us,
                timestamp: chrono::Utc::now(),
            },
        })
    }

    async fn capture_raw_and_jpg(
        &self,
        exposure: &ExposureSettings,
    ) -> Result<(CaptureFrame, CaptureFrame), CameraError> {
        // Capture raw first (produces both DNG + JPG)
        let raw = self.capture_raw(exposure).await?;

        // Read the JPG that was produced alongside
        let jpg_data = std::fs::read("/tmp/aurora_capture_raw.jpg")
            .map_err(|e| CameraError::CaptureFailed(e.to_string()))?;

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
        };

        Ok((raw, jpg))
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
