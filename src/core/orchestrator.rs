use std::path::Path;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};

use crate::core::config::AppConfig;
use crate::core::detection::AuroraDetector;
use crate::core::exposure::{ExposureController, compute_histogram};
use crate::core::models::{CaptureFrame, OutputFormat, Phase, SessionEvent, TimeRange};
use crate::core::session_logger::SessionLogger;
use crate::core::state_machine::StateMachine;
use crate::ports::camera::CameraPort;
use crate::ports::storage::StoragePort;
use crate::ports::system::SystemPort;
use crate::web::AppState;

/// Orchestrator — autonomous night capture loop.
///
/// Runs as a background task alongside the web server.
/// Watches `AppState.phase` for transitions triggered by the UI (Disconnect).
/// Drives the full lifecycle: Calibration → Watch/Run → Shutdown.
pub struct Orchestrator<C: CameraPort, S: StoragePort, Sys: SystemPort> {
    state: AppState,
    camera: C,
    storage: S,
    system: Sys,
}

impl<C: CameraPort, S: StoragePort, Sys: SystemPort> Orchestrator<C, S, Sys> {
    pub fn new(state: AppState, camera: C, storage: S, system: Sys) -> Self {
        Self { state, camera, storage, system }
    }

    /// Main entry point — run the full autonomous loop.
    /// This blocks until shutdown.
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Orchestrator: starting autonomous loop");

        // ─── ARM phase: wait for user to disconnect ─────────
        self.wait_for_disconnect().await;

        // ─── DISCONNECT timer ───────────────────────────────
        info!("Orchestrator: disconnect requested, 15s timer...");
        self.set_phase(Phase::Disconnect).await;
        sleep(Duration::from_secs(15)).await;

        // ─── Stop AP (best-effort) ──────────────────────────
        #[cfg(feature = "rpi")]
        {
            use crate::ports::network::NetworkApPort;
            let network = crate::adapters::rpi::NetworkRpi::new();
            if let Err(e) = network.stop_ap().await {
                warn!("Orchestrator: failed to stop AP: {}", e);
            }
        }

        // ─── Load config snapshot ───────────────────────────
        let config = self.state.config.read().await.clone();

        // ─── Wait for time range to start ───────────────────
        if config.time_range.duration_hours.is_none() {
            let time_range = TimeRange { start: config.time_range.start, end: config.time_range.end };
            let mut logged_wait = false;
            loop {
                if self.is_shutdown().await {
                    return Ok(());
                }
                let now = chrono::Local::now().time();
                if time_range.contains(now) {
                    break;
                }
                if !logged_wait {
                    let msg = format!("Attente du début de la plage horaire ({})", config.time_range.start);
                    info!("Orchestrator: {}", msg);
                    self.log(&msg).await;
                    logged_wait = true;
                }
                sleep(Duration::from_secs(60)).await;
            }
        }

        self.set_phase(Phase::Calibration).await;
        self.log("Phase: CALIBRATION").await;

        // ─── Initialize core components ─────────────────────
        let mut sm = StateMachine::new(config.detection.consecutive_required);
        sm.set_phase(Phase::Calibration);

        let mut exposure_ctrl = ExposureController::from_config(&config.exposure);
        let mut detector = AuroraDetector::from_config(&config.detection);

        let storage_path = Path::new(&config.storage.mount_point);

        // ─── USB storage pre-check ──────────────────────────
        // Before starting the session, verify USB is mounted and writable.
        // Retry for up to 60s to handle slow USB enumeration at boot.
        {
            let mut usb_ok = false;
            for attempt in 1..=12 {
                if self.storage.is_available() {
                    // Try a test write to confirm write access
                    let test_probe = b"aurion_probe";
                    match self.storage.save_file(".write_probe", test_probe).await {
                        Ok(_) => {
                            let _ = std::fs::remove_file(storage_path.join(".write_probe"));
                            self.log("✅ Clé USB montée et accessible en écriture").await;
                            usb_ok = true;
                            break;
                        }
                        Err(e) => {
                            warn!("Orchestrator: USB write test failed (try {}/12): {}", attempt, e);
                        }
                    }
                } else {
                    warn!("Orchestrator: USB not available (try {}/12)", attempt);
                }
                sleep(Duration::from_secs(5)).await;
            }
            if !usb_ok {
                let msg = "❌ Clé USB inaccessible après 60s — session abandonnée";
                error!("Orchestrator: {}", msg);
                self.log(msg).await;
                self.set_phase(Phase::Shutdown).await;
                return Err(anyhow::anyhow!("USB storage unavailable — cannot start session"));
            }
        }

