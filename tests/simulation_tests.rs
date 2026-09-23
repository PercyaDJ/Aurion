//! End-to-end night simulations: the real orchestrator runs against the PC
//! mocks with a virtual clock (`start_paused`), so hours of capture take a
//! few seconds and every scenario is deterministic.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use aurion::adapters::pc::{CameraMock, NetworkMock, SkyPattern, StorageMock, SystemMock, TokioClock};
use aurion::core::config::AppConfig;
use aurion::core::models::{OutputFormat, Phase};
use aurion::core::orchestrator::Orchestrator;
use aurion::ports::clock::ClockPort;
use aurion::ports::network::NetworkApPort;
use aurion::ports::storage::StoragePort;
use aurion::web::{AppState, Paths};
use chrono::{TimeZone, Utc};

struct Night {
    _dir: tempfile::TempDir,
    capture: PathBuf,
    state: AppState,
    system: SystemMock,
}

fn setup(configure: impl FnOnce(&mut AppConfig)) -> (Night, AppConfig) {
    let dir = tempfile::tempdir().unwrap();
    let capture = dir.path().join("capture");
    std::fs::create_dir_all(&capture).unwrap();
    let mut config = AppConfig::default();
    config.storage.mount_point = capture.to_string_lossy().to_string();
    config.capture.output_format = OutputFormat::Jpg;
    config.capture.capture_interval_secs = 10;
    config.capture.watch_interval_secs = 60;
    config.time_range.duration_hours = Some(0.5);
    configure(&mut config);
    config.validate().expect("test config must be valid");

    let state = AppState::with_paths(config.clone(), Paths::from_config_dir(&dir.path().join("config")))
        .with_system_actions(false);
    (Night { _dir: dir, capture, state, system: SystemMock::new() }, config)
}

fn clock() -> Arc<TokioClock> {
    Arc::new(TokioClock::new(Utc.with_ymd_and_hms(2026, 1, 15, 21, 0, 0).unwrap()))
}

fn images(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.starts_with("aurora_"))
        .collect();
    v.sort();
    v
}

