use std::path::Path;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};

use crate::core::config::{AppConfig, DenoiseConfig};
use crate::core::denoise::Stacker;
use crate::core::detection::AuroraDetector;
use crate::core::exposure::{ExposureController, compute_histogram};
use crate::core::models::{CaptureFrame, OutputFormat, Phase, SessionEvent, TimeRange};
use chrono::Datelike;
use crate::core::night::{NightMarker, ResumePlan, MAX_RESUMES, RESUME_DELAY_SECS};
use crate::core::session_logger::SessionLogger;
use crate::core::state_machine::StateMachine;
use crate::ports::camera::CameraPort;
use crate::ports::clock::ClockPort;
use crate::ports::network::NetworkApPort;
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
    clock: Arc<dyn ClockPort>,
    network: Option<Box<dyn NetworkApPort>>,
}

/// Delay between the "Déconnexion" click and the hotspot shutdown, so the
/// phone receives the answer and the user can walk away.
const DISCONNECT_DELAY: Duration = Duration::from_secs(15);

impl<C: CameraPort, S: StoragePort, Sys: SystemPort> Orchestrator<C, S, Sys> {
    pub fn new(state: AppState, camera: C, storage: S, system: Sys) -> Self {
        Self {
            state,
            camera,
            storage,
            system,
            clock: Arc::new(crate::adapters::pc::ClockReal::new()),
            network: None,
        }
    }

    /// Use another clock (tests / simulation with virtual time).
    pub fn with_clock(mut self, clock: Arc<dyn ClockPort>) -> Self {
        self.clock = clock;
        self
    }

    /// Hotspot to switch off when the night starts.
    pub fn with_network(mut self, network: Box<dyn NetworkApPort>) -> Self {
        self.network = Some(network);
        self
    }

    fn now(&self) -> chrono::DateTime<chrono::Local> {
        self.clock.now_local()
    }

    /// Main entry point — run the full autonomous loop.
    /// This blocks until shutdown.
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Orchestrator: starting autonomous loop");

        // ─── Interrupted night (power cut): resume on its own ─
        let resume = self.resume_window().await;

        // ─── ARM phase: wait for user to disconnect ─────────
        if resume.is_none() {
            self.wait_for_disconnect().await;
        }

        // ─── DISCONNECT timer ───────────────────────────────
        if self.is_shutdown().await {
            return Ok(());
        }
        info!("Orchestrator: disconnect requested, 15s timer...");
        self.set_phase(Phase::Disconnect).await;
        sleep(DISCONNECT_DELAY).await;

        // ─── Stop AP (best-effort) ──────────────────────────
        if let Some(ref network) = self.network {
            if let Err(e) = network.stop_ap().await {
                warn!("Orchestrator: failed to stop AP: {}", e);
            }
        }

        // ─── Load config snapshot ───────────────────────────
        let mut config: AppConfig = self.state.config.read().await.clone();

        // ─── Night marker (resume after a power cut) ────────
        let marker_path = self.state.paths.night_marker.clone();
        match resume {
            Some((marker, plan)) => {
                if let ResumePlan::Timer(hours) = plan {
                    config.time_range.duration_hours = Some(hours);
                }
                if let Err(e) = marker.save(&marker_path) {
                    warn!("Orchestrator: cannot update night marker: {}", e);
                }
                self.log(&format!("Reprise de la nuit interrompue ({}/{})", marker.resumes, MAX_RESUMES)).await;
            }
            None => {
                if let Err(e) = NightMarker::new(self.now(), config.time_range.duration_hours).save(&marker_path) {
                    warn!("Orchestrator: cannot write night marker: {}", e);
                }
            }
        }

