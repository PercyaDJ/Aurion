use std::path::PathBuf;
use chrono::{TimeZone, Utc, NaiveTime};

use aurion::core::config::AppConfig;
use aurion::core::detection::AuroraDetector;
use aurion::core::exposure::ExposureController;
use aurion::core::models::{Phase, TimeRange};
use aurion::core::state_machine::{Event, StateMachine};
use aurion::adapters::pc::{CameraMock, ClockMock, StorageMock, NetworkMock, SystemMock};
use aurion::ports::camera::CameraPort;
use aurion::ports::storage::StoragePort;
use aurion::ports::clock::ClockPort;
use aurion::ports::network::NetworkApPort;
use aurion::ports::system::SystemPort;

/// Helper: run the Watch loop until a phase change or frame limit.
/// Returns the final phase.
async fn run_watch_loop(
    sm: &mut StateMachine,
    detector: &mut AuroraDetector,
    exposure_ctrl: &mut ExposureController,
    camera: &CameraMock,
    clock: &ClockMock,
    time_range: &TimeRange,
    watch_interval_secs: i64,
    max_frames: u32,
) -> Phase {
    let mut frame_count = 0u32;

    loop {
        if frame_count >= max_frames {
            break sm.phase();
        }

        let now_time = clock.now().time();
        if !time_range.contains(now_time) {
            let _ = sm.transition(Event::TimeRangeEnded);
            break sm.phase();
        }

        let exposure = exposure_ctrl.current();
        let frame = camera.capture_jpg(&exposure).await.unwrap();
        let result = detector.analyze(&frame.data, frame.width, frame.height);

        let event = if result.detected {
            Event::AuroraDetected
        } else {
            Event::NoAuroraDetected
        };

        let new_phase = sm.transition(event).unwrap();
        if new_phase == Phase::Run {
            break Phase::Run;
        }

        frame_count += 1;
        clock.advance_secs(watch_interval_secs);
    }
}

// ─────────────────────────────────────────────────────────────
// Integration Test 1: Night without aurora → never enters RUN
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_night_without_aurora_never_run() {
    let config = AppConfig::default();
    let mut sm = StateMachine::new(config.detection.consecutive_required);

    // Use very high thresholds so synthetic frames never trigger detection
    let mut detector = AuroraDetector::new(65, 200.0, 200.0, 200.0, 200.0, 1.0, 1.0, 0.7, false, 240.0);
    let mut exposure_ctrl = ExposureController::from_config(&config.exposure);

    // Start at 21:00, end at 06:00 → 9-hour night
    let start_time = Utc.with_ymd_and_hms(2025, 3, 23, 21, 0, 0).unwrap();
    let clock = ClockMock::new(start_time);
    let camera = CameraMock::synthetic();

    let time_range = TimeRange {
        start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
    };

    // Boot → Arm → Disconnect → Calibration → Watch
    sm.transition(Event::BootComplete).unwrap();
    sm.transition(Event::UserDisconnect).unwrap();
    sm.transition(Event::DisconnectTimerExpired).unwrap();
    sm.transition(Event::CalibrationDone).unwrap();
    assert_eq!(sm.phase(), Phase::Watch);

    // Run watch loop — should NEVER enter RUN
    let final_phase = run_watch_loop(
        &mut sm,
        &mut detector,
        &mut exposure_ctrl,
        &camera,
        &clock,
        &time_range,
        60, // 1 minute intervals
        600, // max 600 frames (10 hours worth at 1min interval)
    )
    .await;

    // Should end in Shutdown (time range expired), never RUN
    assert_eq!(final_phase, Phase::Shutdown);
}