        let mut session_logger = match SessionLogger::new(storage_path) {
            Ok(l) => {
                info!("Orchestrator: session logger at {:?}", l.session_path());
                Some(l)
            }
            Err(e) => {
                warn!("Orchestrator: cannot create session logger: {} — continuing without", e);
                None
            }
        };

        let is_safe_mode = !config.detection.detection_capture_enabled;
        let capture_mode_str = if is_safe_mode { "SAFE" } else { "FILTER" };
        self.log(&format!("Mode: {}", capture_mode_str)).await;
        if let Some(ref mut sl) = session_logger {
            sl.log_text(&format!("Mode capture: {}", capture_mode_str));
            sl.log_text(&format!("Format: {:?} | Intervalle: {}s | ROI top: {}%",
                config.capture.output_format, config.capture.capture_interval_secs, config.detection.roi_top_percent));
        }

        // ─── CALIBRATION: 3 frames to stabilize exposure ────
        // Use /tmp for calibration captures — USB may be slow at startup
        // and calibration frames are never saved to disk.
        let tmp_path = Path::new("/tmp");
        info!("Orchestrator: calibrating exposure (3 frames)...");
        for i in 0..3 {
            match self.camera.capture_jpg(&exposure_ctrl.current(), tmp_path).await {
                Ok(frame) => {
                    let roi_data = crop_roi(&frame.data, frame.width, frame.height, config.detection.roi_top_percent);
                    let hist = compute_histogram(roi_data);
                    let settings = exposure_ctrl.update(&hist, Phase::Calibration);
                    info!(
                        "Orchestrator: calibration {}/3 → ISO {} / {}µs",
                        i + 1, settings.iso, settings.shutter_us
                    );
                }
                Err(e) => {
                    warn!("Orchestrator: calibration capture failed: {}", e);
                    sleep(Duration::from_secs(2)).await;
                }
            }
        }

        let settings = exposure_ctrl.current();
        let calib_msg = format!("Calibration done: ISO {} / {}µs", settings.iso, settings.shutter_us);
        self.log(&calib_msg).await;
        if let Some(ref mut sl) = session_logger { sl.log_text(&calib_msg); }

        // ─── Transition based on mode ───────────────────────
        if is_safe_mode {
            info!("Orchestrator: SAFE mode → Run directly");
            sm.set_phase(Phase::Run);
            self.set_phase(Phase::Run).await;
            self.log("Phase: RUN (SAFE mode — capture toute la nuit)").await;
            if let Some(ref mut sl) = session_logger { sl.log_text("Phase: RUN (SAFE mode)"); }
        } else {
            info!("Orchestrator: FILTER mode → Watch");
            sm.set_phase(Phase::Watch);
            self.set_phase(Phase::Watch).await;
            self.log("Phase: WATCH (FILTER mode — attente détection)").await;
            if let Some(ref mut sl) = session_logger { sl.log_text("Phase: WATCH (FILTER mode)"); }
        }

        // ─── Compute deadline (timer vs time-range mode) ────
        let deadline: Option<chrono::NaiveTime> = if let Some(hours) = config.time_range.duration_hours {
            // Timer mode: run for N hours from now
            let secs = (hours * 3600.0) as i64;
            let end = chrono::Local::now() + chrono::Duration::seconds(secs);
            let msg = format!("Mode minuteur: {}h → fin prévue à {}", hours, end.format("%H:%M:%S"));
            self.log(&msg).await;
            if let Some(ref mut sl) = session_logger { sl.log_text(&msg); }
            Some(end.time())
        } else {
            // Time range mode: use configured end time
            let msg = format!("Mode plage horaire: {} → {}", config.time_range.start, config.time_range.end);
            self.log(&msg).await;
            if let Some(ref mut sl) = session_logger { sl.log_text(&msg); }
            None
        };