        // ─── Log system clock immediately (critical for diagnosis without screen) ─
        {
            let sys_now = self.now();
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
                let now = self.now();
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
            if std::fs::create_dir_all(&configured_path).is_ok()
                && std::fs::write(&probe_path, b"ok").is_ok() {
                    let _ = std::fs::remove_file(&probe_path);
                    self.log(&format!("USB accessible: {}", config.storage.mount_point)).await;
                    storage_ok = true;
                }

            // Second attempt: try to (re)mount then retry
            if !storage_ok {
                warn!("Orchestrator: direct write failed, attempting mount...");
                self.log("USB: montage en cours...").await;
                let _ = self.storage.mount().await; // ignore if already mounted
                sleep(Duration::from_secs(3)).await;
                if std::fs::create_dir_all(&configured_path).is_ok()
                    && std::fs::write(&probe_path, b"ok").is_ok() {
                        let _ = std::fs::remove_file(&probe_path);
                        self.log(&format!("USB monte et accessible: {}", config.storage.mount_point)).await;
                        storage_ok = true;
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

        let mut session_logger = match SessionLogger::new_at(storage_path, self.now()) {
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
            sl.log_text("=== CONFIG EFFECTIVE ===");
            sl.log_text(&format!("Mode: {}", capture_mode_str));
            sl.log_text(&format!("Format: {:?}", config.capture.output_format));
            sl.log_text(&format!("Intervalle: {}s", config.capture.capture_interval_secs));
            sl.log_text(&format!("Plage: {} -> {}", config.time_range.start, config.time_range.end));
            sl.log_text(&format!("Minuteur: {:?}h", config.time_range.duration_hours));
            sl.log_text(&format!("detection_capture_enabled: {}", config.detection.detection_capture_enabled));
            sl.log_text(&format!("Mount point: {}", config.storage.mount_point));
            sl.log_text("=======================");
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
            let end = self.now() + chrono::Duration::seconds(secs);
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
        let mut stacker: Option<Stacker> = None;
        let mut iterations = 0u64;
        let mut frame_number = 0u64;
        let mut consecutive_io_errors = 0u32;
        let loop_start = self.now();
        let mut has_started_range = false;
        let mut wait_iters = 0u64;
        let mut interrupted = false;

        loop {
            // Check for external shutdown signal
            if self.is_shutdown().await {
                info!("Orchestrator: shutdown signal received");
                interrupted = true;
                break;
            }

            // Check time limit
            let now = self.now();
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
                if wait_iters.is_multiple_of(30) {
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
                let storage_msg = format!("[storage] libre={:.1}% ({} octets) statut={:?}", info.free_percent(), info.free_bytes, status);
                if let Some(ref mut sl) = session_logger { sl.log_text(&storage_msg); }
                
                // Hard limit: < 50MB free triggers an immediate OS shutdown to prevent FS corruption
                if info.free_bytes < 50_000_000 {
                    let msg = "Stockage critique (< 50Mo restants) — Arrêt système immédiat";
                    self.log(msg).await;
                    if let Some(ref mut sl) = session_logger { sl.log_text(msg); }
                    break;
                } else if status == crate::core::models::StorageStatus::Critical {
                    let msg = "Stockage critique (seuil %) — Arrêt session";
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
            let live_cfg: AppConfig = self.state.config.read().await.clone();
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

            // ─── Exposure update (metering on the sky ROI only) ─
            let roi_data = crop_roi(&frame.data, frame.width, frame.height, config.detection.roi_top_percent);
            let hist = compute_histogram(roi_data);
            // Timelapse: optionally freeze the exposure once capturing
            // (zero flicker; ramping can be done in post-processing).
            if !(current_phase == Phase::Run && config.exposure.lock_in_run) {
                exposure_ctrl.update(&hist, current_phase);
            }

            // ─── Detection ──────────────────────────────────
            // The detector extracts the ROI itself: give it the full frame
            // (passing the already-cropped ROI used to shrink the analysed
            // area to roi² — e.g. 65 % × 65 % = 42 % of the sky).
            let det_result = detector.analyze(&frame.data, frame.width, frame.height);

            // ─── Session logging ────────────────────────────
            if let Some(ref mut logger) = session_logger {
                let event = SessionEvent {
                    timestamp: self.now().format("%Y-%m-%dT%H:%M:%S").to_string(),
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
                    frame_number: (current_phase == Phase::Run).then_some(frame_number),
                };
                if let Err(e) = logger.log_event(&event) {
                    warn!("Orchestrator: log event failed: {}", e);
                }
            }

            // ─── Power-cut safety: push the logs to the USB key every ~6 frames ─
            iterations += 1;
            if iterations.is_multiple_of(6) {
                if let Some(ref mut sl) = session_logger {
                    if let Err(e) = sl.flush() {
                        warn!("Orchestrator: session log sync failed: {}", e);
                    }
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
                    sleep(Duration::from_secs(live_watch_interval)).await;
                }

                Phase::Run => {
                    // Save the captured frame (NO double capture — raw_frame_opt already holds the DNG)
                    self.save_frame(&config, &analysis_frame, raw_frame_opt.as_ref(), frame_number, det_result.detected, &mut stacker).await;
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

        if let Some(ref mut sl) = session_logger {
            sl.log_text("Fin de session");
            let _ = sl.flush();
        }
        drop(session_logger);

        // Night finished normally: nothing to resume at next boot. An
        // external stop (power watch, systemd) keeps the marker.
        if !interrupted {
            NightMarker::remove(&marker_path);
        }

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
    /// `analysis_frame` is the JPG frame used for analysis (always present);
    /// its `raw_bytes` hold the original full-resolution JPEG on the Pi.
    /// `raw_frame` is the pre-captured DNG (Some for RawDng/RawAndJpg).
    /// `is_aurora` controls whether `_AURORA` is appended to the filename.
    /// `stacker` accumulates frames when stacking is enabled.
    async fn save_frame(
        &self,
        config: &AppConfig,
        analysis_frame: &CaptureFrame,
        raw_frame: Option<&CaptureFrame>,
        frame_num: u64,
        is_aurora: bool,
        stacker: &mut Option<Stacker>,
    ) {
        let timestamp = self.now().format("%Y%m%d_%H%M%S");
        let suffix = if is_aurora { "_AURORA" } else { "" };
        let base = format!("aurora_{}_{:05}{}", timestamp, frame_num, suffix);
        let denoise = &config.capture.denoise;
        info!("Orchestrator: save_frame frame={} format={:?} aurora={}", frame_num, config.capture.output_format, is_aurora);

        // Full-resolution processing only when a treatment is enabled.
        let processed = self.process_jpeg(analysis_frame, denoise).await;

        if matches!(config.capture.output_format, OutputFormat::Jpg | OutputFormat::RawAndJpg) {
            let filename = format!("{}.jpg", base);
            match &processed {
                Some(p) if !p.jpeg.is_empty() => {
                    if let Err(e) = self.storage.save_file(&filename, &p.jpeg).await {
                        error!("Orchestrator: save JPG failed: {}", e);
                    } else {
                        self.save_thumbnail(analysis_frame, &filename).await;
                        if p.hot_pixels_fixed > 0 {
                            info!("Orchestrator: {} ({} pixels chauds corrigés)", filename, p.hot_pixels_fixed);
                        }
                    }
                }
                _ => error!("Orchestrator: no JPEG data to save for frame {}", frame_num),
            }
        }

        if matches!(config.capture.output_format, OutputFormat::RawDng | OutputFormat::RawAndJpg) {
            match raw_frame {
                Some(raw) => {
                    let dng_bytes = if !raw.raw_bytes.is_empty() { &raw.raw_bytes } else { &raw.data };
                    if let Err(e) = self.storage.save_file(&format!("{}.dng", base), dng_bytes).await {
                        error!("Orchestrator: save RAW failed: {}", e);
                    }
                }
                None => error!("Orchestrator: no raw frame available for {:?}", config.capture.output_format),
            }
        }

        // ─── Stacking: one extra, less noisy image every N frames ───
        if denoise.stack_frames >= 2 {
            if let Some(p) = processed.and_then(|p| p.full) {
                let (w, h, rgb) = p;
                let st = stacker.get_or_insert_with(|| Stacker::new(w, h));
                if st.dimensions() != (w, h) {
                    *st = Stacker::new(w, h);
                }
                st.add(&rgb);
                if st.count() >= denoise.stack_frames {
                    let n = st.count();
                    let result = st.result();
                    st.reset();
                    if let Some(stacked) = result {
                        let name = format!("aurora_{}_{:05}_STACK{}{}.jpg", timestamp, frame_num, n, suffix);
                        let original = analysis_frame.raw_bytes.clone();
                        let jpeg = tokio::task::spawn_blocking(move || {
                            encode_jpeg(&stacked, w, h).map(|j| crate::core::jpeg::transplant_exif(&original, j))
                        })
                        .await
                        .ok()
                        .flatten();
                        match jpeg {
                            Some(j) => {
                                if self.storage.save_file(&name, &j).await.is_ok() {
                                    self.save_thumbnail(analysis_frame, &name).await;
                                    info!("Orchestrator: saved {} ({} images empilées)", name, n);
                                }
                            }
                            None => error!("Orchestrator: stack encoding failed"),
                        }
                    }
                }
            }
        }
    }

    /// Prepare the JPEG to save: the original bytes when no treatment is
    /// enabled (no decode at all, lowest CPU use), otherwise decode the full
    /// image, remove hot pixels, re-encode (quality 92) and keep the EXIF.
    async fn process_jpeg(&self, frame: &CaptureFrame, denoise: &DenoiseConfig) -> Option<ProcessedJpeg> {
        let original = frame.raw_bytes.clone();
        let fallback_rgb = (frame.data.clone(), frame.width, frame.height);
        let denoise = denoise.clone();
        if !original.is_empty() && !denoise.needs_full_decode() {
            return Some(ProcessedJpeg { jpeg: original, full: None, hot_pixels_fixed: 0 });
        }
        tokio::task::spawn_blocking(move || {
            // Mock / test frames have no original JPEG: use the RGB pixels.
            let (mut rgb, w, h) = if original.is_empty() {
                fallback_rgb
            } else {
                match image::load_from_memory_with_format(&original, image::ImageFormat::Jpeg) {
                    Ok(img) => {
                        let rgb = img.to_rgb8();
                        let (w, h) = rgb.dimensions();
                        (rgb.into_raw(), w, h)
                    }
                    // Undecodable: keep the original file untouched
                    Err(_) => return Some(ProcessedJpeg { jpeg: original, full: None, hot_pixels_fixed: 0 }),
                }
            };
            let fixed = if denoise.hot_pixels {
                crate::core::denoise::remove_hot_pixels(&mut rgb, w, h, denoise.hot_pixel_threshold)
            } else {
                0
            };
            let jpeg = if denoise.hot_pixels || original.is_empty() {
                let encoded = encode_jpeg(&rgb, w, h)?;
                crate::core::jpeg::transplant_exif(&original, encoded)
            } else {
                original
            };
            let full = (denoise.stack_frames >= 2).then_some((w, h, rgb));
            Some(ProcessedJpeg { jpeg, full, hot_pixels_fixed: fixed })
        })
        .await
        .ok()
        .flatten()
    }

    /// Gallery thumbnail (thumbs/<name>): the EXIF thumbnail of the capture
    /// when available (no encoding at all), otherwise a 320×240 resize of the
    /// analysis pixels.
    async fn save_thumbnail(&self, frame: &CaptureFrame, filename: &str) {
        let thumb_name = format!("thumbs/{}", filename);
        if let Some(thumb) = crate::core::jpeg::exif_thumbnail(&frame.raw_bytes) {
            let _ = self.storage.save_file(&thumb_name, thumb).await;
            return;
        }
        use image::{ImageBuffer, Rgb};
        if let Some(img) = ImageBuffer::<Rgb<u8>, Vec<u8>>::from_raw(frame.width, frame.height, frame.data.clone()) {
            let thumb = image::imageops::resize(&img, 320, 240, image::imageops::FilterType::Triangle);
            if let Some(buf) = encode_jpeg(thumb.as_raw(), 320, 240) {
                let _ = self.storage.save_file(&thumb_name, &buf).await;
            }
        }
    }

    /// Wait for the user to trigger disconnect from the UI.
    /// If the previous night was interrupted, keep the hotspot up for
    /// [`RESUME_DELAY_SECS`] (the phone can cancel or resume at once), then
    /// return the marker and the plan to resume. `None`: normal ARM.
    async fn resume_window(&self) -> Option<(NightMarker, ResumePlan)> {
        let path = self.state.paths.night_marker.clone();
        let marker = NightMarker::load(&path)?;
        let config = self.state.config.read().await.clone();
        let range = TimeRange { start: config.time_range.start, end: config.time_range.end };
        if marker.resume_plan(self.now(), &range).is_none() {
            NightMarker::remove(&path);
            return None;
        }

        let deadline = tokio::time::Instant::now() + Duration::from_secs(RESUME_DELAY_SECS);
        *self.state.resume_at.write().await = Some(deadline);
        self.log(&format!(
            "Nuit interrompue (coupure de courant ?) : reprise automatique dans {} min",
            RESUME_DELAY_SECS / 60
        ))
        .await;
        let by_user = loop {
            let phase = *self.state.phase.read().await;
            if phase == Phase::Shutdown {
                *self.state.resume_at.write().await = None;
                return None;
            }
            if phase == Phase::Disconnect {
                break true; // "Reprendre maintenant"
            }
            if self.state.resume_at.read().await.is_none() {
                NightMarker::remove(&path); // cancelled from the phone
                return None;
            }
            if tokio::time::Instant::now() >= deadline {
                break false;
            }
            sleep(Duration::from_millis(500)).await;
        };
        *self.state.resume_at.write().await = None;

        // A phone may have corrected the clock meanwhile: decide again.
        let range = {
            let c = self.state.config.read().await;
            TimeRange { start: c.time_range.start, end: c.time_range.end }
        };
        match marker.resume_plan(self.now(), &range) {
            Some(plan) => {
                let mut marker = marker;
                marker.resumes += 1;
                self.set_phase(Phase::Disconnect).await;
                Some((marker, plan))
            }
            None => {
                NightMarker::remove(&path);
                if !by_user {
                    self.log("La nuit interrompue est terminée : pas de reprise").await;
                }
                // User asked to start: a fresh night (phase already DISCONNECT)
                None
            }
        }
    }

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

/// Result of [`Orchestrator::process_jpeg`].
struct ProcessedJpeg {
    jpeg: Vec<u8>,
    /// Full-resolution pixels `(w, h, rgb)`, kept only for stacking.
    full: Option<(u32, u32, Vec<u8>)>,
    hot_pixels_fixed: usize,
}

/// JPEG quality of re-encoded images (after noise reduction).
const JPEG_QUALITY: u8 = 92;

/// Encode RGB pixels as JPEG.
pub fn encode_jpeg(rgb: &[u8], width: u32, height: u32) -> Option<Vec<u8>> {
    let mut buf = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY)
        .encode(rgb, width, height, image::ExtendedColorType::Rgb8)
        .ok()?;
    Some(buf)
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
