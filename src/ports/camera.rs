use async_trait::async_trait;
use crate::core::models::{CaptureFrame, ExposureSettings};

use std::path::Path;

/// Port for camera operations.
/// Implementations: CameraMock (PC), CameraRpi (Raspberry Pi).
#[async_trait]
pub trait CameraPort: Send + Sync {
    /// Capture a JPG image with the given exposure settings.
    async fn capture_jpg(&self, exposure: &ExposureSettings, tmp_dir: &Path) -> Result<CaptureFrame, CameraError>;

    /// Capture a RAW DNG image with the given exposure settings.
    async fn capture_raw(&self, exposure: &ExposureSettings, tmp_dir: &Path) -> Result<CaptureFrame, CameraError>;

    /// Capture both RAW and JPG simultaneously.
    async fn capture_raw_and_jpg(
        &self,
        exposure: &ExposureSettings,
        tmp_dir: &Path,
    ) -> Result<(CaptureFrame, CaptureFrame), CameraError>;

    /// Check if the camera is connected and operational.
    fn is_connected(&self) -> bool;
}

#[derive(Debug, thiserror::Error)]
pub enum CameraError {
    #[error("Camera not connected")]
    NotConnected,
    #[error("Capture failed: {0}")]
    CaptureFailed(String),
    #[error("Invalid exposure settings: {0}")]
    InvalidSettings(String),
}
