use std::path::PathBuf;
use chrono::{TimeZone, Utc};
use tracing::info;

use crate::core::config::AppConfig;
use crate::core::detection::AuroraDetector;
use crate::core::exposure::{ExposureController, compute_histogram};
use crate::core::models::{Phase, TimeRange};
use crate::core::state_machine::{Event, StateMachine};
use crate::adapters::pc::{CameraMock, ClockMock, NetworkMock, StorageMock, SystemMock};
use crate::ports::camera::CameraPort;
use crate::ports::storage::StoragePort;
use crate::ports::clock::ClockPort;
use crate::ports::network::NetworkApPort;
use crate::ports::system::SystemPort;

/// Run a full night simulation using PC mocks.
pub async fn run_simulation() -> anyhow::Result<()> {
    info!("=== Aurion Simulation ===");
    info!("Simulating a complete night cycle...\n");

    // Load or create default config
    let config = AppConfig::default();

    // Create mocks
    // 21:25 → user disconnects at 21:30, inside the default 21:00–06:00 window
    let start_time = Utc.with_ymd_and_hms(2025, 3, 23, 21, 25, 0).unwrap();
    let clock = ClockMock::accelerated(start_time, 60.0);
    let camera = CameraMock::synthetic();
    let storage = StorageMock::new(PathBuf::from("./output"));
    let network = NetworkMock::new();
    let system = SystemMock::new();

    // Initialize state machine
    let mut sm = StateMachine::new(config.detection.consecutive_required);
    let mut detector = AuroraDetector::from_config(&config.detection);
    let mut exposure_ctrl = ExposureController::from_config(&config.exposure);

    let time_range = TimeRange {
        start: config.time_range.start,
        end: config.time_range.end,
    };

    // ─── BOOT ──────────────────────────────────────────────
    info!("[BOOT] Mounting storage...");
    storage.mount().await?;
    info!("[BOOT] Storage mounted: {}", storage.info()?.display_summary());

    sm.transition(Event::BootComplete)?;
    info!("[→ ARM] System ready\n");

    // ─── ARM ───────────────────────────────────────────────
    info!("[ARM] Starting AP...");
    network.start_ap(&config.network.ssid, &config.network.password, config.network.channel).await?;
    info!("[ARM] Web server would start on port {}...", config.web.port);
    info!("[ARM] Waiting for user interaction (simulated)...");

    // Simulate: user configures and disconnects after 5 minutes
    clock.advance_minutes(5);
    info!("[ARM] User clicks 'Déconnexion'\n");

    // ─── DISCONNECT ────────────────────────────────────────
    sm.transition(Event::UserDisconnect)?;
    info!("[DISCONNECT] 15s countdown...");
    clock.advance_secs(15);
    network.stop_ap().await?;
    info!("[DISCONNECT] AP stopped, config locked");

    sm.transition(Event::DisconnectTimerExpired)?;
    info!("[→ CALIBRATION]\n");

    // ─── CALIBRATION ───────────────────────────────────────
    info!("[CALIBRATION] Capturing test image...");
    let exposure = exposure_ctrl.current();
    let tmp_dir = PathBuf::from("./output");
    let frame = camera.capture_jpg(&exposure, &tmp_dir).await?;
    let histogram = compute_histogram(&frame.data);
    let calibrated = exposure_ctrl.update(&histogram, Phase::Calibration);
    info!(
        "[CALIBRATION] Result: ISO {} / Shutter {} µs",
        calibrated.iso, calibrated.shutter_us
    );

    sm.transition(Event::CalibrationDone)?;
    info!("[→ WATCH]\n");

    // ─── WATCH ─────────────────────────────────────────────
    info!("[WATCH] Starting aurora surveillance...");
    let mut frame_count = 0u32;
    let max_watch_frames = 50; // Limit for simulation

    loop {
        if frame_count >= max_watch_frames {
            info!("[WATCH] Simulation limit reached");
            break;
        }

        // Check time range
        let now_time = clock.now().time();
        if !time_range.contains(now_time) {
            info!("[WATCH] Time range ended at {}", clock.now().format("%H:%M:%S"));
            sm.transition(Event::TimeRangeEnded)?;
            break;
        }

        // Capture
        let exposure = exposure_ctrl.current();
        let tmp_dir = PathBuf::from("./output");
        let frame = camera.capture_jpg(&exposure, &tmp_dir).await?;
        let result = detector.analyze(&frame.data, frame.width, frame.height);

        info!(
            "[WATCH] Frame #{} @ {} | green={:.1} red={:.1} lum={:.1} score={:.2} → {}",
            frame_count,
            clock.now().format("%H:%M:%S"),
            result.green_score,
            result.red_score,
            result.luminosity,
            result.aurora_score,
            if result.detected { "DETECTED" } else { "clear" }
        );

        // State machine event
        let event = if result.detected {
            Event::AuroraDetected
        } else {
            Event::NoAuroraDetected
        };

        let new_phase = sm.transition(event)?;
        if new_phase == Phase::Run {
            info!("[→ RUN] Aurora confirmed!\n");
            break;
        }

        frame_count += 1;
        clock.advance_secs(config.capture.watch_interval_secs as i64);
    }

    // ─── RUN (if entered) ──────────────────────────────────
    if sm.phase() == Phase::Run {
        info!("[RUN] Continuous capture mode...");
        let mut run_frames = 0u32;
        let max_run_frames = 20;

        loop {
            if run_frames >= max_run_frames {
                info!("[RUN] Simulation limit reached");
                break;
            }

            let now_time = clock.now().time();
            if !time_range.contains(now_time) {
                info!("[RUN] Time range ended at {}", clock.now().format("%H:%M:%S"));
                sm.transition(Event::TimeRangeEnded)?;
                break;
            }

            let exposure = exposure_ctrl.current();
            let tmp_dir = PathBuf::from("./output");
            let frame = camera.capture_jpg(&exposure, &tmp_dir).await?;

            // Auto-adjust exposure
            let histogram = compute_histogram(&frame.data);
            exposure_ctrl.update(&histogram, Phase::Run);

            // Save frame (encode the RGB pixels as a real JPEG)
            let filename = format!("aurora_{}_{:05}.jpg", clock.now().format("%Y%m%d_%H%M%S"), run_frames);
            let img = image::RgbImage::from_raw(frame.width, frame.height, frame.data.clone())
                .ok_or_else(|| anyhow::anyhow!("invalid frame size"))?;
            let mut jpg = std::io::Cursor::new(Vec::new());
            img.write_to(&mut jpg, image::ImageFormat::Jpeg)?;
            storage.save_file(&filename, jpg.get_ref()).await?;

            info!(
                "[RUN] Frame #{} saved | ISO {} / {}µs | {}",
                run_frames,
                exposure.iso,
                exposure.shutter_us,
                clock.now().format("%H:%M:%S"),
            );

            run_frames += 1;
            clock.advance_secs(exposure.shutter_us as i64 / 1_000_000);
        }
    }

    // ─── SHUTDOWN ──────────────────────────────────────────
    if sm.phase() != Phase::Shutdown {
        // Force shutdown if not already there
        sm.set_phase(Phase::Shutdown);
    }

    info!("\n[SHUTDOWN] Syncing files...");
    storage.sync().await?;
    info!("[SHUTDOWN] Unmounting storage...");
    storage.unmount().await?;
    info!("[SHUTDOWN] System shutdown...");
    system.shutdown().await?;

    info!("\n=== Simulation Complete ===");
    Ok(())
}
