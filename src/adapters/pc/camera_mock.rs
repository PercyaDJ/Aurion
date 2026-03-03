use async_trait::async_trait;
use std::sync::Mutex;
use std::path::PathBuf;
use tracing::info;

use crate::core::models::{CaptureFormat, CaptureFrame, ExposureSettings, FrameMetadata};
use crate::ports::camera::{CameraError, CameraPort};

/// PC mock camera: reads JPG files from a test data directory.
/// Cycles through files in order to simulate a sequence of captures.
pub struct CameraMock {
    test_dir: PathBuf,
    frame_index: Mutex<usize>,
    connected: bool,
}

impl CameraMock {
    pub fn new(test_dir: PathBuf) -> Self {
        Self {
            test_dir,
            frame_index: Mutex::new(0),
            connected: true,
        }
    }

    /// Create a mock that generates synthetic frames (no test data needed).
    pub fn synthetic() -> Self {
        Self {
            test_dir: PathBuf::from("testdata/watch_sequences"),
            frame_index: Mutex::new(0),
            connected: true,
        }
    }

    fn generate_synthetic_frame(
        &self,
        exposure: &ExposureSettings,
        index: usize,
    ) -> CaptureFrame {
        // Generate a 320x240 synthetic image
        let width = 320u32;
        let height = 240u32;
        let mut data = Vec::with_capacity((width * height * 3) as usize);

        for y in 0..height {
            for x in 0..width {
                // Base: dark sky with slight gradient
                let base = 5u8;
                let gradient = (y as f64 / height as f64 * 10.0) as u8;

                // Simulate aurora at certain frame indices (every 5th frame)
                let is_aurora_frame = index % 5 >= 3;

                if is_aurora_frame && y < height * 65 / 100 {
                    // Green aurora glow in top 65%
                    let intensity = (1.0 - (y as f64 / (height as f64 * 0.65))) * 80.0;
                    let wave = ((x as f64 / 20.0 + index as f64).sin() * 20.0) as u8;
                    data.push(base + (intensity * 0.3) as u8); // R
                    data.push(base + intensity as u8 + wave);   // G
                    data.push(base + (intensity * 0.4) as u8); // B
                } else {
                    // Dark sky / ground
                    data.push(base + gradient);
                    data.push(base + gradient);
                    data.push(base + gradient);
                }
            }
        }

        CaptureFrame {
            data,
            width,
            height,
            format: CaptureFormat::Jpg,
            metadata: FrameMetadata {
                iso: exposure.iso,
                shutter_us: exposure.shutter_us,
                timestamp: chrono::Utc::now(),
            },
            raw_bytes: vec![],
        }
    }

    fn next_index(&self) -> usize {
        let mut idx = self.frame_index.lock().unwrap();
        let current = *idx;
        *idx += 1;
        current
    }
}

#[async_trait]
impl CameraPort for CameraMock {
    async fn capture_jpg(&self, exposure: &ExposureSettings, _tmp_dir: &std::path::Path) -> Result<CaptureFrame, CameraError> {
        if !self.connected {
            return Err(CameraError::NotConnected);
        }

        let index = self.next_index();

        // Try to read from test data files
        let files: Vec<PathBuf> = std::fs::read_dir(&self.test_dir)
            .ok()
            .map(|entries| {
                let mut files: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension()
                            .map(|e| e == "jpg" || e == "jpeg" || e == "png")
                            .unwrap_or(false)
                    })
                    .collect();
                files.sort();
                files
            })
            .unwrap_or_default();

        if !files.is_empty() {
            let file_path = &files[index % files.len()];
            info!("CameraMock: reading {}", file_path.display());

            let img = image::open(file_path).map_err(|e| CameraError::CaptureFailed(e.to_string()))?;
            let rgb = img.to_rgb8();
            let (w, h) = rgb.dimensions();

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
                raw_bytes: vec![],
            })
        } else {
            // No test data: use synthetic frame
            info!("CameraMock: generating synthetic frame #{}", index);
            Ok(self.generate_synthetic_frame(exposure, index))
        }
    }

    async fn capture_raw(&self, exposure: &ExposureSettings, tmp_dir: &std::path::Path) -> Result<CaptureFrame, CameraError> {
        // Mock: return same as JPG but marked as RAW
        let mut frame = self.capture_jpg(exposure, tmp_dir).await?;
        frame.format = CaptureFormat::RawDng;
        Ok(frame)
    }

    async fn capture_raw_and_jpg(
        &self,
        exposure: &ExposureSettings,
        tmp_dir: &std::path::Path,
    ) -> Result<(CaptureFrame, CaptureFrame), CameraError> {
        let raw = self.capture_raw(exposure, tmp_dir).await?;
        let jpg = CaptureFrame {
            data: raw.data.clone(),
            width: raw.width,
            height: raw.height,
            format: CaptureFormat::Jpg,
            metadata: raw.metadata.clone(),
            raw_bytes: vec![],
        };
        Ok((raw, jpg))
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