// ─────────────────────────────────────────────────────────────
// Integration Test 2: Persistent aurora → RUN triggered
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_persistent_aurora_triggers_run() {
    let config = AppConfig::default();
    let mut sm = StateMachine::new(2); // 2 consecutive detections needed

    // Use very low thresholds so synthetic aurora frames always trigger
    let mut detector = AuroraDetector::new(65, 1.0, 1.0, 1.0, 1.0, 0.1, 0.1, 0.05, false, 240.0);
    let mut exposure_ctrl = ExposureController::from_config(&config.exposure);

    let start_time = Utc.with_ymd_and_hms(2025, 3, 23, 23, 0, 0).unwrap();
    let clock = ClockMock::new(start_time);
    let camera = CameraMock::synthetic();

    let time_range = TimeRange {
        start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
    };

    // Boot → Arm → Disconnect → Calibration → Watch
    sm.transition(Event::BootComplete).unwrap();
    sm.transition(Event::UserDisconnect).unwrap();
    sm.transition(Event::DisconnectTimerExpired).unwrap();
    sm.transition(Event::CalibrationDone).unwrap();

    let final_phase = run_watch_loop(
        &mut sm,
        &mut detector,
        &mut exposure_ctrl,
        &camera,
        &clock,
        &time_range,
        60,
        100,
    )
    .await;

    // Should have entered RUN (synthetic camera produces aurora frames periodically)
    assert_eq!(final_phase, Phase::Run);
}

// ─────────────────────────────────────────────────────────────
// Integration Test 3: False positives (headlights) → no RUN
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_false_positives_no_run() {
    let _config = AppConfig::default();
    let mut sm = StateMachine::new(2);

    // Standard thresholds — headlights should not trigger
    let mut detector = AuroraDetector::new(65, 15.0, 10.0, 8.0, 30.0, 1.0, 1.0, 0.7, true, 240.0);

    // Manually test with headlight-like data
    // Concentrated bright spot → spatial spread check should reject it
    let width = 100u32;
    let height = 100u32;
    let mut img_data = vec![5u8; (width * height * 3) as usize]; // Dark frame

    // Add a single bright spot (simulating headlights)
    let cx = 50;
    let cy = 25;
    for y in (cy - 3)..=(cy + 3) {
        for x in (cx - 3)..=(cx + 3) {
            let idx = ((y * width + x) * 3) as usize;
            img_data[idx] = 255;     // R
            img_data[idx + 1] = 255; // G
            img_data[idx + 2] = 255; // B
        }
    }

    // First detection attempt
    let result1 = detector.analyze(&img_data, width, height);
    assert!(!result1.detected, "Headlight should not be detected as aurora");

    // Second attempt with same data
    let result2 = detector.analyze(&img_data, width, height);
    assert!(!result2.detected, "Headlight should still not be detected");

    // State machine should stay in Watch
    sm.transition(Event::BootComplete).unwrap();
    sm.transition(Event::UserDisconnect).unwrap();
    sm.transition(Event::DisconnectTimerExpired).unwrap();
    sm.transition(Event::CalibrationDone).unwrap();

    sm.transition(Event::NoAuroraDetected).unwrap();
    sm.transition(Event::NoAuroraDetected).unwrap();
    assert_eq!(sm.phase(), Phase::Watch);
}

// ─────────────────────────────────────────────────────────────
// Integration Test 4: Storage full → SAFE MODE → SHUTDOWN
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_storage_full_safe_mode() {
    let config = AppConfig::default();
    let mut sm = StateMachine::new(2);

    let storage = StorageMock::nearly_full(PathBuf::from("./output/test_safe_mode"));
    storage.mount().await.unwrap();

    let info = storage.info().unwrap();
    let status = info.status(
        config.storage.warning_percent as f64,
        config.storage.critical_percent as f64,
    );

    // Nearly full → should be Critical (< 5%)
    assert_eq!(status, aurion::core::models::StorageStatus::Critical);

    // Boot → Watch, then storage critical
    sm.transition(Event::BootComplete).unwrap();
    sm.transition(Event::UserDisconnect).unwrap();
    sm.transition(Event::DisconnectTimerExpired).unwrap();
    sm.transition(Event::CalibrationDone).unwrap();
    assert_eq!(sm.phase(), Phase::Watch);

    // Storage critical → SAFE MODE
    sm.transition(Event::StorageCritical).unwrap();
    assert_eq!(sm.phase(), Phase::SafeMode);

    // SAFE MODE → SHUTDOWN
    sm.transition(Event::SafeModeComplete).unwrap();
    assert_eq!(sm.phase(), Phase::Shutdown);

    storage.unmount().await.unwrap();
}