        // ─── Main capture loop ──────────────────────────────
        let mut consecutive_detections = 0u32;
        let mut frame_number = 0u64;
        let mut consecutive_io_errors = 0u32;
        let loop_start = chrono::Local::now();

        loop {
            // Check for external shutdown signal
            if self.is_shutdown().await {
                info!("Orchestrator: shutdown signal received");
                break;
            }

            // Check time limit
            let now = chrono::Local::now();
            let should_stop = if let Some(_dl) = deadline {
                // Timer mode: check elapsed duration
                let elapsed = now.signed_duration_since(loop_start);
                let max_duration = config.time_range.duration_hours.unwrap_or(0.0);
                elapsed.num_seconds() >= (max_duration * 3600.0) as i64
            } else {
                // Time range mode
                let time_range = TimeRange {
                    start: config.time_range.start,
                    end: config.time_range.end,
                };
                !time_range.contains(now.time())
            };
            if should_stop {
                info!("Orchestrator: time limit reached at {}", now.format("%H:%M:%S"));
                break;
            }

            // Check storage
            if let Ok(info) = self.storage.info() {
                let status = info.status(
                    config.storage.warning_percent as f64,
                    config.storage.critical_percent as f64,
                );
                if status == crate::core::models::StorageStatus::Critical {
                    warn!("Orchestrator: storage critical! Entering safe mode");
                    self.log("⚠️ Stockage critique — arrêt").await;
                    break;
                }
            }

            let current_phase = sm.phase();
            let exposure = exposure_ctrl.current();

            // ─── Live config (hot-reload of mutable params) ─────
            // Re-read only the parameters that are safe to change mid-session:
            //   - capture_interval_secs / watch_interval_secs
            //   - detection thresholds (sensitivity, ROI, etc.)
            // Immutable params (mount_point, output_format, time_range) use the snapshot.
            let live_cfg = self.state.config.read().await.clone();
            detector.update_config(&live_cfg.detection);
            let live_capture_interval = live_cfg.capture.capture_interval_secs.max(1);
            let live_watch_interval = live_cfg.capture.watch_interval_secs.max(5);
            drop(live_cfg);

            // ─── Capture frame (format-aware, single capture) ──
            // In RAW mode: capture_raw_and_jpg() returns (raw_frame, jpg_frame)
            //   → jpg_frame used for analysis (exposure + detection)
            //   → raw_frame saved to storage (no second capture)
            // In JPG mode: capture_jpg() for analysis + save
            // Camera always writes temp files to /tmp, not the USB drive.
            let (analysis_frame, raw_frame_opt): (CaptureFrame, Option<CaptureFrame>) =
                match config.capture.output_format {
                    OutputFormat::Jpg => {
                        match self.camera.capture_jpg(&exposure, Path::new("/tmp")).await {
                            Ok(f) => { consecutive_io_errors = 0; (f, None) }
                            Err(e) => {
                                consecutive_io_errors += 1;
                                error!("Orchestrator: capture failed: {} ({}/5)", e, consecutive_io_errors);
                                self.log(&format!("❌ Capture échouée: {} ({}/5)", e, consecutive_io_errors)).await;
                                if consecutive_io_errors >= 5 {
                                    // Don't crash — wait 5 minutes and auto-recover
                                    // (camera may have briefly disconnected)
                                    self.log("⚠️ 5 erreurs consécutives — pause 5 min, tentative de récupération...").await;
                                    warn!("Orchestrator: 5 consecutive camera errors → pausing 5min for auto-recovery");
                                    sleep(Duration::from_secs(300)).await;
                                    consecutive_io_errors = 0; // reset: try again
                                }
                                sleep(Duration::from_secs(5)).await;
                                continue;
                            }
                        }
                    }
                    OutputFormat::RawDng | OutputFormat::RawAndJpg => {
                        // Single capture: returns (dng_frame, jpg_frame)
                        match self.camera.capture_raw_and_jpg(&exposure, Path::new("/tmp")).await {
                            Ok((raw, jpg)) => { consecutive_io_errors = 0; (jpg, Some(raw)) }
                            Err(e) => {
                                consecutive_io_errors += 1;
                                error!("Orchestrator: RAW capture failed: {} ({}/5)", e, consecutive_io_errors);
                                self.log(&format!("❌ Capture RAW échouée: {} ({}/5)", e, consecutive_io_errors)).await;
                                if consecutive_io_errors >= 5 {
                                    self.log("⚠️ 5 erreurs consécutives — pause 5 min, tentative de récupération...").await;
                                    warn!("Orchestrator: 5 consecutive camera errors → pausing 5min for auto-recovery");
                                    sleep(Duration::from_secs(300)).await;
                                    consecutive_io_errors = 0;
                                }
                                sleep(Duration::from_secs(5)).await;
                                continue;
                            }
                        }
                    }
                };
            let frame = &analysis_frame;

            // ROI crop
            let roi_data = crop_roi(&frame.data, frame.width, frame.height, config.detection.roi_top_percent);
            let roi_height = (frame.height as f64 * config.detection.roi_top_percent as f64 / 100.0) as u32;

            // ─── Exposure update ────────────────────────────
            let hist = compute_histogram(roi_data);
            exposure_ctrl.update(&hist, current_phase);

            // ─── Detection ──────────────────────────────────
            let det_result = detector.analyze(roi_data, frame.width, roi_height);

            // ─── Session logging ────────────────────────────
            if let Some(ref mut logger) = session_logger {
                let event = SessionEvent {
                    timestamp: chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                    phase: format!("{:?}", current_phase),
                    capture_mode: capture_mode_str.to_string(),
                    exposure_us: exposure.shutter_us,
                    iso: exposure.iso,
                    format: format!("{:?}", config.capture.output_format),
                    roi_excluded_percent: 100 - config.detection.roi_top_percent,
                    aurora_score: det_result.aurora_score,
                    aurora_detected: det_result.detected,
                    aurora_color: det_result.aurora_color.to_string(),
                    consecutive_hits: consecutive_detections,
                    moon_mask_active: det_result.moon_masked,
                };
                if let Err(e) = logger.log_event(&event) {
                    warn!("Orchestrator: log event failed: {}", e);
                }
            }

            // ─── Phase-specific logic ───────────────────────
            match current_phase {
                Phase::Watch => {
                    // FILTER mode: waiting for aurora
                    if det_result.detected {
                        consecutive_detections += 1;
                        info!(
                            "Orchestrator: aurora detected ({}/{}) score={:.2} color={}",
                            consecutive_detections, config.detection.consecutive_required,
                            det_result.aurora_score, det_result.aurora_color
                        );
                        if consecutive_detections >= config.detection.consecutive_required {
                            info!("Orchestrator: aurora CONFIRMED → RUN");
                            sm.set_phase(Phase::Run);
                            self.set_phase(Phase::Run).await;
                            self.log("🌌 Aurore confirmée → capture intensive!").await;
                        }
                    } else {
                        consecutive_detections = 0;
                    }

                    // Watch interval (hot-reloadable)
                    sleep(Duration::from_secs(live_watch_interval as u64)).await;
                }

                Phase::Run => {
                    // Save the captured frame (NO double capture — raw_frame_opt already holds the DNG)
                    self.save_frame(&config, &analysis_frame, raw_frame_opt.as_ref(), frame_number, det_result.detected).await;
                    frame_number += 1;

                    // In FILTER mode during Run, if detection drops we keep capturing
                    // (conservative: don't stop on momentary gaps)

                    // Wait for configured capture interval (hot-reloadable timelapse pacing)
                    sleep(Duration::from_secs(live_capture_interval as u64)).await;
                }

                _ => {
                    // Should not happen but handle gracefully
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }

        // ─── SHUTDOWN ───────────────────────────────────────
        info!("Orchestrator: entering shutdown sequence");
        self.set_phase(Phase::Shutdown).await;
        self.log("Phase: SHUTDOWN").await;

        // Sync and unmount storage
        if let Err(e) = self.storage.sync().await {
            warn!("Orchestrator: sync failed: {}", e);
        }

        info!("Orchestrator: shutdown complete");

        // Power off the system
        if let Err(e) = self.system.shutdown().await {
            error!("Orchestrator: system shutdown failed: {}", e);
        }

        Ok(())
    }

    /// Save a captured frame to storage.
    /// `analysis_frame` is the JPG frame used for analysis (always present).
    /// `raw_frame` is the pre-captured DNG (Some for RawDng/RawAndJpg, None for JPG mode).
    /// `is_aurora` controls whether `_AURORA` is appended to the filename.
    async fn save_frame(
        &self,
        config: &AppConfig,
        analysis_frame: &CaptureFrame,
        raw_frame: Option<&CaptureFrame>,
        frame_num: u64,
        is_aurora: bool,
    ) {

        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
        let suffix = if is_aurora { "_AURORA" } else { "" };

        match config.capture.output_format {
            OutputFormat::Jpg => {
                let filename = format!("aurora_{}_{:05}{}.jpg", timestamp, frame_num, suffix);
                // Prefer raw_bytes (original JPEG from rpicam-still) to avoid re-encoding.
                // Fall back to RGB re-encode only for mock/test frames where raw_bytes is empty.
                let jpg_bytes: Vec<u8> = if !analysis_frame.raw_bytes.is_empty() {
                    analysis_frame.raw_bytes.clone()
                } else {
                    self.encode_rgb_as_jpg(&analysis_frame.data, analysis_frame.width, analysis_frame.height)
                        .unwrap_or_default()
                };
                if jpg_bytes.is_empty() {
                    error!("Orchestrator: no JPEG data to save for frame {}", frame_num);
                } else if let Err(e) = self.storage.save_file(&filename, &jpg_bytes).await {
                    error!("Orchestrator: save JPG failed: {}", e);
                } else {
                    self.generate_thumbnail(&analysis_frame.data, analysis_frame.width, analysis_frame.height, &filename).await;
                    info!("Orchestrator: saved {}", filename);
                }

            }
            OutputFormat::RawDng => {
                // Use the already-captured raw frame (no re-capture)
                let filename = format!("aurora_{}_{:05}{}.dng", timestamp, frame_num, suffix);
                if let Some(raw) = raw_frame {
                    if let Err(e) = self.storage.save_file(&filename, &raw.data).await {
                        error!("Orchestrator: save RAW failed: {}", e);
                    } else {
                        info!("Orchestrator: saved {}", filename);
                    }
                } else {
                    error!("Orchestrator: no raw frame available for RawDng format — skipping");
                }
            }
            OutputFormat::RawAndJpg => {
                let base = format!("aurora_{}_{:05}{}", timestamp, frame_num, suffix);
                // Save the analysis frame as JPG (no re-capture needed)
                let jpg_filename = format!("{}.jpg", base);
                if let Err(e) = self.save_rgb_as_jpg(&analysis_frame.data, analysis_frame.width, analysis_frame.height, &jpg_filename).await {
                    error!("Orchestrator: save JPG failed: {}", e);
                } else {
                    self.generate_thumbnail(&analysis_frame.data, analysis_frame.width, analysis_frame.height, &jpg_filename).await;
                }
                // Save pre-captured DNG (no re-capture)
                if let Some(raw) = raw_frame {
                    if let Err(e) = self.storage.save_file(&format!("{}.dng", base), &raw.data).await {
                        error!("Orchestrator: save RAW failed: {}", e);
                    }
                } else {
                    error!("Orchestrator: no raw frame for RawAndJpg format");
                }
                info!("Orchestrator: saved {}.jpg + .dng", base);
            }
        }
    }

    /// Encode RGB data as JPEG and save to storage (fallback only).
    async fn save_rgb_as_jpg(&self, rgb_data: &[u8], width: u32, height: u32, filename: &str) -> anyhow::Result<()> {
        let bytes = self.encode_rgb_as_jpg(rgb_data, width, height)?;
        self.storage.save_file(filename, &bytes).await
            .map_err(|e| anyhow::anyhow!("{}", e))
    }

    /// Encode RGB pixels to JPEG bytes.
    fn encode_rgb_as_jpg(&self, rgb_data: &[u8], width: u32, height: u32) -> anyhow::Result<Vec<u8>> {
        use image::{ImageBuffer, Rgb};
        let img: ImageBuffer<Rgb<u8>, Vec<u8>> = ImageBuffer::from_raw(width, height, rgb_data.to_vec())
            .ok_or_else(|| anyhow::anyhow!("Invalid image dimensions"))?;
        let mut buf = std::io::Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Jpeg)?;
        Ok(buf.into_inner())
    }

