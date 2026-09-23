use async_trait::async_trait;
use std::sync::Mutex;
use std::path::PathBuf;
use tracing::info;

use crate::core::models::{CaptureFormat, CaptureFrame, ExposureSettings, FrameMetadata};
use crate::ports::camera::{CameraError, CameraPort};

/// Sky simulated by the synthetic frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyPattern {
    /// Aurora on frames where `index % 5 >= 3` (two consecutive hits every 5 frames).
    Intermittent,
    /// Always a dark sky, never an aurora.
    Dark,
    /// Aurora on every frame.
    Aurora,
}

/// PC mock camera: reads JPG files from a test data directory.
/// Cycles through files in order to simulate a sequence of captures.
/// Without test files, generates synthetic frames following a [`SkyPattern`].
pub struct CameraMock {
    test_dir: PathBuf,
    frame_index: Mutex<usize>,
    connected: bool,
    pattern: SkyPattern,
    /// Number of upcoming captures that must fail (failure injection).
    failures_left: Mutex<u32>,
    /// Total number of capture attempts.
    attempts: Mutex<u32>,
    /// Produce Pi-like captures: full-resolution JPEG (with hot pixels) in
    /// `raw_bytes` + EXIF thumbnail used for analysis.
    jpeg_output: bool,
}

/// Size of the full-resolution JPEG produced in `jpeg_output` mode.
const MOCK_FULL_W: u32 = 1280;
const MOCK_FULL_H: u32 = 960;
/// Hot pixels added to each full-resolution mock capture.
pub const MOCK_HOT_PIXELS: usize = 50;

impl CameraMock {
    pub fn new(test_dir: PathBuf) -> Self {
        Self {
            test_dir,
            frame_index: Mutex::new(0),
            connected: true,
            pattern: SkyPattern::Intermittent,
            failures_left: Mutex::new(0),
            attempts: Mutex::new(0),
            jpeg_output: false,
        }
    }

    /// Produce captures like `rpicam-still` (see [`CameraMock::jpeg_output`]).
    pub fn with_jpeg_output(mut self) -> Self {
        self.jpeg_output = true;
        self
    }

    /// Turn a 320×240 synthetic frame into a Pi-like capture.
    fn to_pi_capture(&self, mut frame: CaptureFrame, index: usize) -> CaptureFrame {
        use crate::core::jpeg;
        let small = image::RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
            .expect("synthetic frame size");
        let mut full = image::imageops::resize(&small, MOCK_FULL_W, MOCK_FULL_H, image::imageops::FilterType::Triangle);
        // Hot pixels: same sensor positions on every frame
        let mut rng = crate::core::denoise::XorShift::new(42);
        for _ in 0..MOCK_HOT_PIXELS {
            let x = 4 + rng.next_u32() % (MOCK_FULL_W - 8);
            let y = 4 + rng.next_u32() % (MOCK_FULL_H - 8);
            full.put_pixel(x, y, image::Rgb([255, 255, 255]));
        }
        let _ = index;
        let full_jpeg = crate::core::orchestrator::encode_jpeg(full.as_raw(), MOCK_FULL_W, MOCK_FULL_H).expect("encode");
        let thumb = crate::core::orchestrator::encode_jpeg(&frame.data, frame.width, frame.height).expect("encode");
        frame.raw_bytes = jpeg::insert_segment(&full_jpeg, &jpeg::build_exif_with_thumbnail(&thumb, true));
        let (rgb, w, h, _) = jpeg::decode_for_analysis(&frame.raw_bytes, 640).expect("decode");
        frame.data = rgb;
        frame.width = w;
        frame.height = h;
        frame
    }

    /// Create a mock that generates synthetic frames (no test data needed).
    pub fn synthetic() -> Self {
        Self::new(PathBuf::from("testdata/watch_sequences"))
    }

    /// Synthetic frames following `pattern` (ignores test data files).
    pub fn with_pattern(pattern: SkyPattern) -> Self {
        let mut cam = Self::new(PathBuf::from("/nonexistent/aurion-testdata"));
        cam.pattern = pattern;
        cam
    }

    /// Make the next `n` captures fail.
    pub fn fail_next(self, n: u32) -> Self {
        *self.failures_left.lock().unwrap() = n;
        self
    }

    /// A camera that is not plugged in.
    pub fn disconnected() -> Self {
        let mut cam = Self::with_pattern(SkyPattern::Dark);
        cam.connected = false;
        cam
    }

    /// Number of capture attempts so far (successful or not).
    pub fn attempts(&self) -> u32 {
        *self.attempts.lock().unwrap()
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

                let is_aurora_frame = match self.pattern {
                    SkyPattern::Intermittent => index % 5 >= 3,
                    SkyPattern::Dark => false,
                    SkyPattern::Aurora => true,
                };

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
        *self.attempts.lock().unwrap() += 1;
        if !self.connected {
            return Err(CameraError::NotConnected);
        }
        {
            let mut left = self.failures_left.lock().unwrap();
            if *left > 0 {
                *left -= 1;
                return Err(CameraError::CaptureFailed("injected failure".into()));
            }
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
            let frame = self.generate_synthetic_frame(exposure, index);
            Ok(if self.jpeg_output { self.to_pi_capture(frame, index) } else { frame })
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
            raw_bytes: raw.raw_bytes.clone(),
        };
        Ok((raw, jpg))
    }

    fn is_connected(&self) -> bool {
        self.connected
    }
}
