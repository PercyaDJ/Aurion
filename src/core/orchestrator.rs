use std::path::Path;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};

use crate::core::config::AppConfig;
use crate::core::detection::AuroraDetector;
use crate::core::exposure::{ExposureController, compute_histogram};
use crate::core::models::{CaptureFrame, OutputFormat, Phase, SessionEvent, TimeRange};
use chrono::Datelike;
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

        // ─── Log system clock immediately (critical for diagnosis without screen) ─
        {
            let sys_now = chrono::Local::now();
            let clock_msg = format!("Heure systeme au demarrage: {}", sys_now.format("%Y-%m-%d %H:%M:%S"));
            info!("Orchestrator: {}", clock_msg);
            self.log(&clock_msg).await;

            // Warn if the clock looks like epoch (Pi without NTP)
            if sys_now.year() < 2024 {
                let warn_msg = "[!] Horloge systeme < 2024 - NTP non synchronise ? La plage horaire peut ne pas fonctionner.";
                warn!("Orchestrator: {}", warn_msg);
                self.log(warn_msg).await;
            }
        }

        // ─── Wait for time range to start ───────────────────
        if config.time_range.duration_hours.is_none() {
            let time_range = TimeRange { start: config.time_range.start, end: config.time_range.end };
            let mut logged_wait = false;
            loop {
                if self.is_shutdown().await {
                    return Ok(());
                }
                let now = chrono::Local::now();
                if time_range.contains(now.time()) {
                    break;
                }
                if !logged_wait {
                    let msg = format!(
                        "Attente plage horaire ({} -> {}) | heure actuelle: {}",
                        config.time_range.start, config.time_range.end,
                        now.format("%H:%M:%S")
                    );
                    info!("Orchestrator: {}", msg);
                    self.log(&msg).await;
                    logged_wait = true;
                }
                sleep(Duration::from_secs(60)).await;
            }
            let start_msg = format!("Plage horaire atteinte - demarrage capture ({} -> {})",
                config.time_range.start, config.time_range.end);
            self.log(&start_msg).await;
            info!("Orchestrator: {}", start_msg);
        }

        self.set_phase(Phase::Calibration).await;
        self.log("Phase: CALIBRATION").await;

        // ─── Initialize core components ─────────────────────
        let mut sm = StateMachine::new(config.detection.consecutive_required);
        sm.set_phase(Phase::Calibration);

        let mut exposure_ctrl = ExposureController::from_config(&config.exposure);
        let mut detector = AuroraDetector::from_config(&config.detection);

        // ─── USB storage: write-first check with auto-mount ─
        // Don't rely on mountpoint -q (can lie). Try a real write directly.
        // If it fails: attempt to (re)mount, then retry.
        let configured_path = std::path::PathBuf::from(&config.storage.mount_point);
        {
            let probe_path = configured_path.join(".aurion_probe");
            let mut storage_ok = false;

            // First attempt: direct write (USB already mounted via fstab/boot)
            if std::fs::create_dir_all(&configured_path).is_ok() {
                if std::fs::write(&probe_path, b"ok").is_ok() {
                    let _ = std::fs::remove_file(&probe_path);
                    self.log(&format!("USB accessible: {}", config.storage.mount_point)).await;
                    storage_ok = true;
                }
            }

            // Second attempt: try to (re)mount then retry
            if !storage_ok {
                warn!("Orchestrator: direct write failed, attempting mount...");
                self.log("USB: montage en cours...").await;
                let _ = self.storage.mount().await; // ignore if already mounted
                sleep(Duration::from_secs(3)).await;
                if std::fs::create_dir_all(&configured_path).is_ok() {
                    if std::fs::write(&probe_path, b"ok").is_ok() {
                        let _ = std::fs::remove_file(&probe_path);
                        self.log(&format!("USB monte et accessible: {}", config.storage.mount_point)).await;
                        storage_ok = true;
                    }
                }
            }

            if !storage_ok {
                // Log a clear warning — session continues and will try to write anyway
                // (write may still succeed if permissions allow)
                let msg = format!(
                    "[!] USB ({}) non accessible en ecriture - la session va tenter de continuer",
                    config.storage.mount_point
                );
                warn!("Orchestrator: {}", msg);
                self.log(&msg).await;
            }
        }
        let storage_path: &Path = configured_path.as_path();

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
        // Dump effective config to session.log so we can verify from SSH next morning
        if let Some(ref mut sl) = session_logger {
            sl.log_text(&format!("=== CONFIG EFFECTIVE ==="));
            sl.log_text(&format!("Mode: {}", capture_mode_str));
            sl.log_text(&format!("Format: {:?}", config.capture.output_format));
            sl.log_text(&format!("Intervalle: {}s", config.capture.capture_interval_secs));
            sl.log_text(&format!("Plage: {} -> {}", config.time_range.start, config.time_range.end));
            sl.log_text(&format!("Minuteur: {:?}h", config.time_range.duration_hours));
            sl.log_text(&format!("detection_capture_enabled: {}", config.detection.detection_capture_enabled));
            sl.log_text(&format!("Mount point: {}", config.storage.mount_point));
            sl.log_text(&format!("======================="));
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
        let mut has_started_range = false;
        let mut wait_iters = 0u64;

        loop {
            // Check for external shutdown signal
            if self.is_shutdown().await {
                info!("Orchestrator: shutdown signal received");
                break;
            }

            // Check time limit
            let now = chrono::Local::now();
            let mut should_stop = false;
            let mut wait_for_start = false;

            if let Some(_dl) = deadline {
                let elapsed = now.signed_duration_since(loop_start);
                let max_duration = config.time_range.duration_hours.unwrap_or(0.0);
                should_stop = elapsed.num_seconds() >= (max_duration * 3600.0) as i64;
            } else {
                let time_range = TimeRange {
                    start: config.time_range.start,
                    end: config.time_range.end,
                };
                if time_range.contains(now.time()) {
                    has_started_range = true;
                } else {
                    if has_started_range {
                        // We were in the range, and now we exited -> STOP
                        should_stop = true;
                    } else {
                        // Not in range, and haven't started yet -> WAIT
                        wait_for_start = true;
                    }
                }
            };

            if wait_for_start {
                if wait_iters % 30 == 0 {
                    let wait_msg = format!(
                        "Attente du début de plage (actuel: {}, début: {})",
                        now.format("%H:%M:%S"), config.time_range.start
                    );
                    info!("Orchestrator: {}", wait_msg);
                    self.log(&wait_msg).await;
                    if let Some(ref mut sl) = session_logger { sl.log_text(&wait_msg); }
                }
                wait_iters += 1;
                sleep(Duration::from_secs(10)).await;
                continue;
            }

            // Log each loop iteration so we can trace from session.log
            {
                let iter_msg = format!(
                    "[loop] heure={} phase={:?} should_stop={}",
                    now.format("%H:%M:%S"), sm.phase(), should_stop
                );
                info!("Orchestrator: {}", iter_msg);
                if let Some(ref mut sl) = session_logger { sl.log_text(&iter_msg); }
            }

            if should_stop {
                let stop_msg = format!("Heure de fin de plage atteinte ({}) — arret capture", now.format("%H:%M:%S"));
                info!("Orchestrator: {}", stop_msg);
                self.log(&stop_msg).await;
                if let Some(ref mut sl) = session_logger { sl.log_text(&stop_msg); }
                break;
            }

            // Check storage — log result, don't silently abort
            if let Ok(info) = self.storage.info() {
                let status = info.status(
                    config.storage.warning_percent as f64,
                    config.storage.critical_percent as f64,
                );
                let storage_msg = format!("[storage] libre={:.1}% statut={:?}", info.free_percent(), status);
                if let Some(ref mut sl) = session_logger { sl.log_text(&storage_msg); }
                if status == crate::core::models::StorageStatus::Critical {
                    let msg = "Stockage critique — arret session";
                    self.log(msg).await;
                    if let Some(ref mut sl) = session_logger { sl.log_text(msg); }
                    break;
                }
            } else {
                // df failed — USB may not be mounted, log it
                let msg = "[storage] df echoue - USB inaccessible ?";
                if let Some(ref mut sl) = session_logger { sl.log_text(msg); }
                warn!("Orchestrator: {}", msg);
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
                                let fail_msg = format!("CAPTURE ECHEC JPG: {} ({}/5)", e, consecutive_io_errors);
                                error!("Orchestrator: {}", fail_msg);
                                self.log(&format!("Capture echouee: {} ({}/5)", e, consecutive_io_errors)).await;
                                if let Some(ref mut sl) = session_logger { sl.log_text(&fail_msg); }
                                if consecutive_io_errors >= 5 {
                                    let rec_msg = "5 erreurs consecutives - pause 5min";
                                    self.log(rec_msg).await;
                                    if let Some(ref mut sl) = session_logger { sl.log_text(rec_msg); }
                                    warn!("Orchestrator: 5 consecutive camera errors → pausing 5min for auto-recovery");
                                    sleep(Duration::from_secs(300)).await;
                                    consecutive_io_errors = 0;
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
                                let fail_msg = format!("CAPTURE ECHEC RAW: {} ({}/5)", e, consecutive_io_errors);
                                error!("Orchestrator: {}", fail_msg);
                                self.log(&format!("Capture RAW echouee: {} ({}/5)", e, consecutive_io_errors)).await;
                                if let Some(ref mut sl) = session_logger { sl.log_text(&fail_msg); }
                                if consecutive_io_errors >= 5 {
                                    let rec_msg = "5 erreurs RAW consecutives - pause 5min";
                                    self.log(rec_msg).await;
                                    if let Some(ref mut sl) = session_logger { sl.log_text(rec_msg); }
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
        info!("Orchestrator: save_frame frame={} format={:?} aurora={}", frame_num, config.capture.output_format, is_aurora);

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
                    let dng_bytes = if !raw.raw_bytes.is_empty() { &raw.raw_bytes } else { &raw.data };
                    if let Err(e) = self.storage.save_file(&filename, dng_bytes).await {
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
                    let dng_bytes = if !raw.raw_bytes.is_empty() { &raw.raw_bytes } else { &raw.data };
                    if let Err(e) = self.storage.save_file(&format!("{}.dng", base), dng_bytes).await {
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