    /// Generate a 320×240 thumbnail and save to thumbs/ subdirectory.
    async fn generate_thumbnail(&self, rgb_data: &[u8], width: u32, height: u32, filename: &str) {
        use image::{ImageBuffer, Rgb};
        if let Some(img) = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(width, height, rgb_data.to_vec()) {
            let thumb = image::imageops::resize(&img, 320, 240, image::imageops::FilterType::Triangle);
            let mut buf = std::io::Cursor::new(Vec::new());
            if thumb.write_to(&mut buf, image::ImageFormat::Jpeg).is_ok() {
                let thumb_name = format!("thumbs/{}", filename);
                let _ = self.storage.save_file(&thumb_name, buf.get_ref()).await;
            }
        }
    }

    /// Wait for the user to trigger disconnect from the UI.
    async fn wait_for_disconnect(&self) {
        info!("Orchestrator: ARM phase — waiting for user disconnect...");
        loop {
            let phase = *self.state.phase.read().await;
            if phase == Phase::Disconnect || phase == Phase::Shutdown {
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }
    }

    /// Set the shared phase (visible to web UI).
    async fn set_phase(&self, phase: Phase) {
        let mut p = self.state.phase.write().await;
        *p = phase;
    }

    /// Check if shutdown was requested externally.
    async fn is_shutdown(&self) -> bool {
        *self.state.phase.read().await == Phase::Shutdown
    }

    /// Add a log message to the shared buffer (visible in web UI).
    async fn log(&self, msg: &str) {
        info!("Orchestrator: {}", msg);
        self.state.add_log(msg.to_string()).await;
    }
}

/// Crop image data to ROI (top N% of the image).
/// Returns a slice with only the ROI pixel data (RGB).
pub fn crop_roi(rgb_data: &[u8], width: u32, height: u32, roi_top_percent: u32) -> &[u8] {
    let roi_height = (height as f64 * roi_top_percent as f64 / 100.0) as u32;
    let roi_bytes = (width * roi_height * 3) as usize;
    if roi_bytes <= rgb_data.len() {
        &rgb_data[..roi_bytes]
    } else {
        rgb_data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crop_roi_65_percent() {
        let w = 100u32;
        let h = 100u32;
        let data = vec![128u8; (w * h * 3) as usize];
        let cropped = crop_roi(&data, w, h, 65);
        assert_eq!(cropped.len(), (w * 65 * 3) as usize);
    }

    #[test]
    fn test_crop_roi_100_percent() {
        let w = 10u32;
        let h = 10u32;
        let data = vec![0u8; (w * h * 3) as usize];
        let cropped = crop_roi(&data, w, h, 100);
        assert_eq!(cropped.len(), data.len());
    }

    #[test]
    fn test_crop_roi_small_image() {
        let data = vec![1u8; 30]; // 10 pixels (too small for 100x100)
        let cropped = crop_roi(&data, 100, 100, 50);
        // Should return full data since crop would exceed bounds
        assert_eq!(cropped.len(), 30);
    }
}