fn session_events(capture: &Path) -> Vec<serde_json::Value> {
    let sessions = capture.join("sessions");
    let dir = std::fs::read_dir(&sessions).unwrap().next().expect("one session").unwrap().path();
    std::fs::read_to_string(dir.join("event.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

async fn run_night(night: &Night, camera: CameraMock, storage: StorageMock, clock: Arc<dyn ClockPort>) {
    storage.mount().await.unwrap();
    *night.state.phase.write().await = Phase::Disconnect; // user clicked "Déconnexion"
    let orch = Orchestrator::new(night.state.clone(), camera, storage, night.system.clone()).with_clock(clock);
    tokio::time::timeout(std::time::Duration::from_secs(48 * 3600), orch.run())
        .await
        .expect("the night must end by itself")
        .unwrap();
}

#[tokio::test(start_paused = true)]
async fn safe_mode_captures_all_night_then_powers_off() {
    let (night, _) = setup(|_| {});
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock()).await;

    let files = images(&night.capture);
    // 30 min at one frame every 10 s ≈ 180 frames
    assert!((170..=182).contains(&files.len()), "got {} frames", files.len());
    assert!(files.iter().all(|f| f.ends_with(".jpg") && !f.contains("_AURORA")));
    // Every file is a real JPEG with its thumbnail
    let first = std::fs::read(night.capture.join(&files[0])).unwrap();
    assert!(image::load_from_memory(&first).is_ok());
    assert!(night.capture.join("thumbs").join(&files[0]).exists());
    // One NDJSON event per frame
    assert_eq!(session_events(&night.capture).len(), files.len());
    assert_eq!(night.state.current_phase().await, Phase::Shutdown);
    assert_eq!(night.system.shutdown_count(), 1, "the Pi powers off at the end of the night");
}

#[tokio::test(start_paused = true)]
async fn filter_mode_starts_capturing_after_confirmed_aurora() {
    let (night, _) = setup(|c| {
        c.detection.detection_capture_enabled = true;
        c.detection.consecutive_required = 2;
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Intermittent), storage, clock()).await;

    let files = images(&night.capture);
    assert!(!files.is_empty(), "aurora must trigger the RUN phase");
    assert!(files.iter().any(|f| f.contains("_AURORA")), "frames with aurora are tagged");
    let events = session_events(&night.capture);
    let first_run = events.iter().position(|e| e["phase"] == "Run").expect("Run phase logged");
    assert!(first_run >= 2, "at least two WATCH frames before RUN");
    assert!(events[..first_run].iter().all(|e| e["phase"] == "Watch"));
    assert_eq!(night.system.shutdown_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn filter_mode_dark_night_saves_nothing() {
    let (night, _) = setup(|c| c.detection.detection_capture_enabled = true);
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock()).await;

    assert!(images(&night.capture).is_empty(), "no aurora → no image");
    let events = session_events(&night.capture);
    // Watch interval 60 s during 30 min
    assert!((25..=31).contains(&events.len()), "{} watch frames", events.len());
    assert!(events.iter().all(|e| e["phase"] == "Watch" && e["aurora_detected"] == false));
    // Non-regression: the session of a night without aurora keeps its log
    // (the gallery used to delete such folders).
    assert!(night.capture.join("sessions").read_dir().unwrap().count() == 1);
}

#[tokio::test(start_paused = true)]
async fn time_range_mode_waits_for_start_and_stops_at_end() {
    let clock = clock();
    let now = clock.now_local();
    let (night, _) = setup(|c| {
        c.time_range.duration_hours = None;
        c.time_range.start = (now + chrono::Duration::minutes(20)).time();
        c.time_range.end = (now + chrono::Duration::minutes(50)).time();
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock.clone()).await;

    let files = images(&night.capture);
    assert!((170..=182).contains(&files.len()), "30 min window → ~180 frames, got {}", files.len());
    let elapsed = clock.now_local() - now;
    assert!(elapsed >= chrono::Duration::minutes(50) && elapsed < chrono::Duration::minutes(53), "ended after {:?}", elapsed);
    // No frame taken before the start of the window
    let first_ts = chrono::NaiveDateTime::parse_from_str(&files[0][7..22], "%Y%m%d_%H%M%S").unwrap();
    assert!(first_ts >= (now + chrono::Duration::minutes(20)).naive_local() - chrono::Duration::seconds(1));
}

#[tokio::test(start_paused = true)]
async fn camera_failures_are_retried_and_the_night_continues() {
    let (night, _) = setup(|_| {});
    let storage = StorageMock::new(night.capture.clone());
    // Calibration (3) + 7 more captures fail → one 5 min recovery pause
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark).fail_next(10), storage, clock()).await;
    let files = images(&night.capture);
    assert!(files.len() > 100, "capture must resume after camera errors, got {}", files.len());
    let logs = night.state.log_buffer.read().await.join("\n");
    assert!(logs.contains("erreurs consecutives"), "the recovery pause is logged");
}

#[tokio::test(start_paused = true)]
async fn missing_camera_never_crashes() {
    let (night, _) = setup(|c| c.time_range.duration_hours = Some(0.2));
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::disconnected(), storage, clock()).await;
    assert!(images(&night.capture).is_empty());
    assert_eq!(night.system.shutdown_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn full_disk_stops_the_session_safely() {
    let (night, _) = setup(|c| c.time_range.duration_hours = Some(8.0));
    let storage = StorageMock::will_fill(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock()).await;
    let files = images(&night.capture);
    assert!(files.len() < 8 * 360, "must stop before the end of the 8 h timer");
    let logs = night.state.log_buffer.read().await.join("\n");
    assert!(logs.contains("Stockage critique"), "{}", logs);
    assert_eq!(night.system.shutdown_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn raw_mode_saves_dng_files() {
    let (night, _) = setup(|c| {
        c.capture.output_format = OutputFormat::RawAndJpg;
        c.time_range.duration_hours = Some(0.1);
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock()).await;
    let files = images(&night.capture);
    let dng = files.iter().filter(|f| f.ends_with(".dng")).count();
    let jpg = files.iter().filter(|f| f.ends_with(".jpg")).count();
    assert!(dng > 0 && dng == jpg, "RAW+JPG pairs: {} dng / {} jpg", dng, jpg);
}

#[tokio::test(start_paused = true)]
async fn hot_reload_of_capture_interval() {
    let (night, _) = setup(|c| c.time_range.duration_hours = Some(1.0));
    let storage = StorageMock::new(night.capture.clone());
    let state = night.state.clone();
    // After 30 min the user slows the timelapse down to one frame per minute
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30 * 60)).await;
        state.config.write().await.capture.capture_interval_secs = 60;
    });
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock()).await;
    let n = images(&night.capture).len();
    // ~180 frames in the first half hour + ~30 in the second
    assert!((200..=220).contains(&n), "got {} frames", n);
}

#[tokio::test(start_paused = true)]
async fn shutdown_signal_before_disconnect_exits_immediately() {
    let (night, _) = setup(|_| {});
    let storage = StorageMock::new(night.capture.clone());
    *night.state.phase.write().await = Phase::Shutdown;
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock());
    tokio::time::timeout(std::time::Duration::from_secs(5), orch.run()).await.unwrap().unwrap();
    assert!(images(&night.capture).is_empty());
    assert_eq!(night.system.shutdown_count(), 0, "a service stop must not power the Pi off");
}

#[tokio::test(start_paused = true)]
async fn hotspot_is_switched_off_when_the_night_starts() {
    let (night, _) = setup(|c| c.time_range.duration_hours = Some(0.05));
    let network = NetworkMock::new();
    network.start_ap("Aurion", "aurora2024", 6).await.unwrap();
    let storage = StorageMock::new(night.capture.clone());
    storage.mount().await.unwrap();
    *night.state.phase.write().await = Phase::Disconnect;

    struct Shared(Arc<NetworkMock>);
    #[async_trait::async_trait]
    impl NetworkApPort for Shared {
        async fn start_ap(&self, s: &str, p: &str, c: u32) -> Result<(), aurion::ports::NetworkError> { self.0.start_ap(s, p, c).await }
        async fn stop_ap(&self) -> Result<(), aurion::ports::NetworkError> { self.0.stop_ap().await }
        fn is_ap_active(&self) -> bool { self.0.is_ap_active() }
    }
    let shared = Arc::new(network);
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock())
        .with_network(Box::new(Shared(shared.clone())));
    orch.run().await.unwrap();
    assert!(!shared.is_ap_active());
}