// ─────────────────────────────────────────────────────────────
// Integration Test 5: End of time range → SHUTDOWN
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_end_of_time_range_shutdown() {
    let config = AppConfig::default();
    let mut sm = StateMachine::new(2);

    // Start at 05:50, time range ends at 06:00
    let start_time = Utc.with_ymd_and_hms(2025, 3, 24, 5, 50, 0).unwrap();
    let clock = ClockMock::new(start_time);
    let camera = CameraMock::synthetic();

    // High thresholds → no detection
    let mut detector = AuroraDetector::new(65, 200.0, 200.0, 200.0, 200.0, 1.0, 1.0, 0.7, false, 240.0);
    let mut exposure_ctrl = ExposureController::from_config(&config.exposure);

    let time_range = TimeRange {
        start: NaiveTime::from_hms_opt(21, 0, 0).unwrap(),
        end: NaiveTime::from_hms_opt(6, 0, 0).unwrap(),
    };

    // Boot → Watch
    sm.transition(Event::BootComplete).unwrap();
    sm.transition(Event::UserDisconnect).unwrap();
    sm.transition(Event::DisconnectTimerExpired).unwrap();
    sm.transition(Event::CalibrationDone).unwrap();

    // Watch loop: 10 minutes left, 60s intervals → ~10 frames then shutdown
    let final_phase = run_watch_loop(
        &mut sm,
        &mut detector,
        &mut exposure_ctrl,
        &camera,
        &clock,
        &time_range,
        60,
        100,
    )
    .await;

    assert_eq!(final_phase, Phase::Shutdown);
}

// ─────────────────────────────────────────────────────────────
// Integration Test 6: RUN continues to end of time range
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_run_continues_to_end_of_time_range() {
    let mut sm = StateMachine::new(2);

    // Already in RUN phase
    sm.set_phase(Phase::Run);

    // Time range ends
    let result = sm.transition(Event::TimeRangeEnded).unwrap();
    assert_eq!(result, Phase::Shutdown);

    // Verify: RUN does NOT go back to Watch
    let mut sm2 = StateMachine::new(2);
    sm2.set_phase(Phase::Run);
    assert!(sm2.transition(Event::NoAuroraDetected).is_err());
}

// ─────────────────────────────────────────────────────────────
// Integration Test 7: Full lifecycle Boot → Shutdown
// ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_full_lifecycle() {
    let _config = AppConfig::default();
    let mut sm = StateMachine::new(2);

    let camera = CameraMock::synthetic();
    let storage = StorageMock::new(PathBuf::from("./output/test_lifecycle"));
    let network = NetworkMock::new();
    let system = SystemMock::new();

    // BOOT
    assert_eq!(sm.phase(), Phase::Boot);
    storage.mount().await.unwrap();
    assert!(storage.is_available());
    sm.transition(Event::BootComplete).unwrap();

    // ARM
    assert_eq!(sm.phase(), Phase::Arm);
    network.start_ap("Aurion", "test", 6).await.unwrap();
    assert!(network.is_ap_active());

    // DISCONNECT
    sm.transition(Event::UserDisconnect).unwrap();
    assert_eq!(sm.phase(), Phase::Disconnect);
    network.stop_ap().await.unwrap();
    assert!(!network.is_ap_active());
    sm.transition(Event::DisconnectTimerExpired).unwrap();

    // CALIBRATION
    assert_eq!(sm.phase(), Phase::Calibration);
    let exp = aurion::core::models::ExposureSettings::new(800, 5_000_000);
    let frame = camera.capture_jpg(&exp).await.unwrap();
    assert!(frame.width > 0);
    sm.transition(Event::CalibrationDone).unwrap();

    // WATCH
    assert_eq!(sm.phase(), Phase::Watch);

    // Simulate 2 positive detections → RUN
    sm.transition(Event::AuroraDetected).unwrap();
    assert_eq!(sm.phase(), Phase::Watch);
    sm.transition(Event::AuroraDetected).unwrap();
    assert_eq!(sm.phase(), Phase::Run);

    // RUN → time range ends
    sm.transition(Event::TimeRangeEnded).unwrap();
    assert_eq!(sm.phase(), Phase::Shutdown);

    // SHUTDOWN
    storage.sync().await.unwrap();
    storage.unmount().await.unwrap();
    system.shutdown().await.unwrap();
}
