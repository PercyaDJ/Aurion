use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn, error};

use crate::core::config::{AppConfig, DenoiseConfig};
use crate::core::denoise::Stacker;
use crate::core::detection::AuroraDetector;
use crate::core::exposure::{ExposureController, compute_histogram};
use crate::core::models::{
    CaptureFrame, DetectionResult, ExposureSettings, OutputFormat, Phase, SessionEvent, StorageStatus, TimeRange,
};
use chrono::Datelike;
use crate::core::dark_reminder::{DarkReminder, NightStats};
use crate::core::layout::{self, NightLayout};
use crate::core::night::{
    next_occurrence, NightMarker, ResumePlan, AUTO_START_IDLE_SECS, MAX_RESUMES, RESUME_DELAY_SECS,
    SLEEP_IF_START_IN_SECS, WAKE_LEAD_SECS,
};
use crate::core::session_logger::SessionLogger;
use crate::core::state_machine::StateMachine;
use crate::ports::camera::CameraPort;
use crate::ports::clock::ClockPort;
use crate::ports::network::NetworkApPort;
use crate::ports::storage::StoragePort;
use crate::ports::system::{PowerProfile, SystemPort};
use crate::web::{AppState, AutoStart};

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

/// `JpgAuroraRaw`: RAW kept this long after the last aurora detection, so
/// the end of an aurora (often faint but beautiful) is in RAW too.
pub const AURORA_RAW_HOLD_SECS: i64 = 600;

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
    ///
    /// The night is a sequence of steps, each in its own method below:
    /// resume or ARM, disconnect, night marker, wait for the time range,
    /// key check, night folder, calibration, capture loop, end of night.
    pub async fn run(&self) -> anyhow::Result<()> {
        info!("Orchestrator: starting autonomous loop");

        // ─── Interrupted night (power cut): resume on its own ─
        let resume = self.resume_window().await;

        // ─── ARM phase: wait for the user (or the automatic start) ─
        if resume.is_none() {
            self.wait_for_start().await;
        }

        if self.is_shutdown().await {
            return Ok(());
        }
        self.disconnect().await;

        // ─── Load config snapshot ───────────────────────────
        let mut config: AppConfig = self.state.config.read().await.clone();
        let marker_path = self.state.paths.night_marker.clone();
        let resumed_session = self.mark_night(&mut config, resume, &marker_path).await;
        self.log_start_clock().await;

        if !self.wait_for_night(&config).await {
            return Ok(());
        }

        self.set_phase(Phase::Calibration).await;
        self.log("Phase: CALIBRATION").await;

        // ─── Initialize core components ─────────────────────
        let mut sm = StateMachine::new(config.detection.consecutive_required);
        sm.set_phase(Phase::Calibration);
        let mut exposure = ExposureController::from_config(&config.exposure);
        let detector = AuroraDetector::from_config(&config.detection);

        let storage_path = PathBuf::from(&config.storage.mount_point);
        self.check_key_writable(&config, &storage_path).await;
        let mut files = self.open_night_files(&storage_path, resumed_session.as_deref(), &marker_path).await;

        let is_safe_mode = !config.detection.detection_capture_enabled;
        let mode = if is_safe_mode { "SAFE" } else { "FILTER" };
        self.log_effective_config(&config, &mut files, mode).await;
        self.calibrate(&config, &mut exposure, &mut files).await;
        self.enter_first_phase(is_safe_mode, &mut sm, &mut files).await;
        self.log_night_mode(&config, &mut files).await;

        // ─── Main capture loop ──────────────────────────────
        let range = TimeRange { start: config.time_range.start, end: config.time_range.end };
        let mut night = NightLoop {
            sm,
            exposure,
            detector,
            confirmation: AuroraConfirmation::new(config.detection.consecutive_required),
            stacker: None,
            iterations: 0,
            // A resumed night continues the numbering (timelapse order kept)
            frame_number: files.path.as_deref().and_then(layout::max_frame_number).map(|n| n + 1).unwrap_or(0),
            // Settings of the saved frames: the darks are taken at their average
            stats: NightStats::new(),
            last_aurora_at: None,
            failures: CameraFailures::default(),
            window: NightWindow::new(config.time_range.duration_hours, range, self.now()),
            wait_iters: 0,
        };
        let interrupted = self.capture_loop(&config, &mut files, &mut night, mode).await;

        self.finish_night(&config, files, night.stats, &marker_path, interrupted).await;
        Ok(())
    }

    /// DISCONNECT: leave the phone time to get the answer, then switch the
    /// hotspot off (best effort).
    async fn disconnect(&self) {
        info!("Orchestrator: disconnect requested, 15s timer...");
        self.set_phase(Phase::Disconnect).await;
        sleep(DISCONNECT_DELAY).await;

        if let Some(ref network) = self.network {
            if let Err(e) = network.stop_ap().await {
                warn!("Orchestrator: failed to stop AP: {}", e);
            }
        }
    }

    /// Night marker (resume after a power cut): write a new one, or update
    /// the resumed one. A resumed timer night only runs its remaining time.
    /// Returns the folder name of the resumed night, if any.
    async fn mark_night(
        &self,
        config: &mut AppConfig,
        resume: Option<(NightMarker, ResumePlan)>,
        marker_path: &Path,
    ) -> Option<String> {
        let resumed_session = resume.as_ref().and_then(|(m, _)| m.session.clone());
        match resume {
            Some((marker, plan)) => {
                if let ResumePlan::Timer(hours) = plan {
                    config.time_range.duration_hours = Some(hours);
                }
                if let Err(e) = marker.save(marker_path) {
                    warn!("Orchestrator: cannot update night marker: {}", e);
                }
                self.log(&format!("Reprise de la nuit interrompue ({}/{})", marker.resumes, MAX_RESUMES)).await;
            }
            None => {
                if let Err(e) = NightMarker::new(self.now(), config.time_range.duration_hours).save(marker_path) {
                    warn!("Orchestrator: cannot write night marker: {}", e);
                }
            }
        }
        resumed_session
    }

    /// Log the system clock (critical for diagnosis without screen).
    async fn log_start_clock(&self) {
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

    /// Time-range mode: wait for the start of the night. A Pi that can wake
    /// itself (Pi 5) powers off and wakes up just before, rather than idling
    /// for hours (the marker resumes the night). `false`: nothing more to do
    /// (stop requested, or powered off until the night).
    async fn wait_for_night(&self, config: &AppConfig) -> bool {
        if config.time_range.duration_hours.is_some() {
            return true;
        }
        let range = TimeRange { start: config.time_range.start, end: config.time_range.end };
        let mut logged_wait = false;
        loop {
            if self.is_shutdown().await {
                return false;
            }
            let now = self.now();
            match before_night(now, &range, self.system.can_wake()) {
                BeforeNight::Start => break,
                BeforeNight::Sleep { wake, hours } => {
                    if self.system.schedule_wake(wake.with_timezone(&chrono::Utc)).await.is_ok() {
                        self.log(&format!("Nuit dans {} h : extinction, réveil programmé à {}", hours, wake.format("%H:%M"))).await;
                        self.set_phase(Phase::Shutdown).await;
                        let _ = self.storage.sync().await;
                        if let Err(e) = self.system.shutdown().await {
                            error!("Orchestrator: shutdown before the night failed: {}", e);
                        }
                        return false;
                    }
                }
                BeforeNight::Wait => {}
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
        true
    }

    /// USB storage: write-first check with auto-mount. Don't rely on
    /// `mountpoint -q` (can lie): try a real write, and if it fails,
    /// (re)mount and retry. The night goes on either way.
    async fn check_key_writable(&self, config: &AppConfig, configured_path: &Path) {
        let probe_path = configured_path.join(".aurion_probe");
        let probe = || {
            std::fs::create_dir_all(configured_path).is_ok() && std::fs::write(&probe_path, b"ok").is_ok() && {
                let _ = std::fs::remove_file(&probe_path);
                true
            }
        };

        // First attempt: direct write (USB already mounted via fstab/boot)
        if probe() {
            self.log(&format!("USB accessible: {}", config.storage.mount_point)).await;
            return;
        }

        // Second attempt: try to (re)mount then retry
        warn!("Orchestrator: direct write failed, attempting mount...");
        self.log("USB: montage en cours...").await;
        let _ = self.storage.mount().await; // ignore if already mounted
        sleep(Duration::from_secs(3)).await;
        if probe() {
            self.log(&format!("USB monte et accessible: {}", config.storage.mount_point)).await;
            return;
        }

        // Clear warning: the session continues and will try to write anyway
        // (write may still succeed if permissions allow)
        let msg = format!(
            "[!] USB ({}) non accessible en ecriture - la session va tenter de continuer",
            config.storage.mount_point
        );
        warn!("Orchestrator: {}", msg);
        self.log(&msg).await;
    }

    /// Session logs and night folder on the key (a resumed night continues
    /// in its folder). Leftovers of a power cut are removed.
    async fn open_night_files(&self, storage_path: &Path, resumed_session: Option<&str>, marker_path: &Path) -> NightFiles {
        let opened = match resumed_session {
            Some(name) => SessionLogger::open_named(storage_path, name, self.now()),
            None => SessionLogger::new_at(storage_path, self.now()),
        };
        let logger = match opened {
            Ok(l) => {
                info!("Orchestrator: session logger at {:?}", l.session_path());
                Some(l)
            }
            Err(e) => {
                warn!("Orchestrator: cannot create session logger: {} — continuing without", e);
                None
            }
        };

        let night_name = logger
            .as_ref()
            .and_then(|l| l.session_path().file_name().map(|n| n.to_string_lossy().to_string()));
        let night_layout = night_name.as_deref().map(NightLayout::night).unwrap_or_else(NightLayout::root);
        let night_path = night_name.as_deref().map(|n| layout::night_dir(storage_path, n));
        if let Some(ref dir) = night_path {
            let removed = layout::remove_partial_files(dir) + layout::remove_partial_files(storage_path);
            if removed > 0 {
                self.log(&format!("{} fichier(s) incomplet(s) d'une coupure supprimé(s)", removed)).await;
            }
        }
        // The marker remembers the folder so a resumed night continues in it
        if let (Some(name), Some(mut m)) = (&night_name, NightMarker::load(marker_path)) {
            if m.session.as_deref() != Some(name.as_str()) {
                m.session = Some(name.clone());
                let _ = m.save(marker_path);
            }
        }
        NightFiles { logger, name: night_name, layout: night_layout, path: night_path }
    }

    /// Mode, and the effective config in session.log (checked the next
    /// morning without a screen).
    async fn log_effective_config(&self, config: &AppConfig, files: &mut NightFiles, mode: &str) {
        self.log(&format!("Mode: {}", mode)).await;
        files.text("=== CONFIG EFFECTIVE ===");
        files.text(&format!("Mode: {}", mode));
        files.text(&format!("Format: {:?}", config.capture.output_format));
        files.text(&format!("Intervalle: {}s", config.capture.capture_interval_secs));
        files.text(&format!("Plage: {} -> {}", config.time_range.start, config.time_range.end));
        files.text(&format!("Minuteur: {:?}h", config.time_range.duration_hours));
        files.text(&format!("detection_capture_enabled: {}", config.detection.detection_capture_enabled));
        files.text(&format!("Mount point: {}", config.storage.mount_point));
        files.text("=======================");
    }

    /// CALIBRATION: 3 frames to stabilize exposure. Captured in /tmp (the
    /// USB key may be slow at startup), never saved.
    async fn calibrate(&self, config: &AppConfig, exposure: &mut ExposureController, files: &mut NightFiles) {
        let tmp_path = Path::new("/tmp");
        info!("Orchestrator: calibrating exposure (3 frames)...");
        for i in 0..3 {
            match self.camera.capture_jpg(&exposure.current(), tmp_path).await {
                Ok(frame) => {
                    let roi_data = crop_roi(&frame.data, frame.width, frame.height, config.detection.roi_top_percent);
                    let hist = compute_histogram(roi_data);
                    let settings = exposure.update(&hist, Phase::Calibration);
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

        let settings = exposure.current();
        let calib_msg = format!("Calibration done: ISO {} / {}µs", settings.iso, settings.shutter_us);
        self.log(&calib_msg).await;
        files.text(&calib_msg);
    }

    /// SAFE mode captures all night (RUN), FILTER mode watches for an aurora.
    async fn enter_first_phase(&self, is_safe_mode: bool, sm: &mut StateMachine, files: &mut NightFiles) {
        let phase = first_phase(is_safe_mode);
        sm.set_phase(phase);
        self.set_phase(phase).await;
        if is_safe_mode {
            info!("Orchestrator: SAFE mode → Run directly");
            self.power_profile(PowerProfile::Capture).await;
            self.log("Phase: RUN (SAFE mode — capture toute la nuit)").await;
            files.text("Phase: RUN (SAFE mode)");
        } else {
            info!("Orchestrator: FILTER mode → Watch");
            self.power_profile(PowerProfile::Watch).await;
            self.log("Phase: WATCH (FILTER mode — attente détection)").await;
            files.text("Phase: WATCH (FILTER mode)");
        }
    }

    /// End of the night: timer (N hours from now) or time range.
    async fn log_night_mode(&self, config: &AppConfig, files: &mut NightFiles) {
        let msg = if let Some(hours) = config.time_range.duration_hours {
            let end = self.now() + chrono::Duration::seconds((hours * 3600.0) as i64);
            format!("Mode minuteur: {}h → fin prévue à {}", hours, end.format("%H:%M:%S"))
        } else {
            format!("Mode plage horaire: {} → {}", config.time_range.start, config.time_range.end)
        };
        self.log(&msg).await;
        files.text(&msg);
    }

    /// The capture loop, until the end of the night. `true` when stopped
    /// from outside (power watch, systemd): the night can be resumed.
    async fn capture_loop(&self, config: &AppConfig, files: &mut NightFiles, night: &mut NightLoop, mode: &str) -> bool {
        loop {
            // Check for external shutdown signal
            if self.is_shutdown().await {
                info!("Orchestrator: shutdown signal received");
                return true;
            }

            let now = self.now();
            let window = night.window.check(now);
            if window == WindowState::Wait {
                if night.wait_iters.is_multiple_of(30) {
                    let wait_msg = format!(
                        "Attente du début de plage (actuel: {}, début: {})",
                        now.format("%H:%M:%S"), config.time_range.start
                    );
                    info!("Orchestrator: {}", wait_msg);
                    self.log(&wait_msg).await;
                    files.text(&wait_msg);
                }
                night.wait_iters += 1;
                sleep(Duration::from_secs(10)).await;
                continue;
            }

            // Log each loop iteration so we can trace from session.log
            let should_stop = window == WindowState::Stop;
            let iter_msg = format!(
                "[loop] heure={} phase={:?} should_stop={}",
                now.format("%H:%M:%S"), night.sm.phase(), should_stop
            );
            info!("Orchestrator: {}", iter_msg);
            files.text(&iter_msg);

            if should_stop {
                let stop_msg = format!("Heure de fin de plage atteinte ({}) — arret capture", now.format("%H:%M:%S"));
                info!("Orchestrator: {}", stop_msg);
                self.log(&stop_msg).await;
                files.text(&stop_msg);
                return false;
            }

            if !self.check_storage(config, files).await {
                return false;
            }

            let current_phase = night.sm.phase();
            let exposure = night.exposure.current();
            let (live_capture_interval, live_watch_interval) = self.live_intervals(&mut night.detector).await;

            let wants_raw = captures_raw_now(current_phase, config.capture.output_format);
            let capture_started = tokio::time::Instant::now();
            let Some((analysis_frame, raw_frame)) = self.capture(wants_raw, &exposure, &mut night.failures, files).await else {
                continue;
            };
            let capture_ms = capture_started.elapsed().as_millis() as u64;

            let det_result = night.analyze(&analysis_frame, config, current_phase);

            // ─── Session logging ────────────────────────────
            if let Some(ref mut logger) = files.logger {
                let event = night_event(NightEventInput {
                    timestamp: self.now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                    config,
                    mode,
                    phase: current_phase,
                    exposure: &exposure,
                    detection: &det_result,
                    consecutive_hits: night.confirmation.hits(),
                    frame_number: night.frame_number,
                    capture_ms,
                });
                if let Err(e) = logger.log_event(&event) {
                    warn!("Orchestrator: log event failed: {}", e);
                }
            }

            // ─── Power-cut safety: push the logs to the USB key every ~6 frames ─
            night.iterations += 1;
            if night.iterations.is_multiple_of(6) {
                if let Some(ref mut sl) = files.logger {
                    if let Err(e) = sl.flush() {
                        warn!("Orchestrator: session log sync failed: {}", e);
                    }
                }
            }

            // ─── Phase-specific logic ───────────────────────
            match current_phase {
                Phase::Watch => self.watch_step(config, night, &det_result, live_watch_interval).await,
                Phase::Run => {
                    let shot = Shot { frame: &analysis_frame, raw: raw_frame.as_ref(), detected: det_result.detected, exposure: &exposure };
                    self.run_step(config, files, night, shot).await;
                    // Wait for configured capture interval (hot-reloadable timelapse pacing)
                    if let Some(pause) = run_pause(live_capture_interval, capture_started.elapsed()) {
                        sleep(pause).await;
                    }
                }
                _ => {
                    // Should not happen but handle gracefully
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Free space on the key: logged every frame; `false` stops the night
    /// before the file system gets full.
    async fn check_storage(&self, config: &AppConfig, files: &mut NightFiles) -> bool {
        let Ok(info) = self.storage.info() else {
            // df failed — USB may not be mounted, log it
            let msg = "[storage] df echoue - USB inaccessible ?";
            files.text(msg);
            warn!("Orchestrator: {}", msg);
            return true;
        };
        let status = info.status(
            config.storage.warning_percent as f64,
            config.storage.critical_percent as f64,
        );
        let storage_msg = format!("[storage] libre={:.1}% ({} octets) statut={:?}", info.free_percent(), info.free_bytes, status);
        files.text(&storage_msg);

        let msg = match storage_verdict(info.free_bytes, status) {
            StorageVerdict::Continue => return true,
            StorageVerdict::StopNearlyFull => "Stockage critique (< 50Mo restants) — Arrêt système immédiat",
            StorageVerdict::StopCritical => "Stockage critique (seuil %) — Arrêt session",
        };
        self.log(msg).await;
        files.text(msg);
        false
    }

    /// Live config (hot-reload of mutable params): capture and watch
    /// intervals, detection thresholds (sensitivity, ROI…). Immutable params
    /// (mount_point, output_format, time_range) use the snapshot.
    async fn live_intervals(&self, detector: &mut AuroraDetector) -> (u32, u64) {
        let live_cfg: AppConfig = self.state.config.read().await.clone();
        detector.update_config(&live_cfg.detection);
        // 0 = next photo right after this one (the exposure sets the pace)
        (live_cfg.capture.capture_interval_secs, live_cfg.capture.watch_interval_secs.max(5))
    }

    /// One capture, format-aware. With RAW: a single capture returns the DNG
    /// (saved) and the JPEG (analysis). Temp files go to /tmp, not the key.
    /// `None` after a failure (logged, with a 5 min pause after 5 in a row).
    async fn capture(
        &self,
        wants_raw: bool,
        exposure: &ExposureSettings,
        failures: &mut CameraFailures,
        files: &mut NightFiles,
    ) -> Option<(CaptureFrame, Option<CaptureFrame>)> {
        let result = if wants_raw {
            self.camera.capture_raw_and_jpg(exposure, Path::new("/tmp")).await.map(|(raw, jpg)| (jpg, Some(raw)))
        } else {
            self.camera.capture_jpg(exposure, Path::new("/tmp")).await.map(|f| (f, None))
        };
        let e = match result {
            Ok(frames) => {
                failures.success();
                return Some(frames);
            }
            Err(e) => e,
        };
        let n = failures.failure();
        let (kind, label, pause_msg) = if wants_raw {
            ("RAW", "Capture RAW echouee", "5 erreurs RAW consecutives - pause 5min")
        } else {
            ("JPG", "Capture echouee", "5 erreurs consecutives - pause 5min")
        };
        let fail_msg = format!("CAPTURE ECHEC {}: {} ({}/{})", kind, e, n, MAX_CAMERA_FAILURES);
        error!("Orchestrator: {}", fail_msg);
        self.log(&format!("{}: {} ({}/{})", label, e, n, MAX_CAMERA_FAILURES)).await;
        files.text(&fail_msg);
        if failures.needs_pause() {
            self.log(pause_msg).await;
            files.text(pause_msg);
            warn!("Orchestrator: 5 consecutive camera errors → pausing 5min for auto-recovery");
            sleep(Duration::from_secs(300)).await;
            failures.reset();
        }
        sleep(Duration::from_secs(5)).await;
        None
    }

    /// WATCH (FILTER mode): switch to RUN once the aurora is confirmed on
    /// consecutive frames, then wait for the watch interval.
    async fn watch_step(&self, config: &AppConfig, night: &mut NightLoop, det: &DetectionResult, interval_secs: u64) {
        let confirmed = night.confirmation.observe(det.detected);
        if det.detected {
            info!(
                "Orchestrator: aurora detected ({}/{}) score={:.2} color={}",
                night.confirmation.hits(), config.detection.consecutive_required,
                det.aurora_score, det.aurora_color
            );
        }
        if confirmed {
            info!("Orchestrator: aurora CONFIRMED → RUN");
            night.sm.set_phase(Phase::Run);
            self.set_phase(Phase::Run).await;
            self.power_profile(PowerProfile::Capture).await;
            self.log("🌌 Aurore confirmée → capture intensive!").await;
        }
        sleep(Duration::from_secs(interval_secs)).await;
    }

    /// RUN: save the frame already captured (no second capture). A drop in
    /// detection never stops the capture (momentary gaps are common).
    async fn run_step(
        &self,
        config: &AppConfig,
        files: &NightFiles,
        night: &mut NightLoop,
        shot: Shot<'_>,
    ) {
        if shot.detected {
            night.last_aurora_at = Some(self.now());
        }
        let raw = shot.raw.filter(|_| keeps_raw(config.capture.output_format, night.last_aurora_at, self.now()));
        self.save_frame(config, &files.layout, shot.frame, raw, night.frame_number, shot.detected, &mut night.stacker).await;
        night.frame_number += 1;
        night.stats.add(shot.exposure.iso, shot.exposure.shutter_us, self.system.temperature_c());
    }

    /// SHUTDOWN: close the logs, write the aurora index, forget or keep the
    /// marker, leave the darks reminder, program the next wake-up (expedition),
    /// sync and power off.
    async fn finish_night(&self, config: &AppConfig, files: NightFiles, stats: NightStats, marker_path: &Path, interrupted: bool) {
        info!("Orchestrator: entering shutdown sequence");
        self.set_phase(Phase::Shutdown).await;
        self.log("Phase: SHUTDOWN").await;

        let NightFiles { mut logger, name, path, .. } = files;

        // Darks reminder shown on the home screen at the next power-on
        let dark_path = self.state.paths.dark_reminder.clone();
        if let Some(reminder) = stats.reminder(name, self.now().timestamp_millis()) {
            let reminder = reminder.merge(DarkReminder::load(&dark_path));
            let summary = reminder.summary();
            if let Err(e) = reminder.save(&dark_path) {
                warn!("Orchestrator: cannot write the darks reminder: {}", e);
            }
            if let Some(ref mut sl) = logger {
                sl.log_text(&summary);
            }
            self.log(&summary).await;
        }

        if let Some(mut sl) = logger {
            sl.log_text("Fin de session");
            let _ = sl.flush();
        }

        // Spreadsheet of the auroras of the night (rewritten after a resume)
        if let Some(ref dir) = path {
            match layout::write_aurora_index(dir) {
                Ok(n) => self.log(&format!("aurores.csv : {} image(s) avec aurore", n)).await,
                Err(e) => warn!("Orchestrator: aurores.csv: {}", e),
            }
        }

        // Night finished normally: nothing to resume at next boot. An
        // external stop (power watch, systemd) keeps the marker.
        if !interrupted {
            NightMarker::remove(marker_path);
        }

        // Expedition: a Raspberry Pi 5 wakes itself up for the next night
        if !interrupted && config.expedition.enabled {
            if config.time_range.duration_hours.is_none() && self.system.can_wake() {
                let wake = wake_before(self.now(), config.time_range.start);
                match self.system.schedule_wake(wake.with_timezone(&chrono::Utc)).await {
                    Ok(()) => self.log(&format!("Expédition : réveil programmé le {}", wake.format("%d/%m à %H:%M"))).await,
                    Err(e) => self.log(&format!("Expédition : réveil automatique impossible ({})", e)).await,
                }
            } else {
                self.log("Expédition : rebranchez l'alimentation ce soir, la nuit démarrera seule").await;
            }
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
    }

    /// Save a captured frame to storage.
    /// `analysis_frame` is the JPG frame used for analysis (always present);
    /// its `raw_bytes` hold the original full-resolution JPEG on the Pi.
    /// `raw_frame` is the pre-captured DNG (Some for RawDng/RawAndJpg).
    /// `is_aurora` controls whether `_AURORA` is appended to the filename.
    /// `stacker` accumulates frames when stacking is enabled.
    #[allow(clippy::too_many_arguments)]
    async fn save_frame(
        &self,
        config: &AppConfig,
        night: &NightLayout,
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

        if config.capture.output_format.saves_jpg() {
            let filename = format!("{}.jpg", base);
            match &processed {
                Some(p) if !p.jpeg.is_empty() => {
                    if let Err(e) = self.storage.save_file(&night.image(&filename), &p.jpeg).await {
                        error!("Orchestrator: save JPG failed: {}", e);
                    } else {
                        self.save_thumbnail(night, analysis_frame, &filename).await;
                        if p.hot_pixels_fixed > 0 {
                            info!("Orchestrator: {} ({} pixels chauds corrigés)", filename, p.hot_pixels_fixed);
                        }
                    }
                }
                _ => error!("Orchestrator: no JPEG data to save for frame {}", frame_num),
            }
        }

        match raw_frame {
            Some(raw) => {
                let dng_bytes = if !raw.raw_bytes.is_empty() { &raw.raw_bytes } else { &raw.data };
                let dng_name = format!("{}.dng", base);
                if let Err(e) = self.storage.save_file(&night.image(&dng_name), dng_bytes).await {
                    error!("Orchestrator: save RAW failed: {}", e);
                } else if !config.capture.output_format.saves_jpg() {
                    // RAW only: the gallery still gets a preview of each frame
                    self.save_thumbnail(night, analysis_frame, &dng_name).await;
                }
            }
            None if matches!(config.capture.output_format, OutputFormat::RawDng | OutputFormat::RawAndJpg) => {
                error!("Orchestrator: no raw frame available for {:?}", config.capture.output_format)
            }
            None => {} // JpgAuroraRaw outside an aurora
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
                                if self.storage.save_file(&night.image(&name), &j).await.is_ok() {
                                    self.save_thumbnail(night, analysis_frame, &name).await;
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
    async fn process_jpeg<'a>(&self, frame: &'a CaptureFrame, denoise: &DenoiseConfig) -> Option<ProcessedJpeg<'a>> {
        // Default settings: the capture is saved as is, borrowed (no copy of
        // the ~5 MB JPEG nor of the pixels for every frame)
        if !frame.raw_bytes.is_empty() && !denoise.needs_full_decode() {
            return Some(ProcessedJpeg { jpeg: std::borrow::Cow::Borrowed(&frame.raw_bytes), full: None, hot_pixels_fixed: 0 });
        }
        let original = frame.raw_bytes.clone();
        let fallback_rgb = (frame.data.clone(), frame.width, frame.height);
        let denoise = denoise.clone();
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
                    Err(_) => return Some(ProcessedJpeg { jpeg: original.into(), full: None, hot_pixels_fixed: 0 }),
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
            Some(ProcessedJpeg { jpeg: jpeg.into(), full, hot_pixels_fixed: fixed })
        })
        .await
        .ok()
        .flatten()
    }

    /// Gallery thumbnail (thumbs/<name>): the EXIF thumbnail of the capture
    /// when available (no encoding at all), otherwise a 320×240 resize of the
    /// analysis pixels.
    async fn save_thumbnail(&self, night: &NightLayout, frame: &CaptureFrame, filename: &str) {
        let thumb_name = night.thumb(filename);
        if let Some(thumb) = crate::core::jpeg::exif_thumbnail(&frame.raw_bytes) {
            let _ = self.storage.save_file(&thumb_name, thumb).await;
            return;
        }
        use image::{ImageBuffer, Rgb};
        if let Some(img) = ImageBuffer::<Rgb<u8>, &[u8]>::from_raw(frame.width, frame.height, &frame.data[..]) {
            let thumb = image::imageops::resize(&img, 320, 240, image::imageops::FilterType::Triangle);
            if let Some(buf) = encode_jpeg(thumb.as_raw(), 320, 240) {
                let _ = self.storage.save_file(&thumb_name, &buf).await;
            }
        }
    }

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

    /// ARM phase: wait for "Lancer la nuit". In expedition mode the night
    /// also starts on its own once nobody has used the interface for
    /// [`AUTO_START_IDLE_SECS`], provided the clock is reliable and the key
    /// is present.
    async fn wait_for_start(&self) {
        let idle_needed = Duration::from_secs(AUTO_START_IDLE_SECS);
        let mut announced = false;
        loop {
            let phase = *self.state.phase.read().await;
            if phase != Phase::Arm {
                break;
            }
            let (enabled, mount) = {
                let c = self.state.config.read().await;
                (c.expedition.enabled, c.storage.mount_point.clone())
            };
            let health = crate::sys::storage_health(Path::new(&mount));
            let key_ok = health.writable && (health.is_mountpoint || !cfg!(feature = "rpi"));
            let next = if !enabled {
                AutoStart::Off
            } else if !self.state.clock_trusted() {
                AutoStart::Blocked("heure à confirmer : ouvrez cette page une fois depuis le téléphone")
            } else if !key_ok {
                AutoStart::Blocked("clé USB absente")
            } else if let Some(busy) = crate::web::api::night_blocker(&self.state).await {
                AutoStart::Blocked(busy)
            } else {
                let left = idle_needed.saturating_sub(self.state.idle_for());
                if left.is_zero() {
                    self.log("Expédition : démarrage automatique de la nuit").await;
                    self.set_phase(Phase::Disconnect).await;
                    break;
                }
                if !announced {
                    self.log("Expédition : la nuit démarrera seule 5 min après la dernière utilisation de l'interface").await;
                    announced = true;
                }
                AutoStart::At(tokio::time::Instant::now() + left)
            };
            *self.state.auto_start.write().await = next;
            sleep(Duration::from_secs(1)).await;
        }
        *self.state.auto_start.write().await = AutoStart::Off;
    }

    /// Apply a night power profile (best effort: never stops the night).
    async fn power_profile(&self, profile: PowerProfile) {
        if let Err(e) = self.system.set_power_profile(profile).await {
            warn!("Orchestrator: power profile {:?}: {}", profile, e);
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

// ─── Steps of the night: state and decisions ──────────────────────────
// The decisions of the capture loop are plain functions and small types,
// tested on their own below; the orchestrator only wires them to the ports.

/// Session logs and night folder on the key.
struct NightFiles {
    logger: Option<SessionLogger>,
    /// Folder name of the night (`None` without a session logger).
    name: Option<String>,
    layout: NightLayout,
    /// `sessions/<night>`; `None` without a session logger.
    path: Option<PathBuf>,
}

impl NightFiles {
    /// Line in session.log (nothing without a logger).
    fn text(&mut self, msg: &str) {
        if let Some(ref mut sl) = self.logger {
            sl.log_text(msg);
        }
    }
}

/// One capture of the loop, as the RUN step saves it.
struct Shot<'a> {
    /// JPEG used for analysis (and saved as JPEG).
    frame: &'a CaptureFrame,
    /// DNG of the same capture, when the camera produced one.
    raw: Option<&'a CaptureFrame>,
    detected: bool,
    exposure: &'a ExposureSettings,
}

/// Everything the capture loop keeps from one frame to the next.
struct NightLoop {
    sm: StateMachine,
    exposure: ExposureController,
    detector: AuroraDetector,
    confirmation: AuroraConfirmation,
    stacker: Option<Stacker>,
    iterations: u64,
    frame_number: u64,
    stats: NightStats,
    last_aurora_at: Option<chrono::DateTime<chrono::Local>>,
    failures: CameraFailures,
    window: NightWindow,
    wait_iters: u64,
}

impl NightLoop {
    /// Exposure update (metering on the sky ROI only), then detection.
    fn analyze(&mut self, frame: &CaptureFrame, config: &AppConfig, phase: Phase) -> DetectionResult {
        let roi_data = crop_roi(&frame.data, frame.width, frame.height, config.detection.roi_top_percent);
        let hist = compute_histogram(roi_data);
        // Timelapse: optionally freeze the exposure once capturing
        // (zero flicker; ramping can be done in post-processing).
        if !(phase == Phase::Run && config.exposure.lock_in_run) {
            self.exposure.update(&hist, phase);
        }
        // The detector extracts the ROI itself: give it the full frame
        // (passing the already-cropped ROI used to shrink the analysed
        // area to roi² — e.g. 65 % × 65 % = 42 % of the sky).
        self.detector.analyze(&frame.data, frame.width, frame.height)
    }
}

/// Where the night stands at a given moment of the capture loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// Before the time range (time-range mode only).
    Wait,
    Capture,
    /// Timer elapsed, or time range left after being in it.
    Stop,
}

/// The capture window: timer mode (N hours from the start of the loop) or
/// time-range mode (stops when leaving the range, once it was entered).
#[derive(Debug, Clone)]
pub struct NightWindow {
    timer_secs: Option<i64>,
    range: TimeRange,
    start: chrono::DateTime<chrono::Local>,
    entered: bool,
}

impl NightWindow {
    pub fn new(duration_hours: Option<f64>, range: TimeRange, start: chrono::DateTime<chrono::Local>) -> Self {
        Self { timer_secs: duration_hours.map(|h| (h * 3600.0) as i64), range, start, entered: false }
    }

    pub fn check(&mut self, now: chrono::DateTime<chrono::Local>) -> WindowState {
        match self.timer_secs {
            Some(max) if now.signed_duration_since(self.start).num_seconds() >= max => WindowState::Stop,
            Some(_) => WindowState::Capture,
            None if self.range.contains(now.time()) => {
                self.entered = true;
                WindowState::Capture
            }
            None if self.entered => WindowState::Stop,
            None => WindowState::Wait,
        }
    }
}

/// Before a time-range night: what to do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeforeNight {
    Start,
    /// Power off and wake up at `wake` (the night starts in `hours` h).
    Sleep { wake: chrono::DateTime<chrono::Local>, hours: i64 },
    Wait,
}

/// Start now, wait, or (a Pi that can wake itself) power off until just
/// before the night when it starts in more than [`SLEEP_IF_START_IN_SECS`].
pub fn before_night(now: chrono::DateTime<chrono::Local>, range: &TimeRange, can_wake: bool) -> BeforeNight {
    if range.contains(now.time()) {
        return BeforeNight::Start;
    }
    let until_start = (next_occurrence(now, range.start) - now).num_seconds();
    if can_wake && until_start > SLEEP_IF_START_IN_SECS {
        return BeforeNight::Sleep { wake: wake_before(now, range.start), hours: until_start / 3600 };
    }
    BeforeNight::Wait
}

/// Wake-up time for the next night starting at `start`.
pub fn wake_before(now: chrono::DateTime<chrono::Local>, start: chrono::NaiveTime) -> chrono::DateTime<chrono::Local> {
    next_occurrence(now, start) - chrono::Duration::seconds(WAKE_LEAD_SECS)
}

/// Below this free space the night stops at once, before the file system
/// gets corrupted by a full disk.
pub const MIN_FREE_BYTES: u64 = 50_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageVerdict {
    Continue,
    /// Less than [`MIN_FREE_BYTES`] left.
    StopNearlyFull,
    /// Below the configured critical percentage.
    StopCritical,
}

pub fn storage_verdict(free_bytes: u64, status: StorageStatus) -> StorageVerdict {
    if free_bytes < MIN_FREE_BYTES {
        StorageVerdict::StopNearlyFull
    } else if status == StorageStatus::Critical {
        StorageVerdict::StopCritical
    } else {
        StorageVerdict::Continue
    }
}

/// FILTER mode: an aurora counts once seen on `required` frames in a row.
#[derive(Debug, Clone)]
pub struct AuroraConfirmation {
    required: u32,
    hits: u32,
}

impl AuroraConfirmation {
    pub fn new(required: u32) -> Self {
        Self { required, hits: 0 }
    }

    /// Frames in a row with an aurora so far.
    pub fn hits(&self) -> u32 {
        self.hits
    }

    /// Record a frame; `true` when the aurora is confirmed.
    pub fn observe(&mut self, detected: bool) -> bool {
        if detected {
            self.hits += 1;
            self.hits >= self.required
        } else {
            self.hits = 0;
            false
        }
    }
}

/// Camera failures in a row before a recovery pause.
pub const MAX_CAMERA_FAILURES: u32 = 5;

#[derive(Debug, Clone, Default)]
pub struct CameraFailures {
    count: u32,
}

impl CameraFailures {
    pub fn success(&mut self) {
        self.count = 0;
    }

    /// Record a failure; returns the number in a row.
    pub fn failure(&mut self) -> u32 {
        self.count += 1;
        self.count
    }

    pub fn needs_pause(&self) -> bool {
        self.count >= MAX_CAMERA_FAILURES
    }

    pub fn reset(&mut self) {
        self.count = 0;
    }
}

/// SAFE mode captures all night, FILTER mode watches first.
pub fn first_phase(is_safe_mode: bool) -> Phase {
    if is_safe_mode { Phase::Run } else { Phase::Watch }
}

/// Watching the sky (nothing saved): a JPEG is enough, no RAW readout nor
/// DNG writing for a frame that is thrown away.
pub fn captures_raw_now(phase: Phase, format: OutputFormat) -> bool {
    phase == Phase::Run && format.captures_raw()
}

/// Whether the DNG of this frame is kept. `JpgAuroraRaw`: only during an
/// aurora and [`AURORA_RAW_HOLD_SECS`] after the last detection.
pub fn keeps_raw(
    format: OutputFormat,
    last_aurora_at: Option<chrono::DateTime<chrono::Local>>,
    now: chrono::DateTime<chrono::Local>,
) -> bool {
    match format {
        OutputFormat::JpgAuroraRaw => last_aurora_at.is_some_and(|t| (now - t).num_seconds() <= AURORA_RAW_HOLD_SECS),
        f => f.captures_raw(),
    }
}

/// Pause after a RUN frame: the configured interval, or back to back
/// (`0`) with at least 1 s per frame, so that a camera answering instantly
/// (fault) never makes the loop spin on the CPU.
pub fn run_pause(interval_secs: u32, spent: Duration) -> Option<Duration> {
    if interval_secs > 0 {
        Some(Duration::from_secs(interval_secs as u64))
    } else if spent < Duration::from_secs(1) {
        Some(Duration::from_secs(1) - spent)
    } else {
        None
    }
}

/// What one line of event.jsonl is made of.
struct NightEventInput<'a> {
    timestamp: String,
    config: &'a AppConfig,
    mode: &'a str,
    phase: Phase,
    exposure: &'a ExposureSettings,
    detection: &'a DetectionResult,
    consecutive_hits: u32,
    frame_number: u64,
    capture_ms: u64,
}

/// One line of event.jsonl. The frame number only exists in RUN (saved frames).
fn night_event(i: NightEventInput) -> SessionEvent {
    SessionEvent {
        timestamp: i.timestamp,
        phase: format!("{:?}", i.phase),
        capture_mode: i.mode.to_string(),
        exposure_us: i.exposure.shutter_us,
        iso: i.exposure.iso,
        format: format!("{:?}", i.config.capture.output_format),
        roi_excluded_percent: 100 - i.config.detection.roi_top_percent,
        aurora_score: i.detection.aurora_score,
        aurora_detected: i.detection.detected,
        aurora_color: i.detection.aurora_color.to_string(),
        consecutive_hits: i.consecutive_hits,
        moon_mask_active: i.detection.moon_masked,
        frame_number: (i.phase == Phase::Run).then_some(i.frame_number),
        capture_ms: Some(i.capture_ms),
    }
}

/// Result of [`Orchestrator::process_jpeg`].
struct ProcessedJpeg<'a> {
    /// Borrowed from the capture when saved untouched.
    jpeg: std::borrow::Cow<'a, [u8]>,
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

    // ─── Steps of the night ───────────────────────────────

    use chrono::{Local, NaiveTime, TimeZone};

    fn at(h: u32, m: u32) -> chrono::DateTime<Local> {
        Local.with_ymd_and_hms(2026, 1, 15, h, m, 0).unwrap()
    }

    fn night_range() -> TimeRange {
        TimeRange { start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(), end: NaiveTime::from_hms_opt(6, 0, 0).unwrap() }
    }

    #[test]
    fn timer_window_stops_after_its_duration() {
        let mut w = NightWindow::new(Some(0.5), night_range(), at(12, 0));
        assert_eq!(w.check(at(12, 0)), WindowState::Capture);
        assert_eq!(w.check(at(12, 29)), WindowState::Capture);
        assert_eq!(w.check(at(12, 30)), WindowState::Stop);
    }

    #[test]
    fn timer_window_ignores_the_time_range() {
        // Noon is outside 21:00 → 06:00, the timer runs anyway
        let mut w = NightWindow::new(Some(1.0), night_range(), at(12, 0));
        assert_eq!(w.check(at(12, 10)), WindowState::Capture);
    }

    #[test]
    fn range_window_waits_then_captures_then_stops_on_leaving() {
        let mut w = NightWindow::new(None, night_range(), at(20, 0));
        assert_eq!(w.check(at(20, 50)), WindowState::Wait, "before the range, never entered");
        assert_eq!(w.check(at(21, 0)), WindowState::Capture);
        assert_eq!(w.check(at(23, 59)), WindowState::Capture);
        assert_eq!(w.check(Local.with_ymd_and_hms(2026, 1, 16, 5, 59, 0).unwrap()), WindowState::Capture, "overnight");
        assert_eq!(w.check(Local.with_ymd_and_hms(2026, 1, 16, 6, 1, 0).unwrap()), WindowState::Stop, "left after entering");
    }

    #[test]
    fn before_night_starts_inside_the_range() {
        assert_eq!(before_night(at(22, 0), &night_range(), true), BeforeNight::Start);
    }

    #[test]
    fn before_night_sleeps_only_when_the_pi_can_wake_and_the_night_is_far() {
        let far = at(14, 0); // 7 h before 21:00
        assert_eq!(
            before_night(far, &night_range(), true),
            BeforeNight::Sleep { wake: at(20, 50), hours: 7 }
        );
        assert_eq!(before_night(far, &night_range(), false), BeforeNight::Wait, "Pi 4 without wake alarm");
        let near = at(19, 30); // 1 h 30 before: not worth a power cycle
        assert_eq!(before_night(near, &night_range(), true), BeforeNight::Wait);
    }

    #[test]
    fn wake_up_is_ten_minutes_before_the_next_night() {
        assert_eq!(wake_before(at(7, 0), NaiveTime::from_hms_opt(21, 0, 0).unwrap()), at(20, 50));
        // After tonight's start: tomorrow's
        assert_eq!(
            wake_before(at(22, 0), NaiveTime::from_hms_opt(21, 0, 0).unwrap()),
            Local.with_ymd_and_hms(2026, 1, 16, 20, 50, 0).unwrap()
        );
    }

    #[test]
    fn storage_stops_before_the_disk_is_full() {
        assert_eq!(storage_verdict(MIN_FREE_BYTES - 1, StorageStatus::Ok), StorageVerdict::StopNearlyFull);
        assert_eq!(storage_verdict(MIN_FREE_BYTES, StorageStatus::Critical), StorageVerdict::StopCritical);
        assert_eq!(storage_verdict(MIN_FREE_BYTES, StorageStatus::Warning), StorageVerdict::Continue);
        assert_eq!(storage_verdict(u64::MAX, StorageStatus::Ok), StorageVerdict::Continue);
    }

    #[test]
    fn aurora_needs_consecutive_frames() {
        let mut c = AuroraConfirmation::new(3);
        assert!(!c.observe(true));
        assert!(!c.observe(true));
        assert!(!c.observe(false), "a gap starts again");
        assert_eq!(c.hits(), 0);
        assert!(!c.observe(true));
        assert!(!c.observe(true));
        assert!(c.observe(true));
        assert_eq!(c.hits(), 3);
    }

    #[test]
    fn camera_pause_after_five_failures_in_a_row() {
        let mut f = CameraFailures::default();
        for n in 1..MAX_CAMERA_FAILURES {
            assert_eq!(f.failure(), n);
            assert!(!f.needs_pause());
        }
        f.success();
        assert_eq!(f.failure(), 1, "a success resets the count");
        for _ in 1..MAX_CAMERA_FAILURES {
            f.failure();
        }
        assert!(f.needs_pause());
        f.reset();
        assert!(!f.needs_pause());
    }

    #[test]
    fn safe_mode_captures_at_once_filter_mode_watches() {
        assert_eq!(first_phase(true), Phase::Run);
        assert_eq!(first_phase(false), Phase::Watch);
    }

    #[test]
    fn raw_is_only_read_out_when_saved() {
        assert!(!captures_raw_now(Phase::Watch, OutputFormat::RawDng), "watching: JPEG only");
        assert!(captures_raw_now(Phase::Run, OutputFormat::RawAndJpg));
        assert!(!captures_raw_now(Phase::Run, OutputFormat::Jpg));
    }

    #[test]
    fn aurora_raw_is_kept_during_the_aurora_and_ten_minutes_after() {
        let now = at(23, 0);
        let hold = chrono::Duration::seconds(AURORA_RAW_HOLD_SECS);
        assert!(!keeps_raw(OutputFormat::JpgAuroraRaw, None, now), "no aurora yet");
        assert!(keeps_raw(OutputFormat::JpgAuroraRaw, Some(now), now));
        assert!(keeps_raw(OutputFormat::JpgAuroraRaw, Some(now - hold), now));
        assert!(!keeps_raw(OutputFormat::JpgAuroraRaw, Some(now - hold - chrono::Duration::seconds(1)), now));
        assert!(keeps_raw(OutputFormat::RawDng, None, now));
        assert!(!keeps_raw(OutputFormat::Jpg, Some(now), now));
    }

    #[test]
    fn run_pause_follows_the_interval_or_one_second_back_to_back() {
        assert_eq!(run_pause(10, Duration::from_secs(3)), Some(Duration::from_secs(10)));
        assert_eq!(run_pause(0, Duration::from_millis(300)), Some(Duration::from_millis(700)), "camera answering instantly");
        assert_eq!(run_pause(0, Duration::from_secs(8)), None, "long exposure: next photo at once");
    }

    #[test]
    fn frame_number_is_logged_for_saved_frames_only() {
        let config = AppConfig::default();
        let exposure = ExposureSettings::new(800, 2_000_000);
        let detection = DetectionResult::negative();
        let event = |phase| night_event(NightEventInput {
            timestamp: "2026-01-15T23:00:00".into(),
            config: &config,
            mode: "FILTER",
            phase,
            exposure: &exposure,
            detection: &detection,
            consecutive_hits: 2,
            frame_number: 42,
            capture_ms: 1500,
        });
        let run = event(Phase::Run);
        assert_eq!(run.frame_number, Some(42));
        assert_eq!((run.iso, run.exposure_us, run.consecutive_hits, run.capture_ms), (800, 2_000_000, 2, Some(1500)));
        assert_eq!(run.roi_excluded_percent, 100 - config.detection.roi_top_percent);
        assert_eq!(event(Phase::Watch).frame_number, None);
    }

    #[tokio::test]
    async fn exposure_lock_freezes_the_exposure_in_run_only() {
        use crate::adapters::pc::{CameraMock, SkyPattern};
        let mut config = AppConfig::default();
        config.exposure.lock_in_run = true;
        let frame = CameraMock::with_pattern(SkyPattern::Dark)
            .capture_jpg(&ExposureSettings::new(800, 2_000_000), Path::new("/tmp"))
            .await
            .unwrap();
        let night = |c: &AppConfig| NightLoop {
            sm: StateMachine::new(c.detection.consecutive_required),
            exposure: ExposureController::from_config(&c.exposure),
            detector: AuroraDetector::from_config(&c.detection),
            confirmation: AuroraConfirmation::new(c.detection.consecutive_required),
            stacker: None,
            iterations: 0,
            frame_number: 0,
            stats: NightStats::new(),
            last_aurora_at: None,
            failures: CameraFailures::default(),
            window: NightWindow::new(Some(1.0), night_range(), at(21, 0)),
            wait_iters: 0,
        };

        let mut locked = night(&config);
        let before = locked.exposure.current();
        locked.analyze(&frame, &config, Phase::Run);
        assert_eq!(locked.exposure.current(), before, "RUN with the lock: frozen");

        let mut watching = night(&config);
        let mut reference = ExposureController::from_config(&config.exposure);
        let hist = compute_histogram(crop_roi(&frame.data, frame.width, frame.height, config.detection.roi_top_percent));
        let expected = reference.update(&hist, Phase::Watch);
        assert_ne!(expected, before, "the test frame must move the exposure");
        watching.analyze(&frame, &config, Phase::Watch);
        assert_eq!(watching.exposure.current(), expected, "WATCH: metered on the sky ROI");
    }
}
