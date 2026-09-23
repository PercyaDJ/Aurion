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

/// Images of every night folder (`sessions/<night>/JPG|RAW`), by name.
fn images(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = image_paths(dir)
        .into_iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

fn image_paths(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(nights) = std::fs::read_dir(dir.join("sessions")) else { return out };
    for night in nights.filter_map(|e| e.ok()) {
        for sub in ["JPG", "RAW"] {
            let Ok(rd) = std::fs::read_dir(night.path().join(sub)) else { continue };
            out.extend(rd.filter_map(|e| e.ok()).map(|e| e.path())
                .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("aurora_")));
        }
    }
    assert!(std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok())
        .all(|e| !e.file_name().to_string_lossy().starts_with("aurora_")), "nothing written at the root of the key");
    out
}

/// Path of an image in its night folder.
fn image_path(dir: &Path, name: &str) -> PathBuf {
    image_paths(dir).into_iter().find(|p| p.file_name().unwrap() == name).expect("image in a night folder")
}

/// Capture-time thumbnail of an image.
fn thumb_path(dir: &Path, name: &str) -> PathBuf {
    image_path(dir, name).parent().unwrap().parent().unwrap().join("thumbs").join(name)
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
    let first = std::fs::read(image_path(&night.capture, &files[0])).unwrap();
    assert!(image::load_from_memory(&first).is_ok());
    assert!(thumb_path(&night.capture, &files[0]).exists());
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

// ─── Photo quality: Pi-like captures (full JPEG + EXIF thumbnail) ────

fn saved_jpeg(capture: &Path, name: &str) -> Vec<u8> {
    std::fs::read(image_path(capture, name)).unwrap()
}

#[tokio::test(start_paused = true)]
async fn pi_captures_are_saved_untouched_with_exif_thumbnail() {
    let (night, _) = setup(|c| {
        c.capture.output_format = OutputFormat::RawAndJpg;
        c.time_range.duration_hours = Some(0.02);
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Aurora).with_jpeg_output(), storage, clock()).await;

    let files = images(&night.capture);
    let jpg = files.iter().find(|f| f.ends_with(".jpg")).expect("a JPEG");
    let data = saved_jpeg(&night.capture, jpg);
    let full = image::load_from_memory(&data).unwrap();
    assert_eq!((full.width(), full.height()), (1280, 960), "full resolution kept");
    let thumb = aurion::core::jpeg::exif_thumbnail(&data).expect("EXIF thumbnail kept");
    // Gallery thumbnail = the EXIF thumbnail itself (no re-encoding)
    assert_eq!(std::fs::read(thumb_path(&night.capture, jpg)).unwrap(), thumb);
    assert!(files.iter().any(|f| f.ends_with(".dng")));
}

#[tokio::test(start_paused = true)]
async fn hot_pixel_correction_on_jpeg_keeps_exif() {
    let (night, _) = setup(|c| {
        c.capture.denoise.hot_pixels = true;
        c.time_range.duration_hours = Some(0.01);
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark).with_jpeg_output(), storage, clock()).await;
    let files = images(&night.capture);
    let data = saved_jpeg(&night.capture, &files[0]);
    assert!(aurion::core::jpeg::exif_thumbnail(&data).is_some(), "EXIF must survive re-encoding");
    let rgb = image::load_from_memory(&data).unwrap().to_rgb8();
    let saturated = rgb.pixels().filter(|p| p.0.iter().all(|&v| v > 240)).count();
    assert!(saturated < aurion::adapters::pc::camera_mock::MOCK_HOT_PIXELS / 5, "{} hot pixels left", saturated);
}

#[tokio::test(start_paused = true)]
async fn stacking_produces_one_extra_image_every_n_frames() {
    let (night, _) = setup(|c| {
        c.capture.denoise.stack_frames = 4;
        c.time_range.duration_hours = Some(0.05); // 3 min → ~18 frames
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Aurora).with_jpeg_output(), storage, clock()).await;
    let files = images(&night.capture);
    let stacks: Vec<_> = files.iter().filter(|f| f.contains("_STACK4")).collect();
    let singles = files.iter().filter(|f| !f.contains("_STACK")).count();
    assert_eq!(stacks.len(), singles / 4, "{} stacks for {} frames", stacks.len(), singles);
    let img = image::load_from_memory(&saved_jpeg(&night.capture, stacks[0])).unwrap();
    assert_eq!(img.width(), 1280);
}

#[tokio::test(start_paused = true)]
async fn exposure_lock_keeps_the_timelapse_flicker_free() {
    let (night, _) = setup(|c| {
        c.exposure.lock_in_run = true;
        c.time_range.duration_hours = Some(0.1);
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Intermittent), storage, clock()).await;
    let events = session_events(&night.capture);
    let run: Vec<_> = events.iter().filter(|e| e["phase"] == "Run").collect();
    assert!(run.len() > 20);
    assert!(run.windows(2).all(|w| w[0]["exposure_us"] == w[1]["exposure_us"] && w[0]["iso"] == w[1]["iso"]),
        "exposure must not change during RUN");
}

#[tokio::test(start_paused = true)]
async fn strongest_auroras_are_ranked_from_the_night_log() {
    let (night, _) = setup(|c| {
        c.capture.output_format = OutputFormat::RawAndJpg;
        c.time_range.duration_hours = Some(0.05);
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Intermittent), storage, clock()).await;
    let events = session_events(&night.capture);
    assert!(events.iter().all(|e| e["frame_number"].is_u64()), "every saved frame is numbered");

    let best = aurion::web::gallery::best_auroras(&night.capture, 5);
    assert!(!best.is_empty() && best.len() <= 5);
    assert!(best.windows(2).all(|w| w[0].score >= w[1].score), "sorted by score");
    assert!(best.iter().all(|b| b.score > 0.0 && b.dng.is_some() && b.jpg.is_some()));
}

// ─── Resume after a power cut ───────────────────────────────

use aurion::core::night::{NightMarker, RESUME_DELAY_SECS};

fn interrupted_marker(night: &Night, clock: &TokioClock, started_hours_ago: f64, duration: f64) -> PathBuf {
    let started = clock.now_local() - chrono::Duration::seconds((started_hours_ago * 3600.0) as i64);
    let path = night.state.paths.night_marker.clone();
    NightMarker::new(started, Some(duration)).save(&path).unwrap();
    path
}

#[tokio::test(start_paused = true)]
async fn interrupted_night_resumes_alone_for_the_remaining_time() {
    let (night, _) = setup(|_| {});
    let clock = clock();
    let marker = interrupted_marker(&night, &clock, 1.0, 2.0); // 1 h left
    let storage = StorageMock::new(night.capture.clone());
    storage.mount().await.unwrap();
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock.clone());
    let run = tokio::spawn(async move { orch.run().await });

    // Nobody touches the phone: the hotspot waits, then the night resumes
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    assert_eq!(night.state.current_phase().await, Phase::Arm);
    let left = night.state.resume_at.read().await.expect("resume pending");
    assert!(left.saturating_duration_since(tokio::time::Instant::now()).as_secs() <= RESUME_DELAY_SECS - 60);

    tokio::time::timeout(std::time::Duration::from_secs(48 * 3600), run).await.unwrap().unwrap().unwrap();
    let files = images(&night.capture);
    // The planned end does not move: 1 h left minus the 5 min wait = 55 min
    // at one frame every 10 s (not the configured 0.5 h, not the full 2 h)
    assert!((310..=332).contains(&files.len()), "got {} frames", files.len());
    assert_eq!(night.system.shutdown_count(), 1);
    assert!(!marker.exists(), "finished night leaves no marker");
}

#[tokio::test(start_paused = true)]
async fn interrupted_night_can_be_cancelled_from_the_phone() {
    let (night, _) = setup(|_| {});
    let clock = clock();
    let marker = interrupted_marker(&night, &clock, 1.0, 6.0);
    let storage = StorageMock::new(night.capture.clone());
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock.clone());
    let run = tokio::spawn(async move { orch.run().await });

    tokio::time::sleep(std::time::Duration::from_secs(30)).await;
    let server = axum_test::TestServer::new(aurion::web::build_router(night.state.clone())).unwrap();
    let pf: serde_json::Value = server.get("/api/preflight").await.json();
    assert!(pf["resume_in_secs"].as_u64().unwrap() > 200, "{}", pf);
    server.post("/api/night/resume/cancel").await.assert_status_ok();
    server.post("/api/night/resume/cancel").await.assert_status(axum::http::StatusCode::CONFLICT);

    tokio::time::sleep(std::time::Duration::from_secs(2 * RESUME_DELAY_SECS)).await;
    assert_eq!(night.state.current_phase().await, Phase::Arm, "back to normal: waits for the user");
    assert!(!marker.exists());
    assert!(images(&night.capture).is_empty());
    run.abort();
}

#[tokio::test(start_paused = true)]
async fn finished_night_marker_is_ignored() {
    let (night, _) = setup(|_| {});
    let clock = clock();
    let marker = interrupted_marker(&night, &clock, 7.0, 6.0); // ended 1 h ago
    let storage = StorageMock::new(night.capture.clone());
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock.clone());
    let run = tokio::spawn(async move { orch.run().await });
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    assert!(night.state.resume_at.read().await.is_none());
    assert!(!marker.exists());
    assert_eq!(night.state.current_phase().await, Phase::Arm);
    run.abort();
}

#[tokio::test(start_paused = true)]
async fn night_marker_exists_during_the_night_only() {
    let (night, _) = setup(|_| {});
    let marker = night.state.paths.night_marker.clone();
    let storage = StorageMock::new(night.capture.clone());
    storage.mount().await.unwrap();
    *night.state.phase.write().await = Phase::Disconnect;
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock());
    let run = tokio::spawn(async move { orch.run().await });
    tokio::time::sleep(std::time::Duration::from_secs(600)).await;
    assert!(marker.exists(), "written when the night starts");
    tokio::time::timeout(std::time::Duration::from_secs(48 * 3600), run).await.unwrap().unwrap().unwrap();
    assert!(!marker.exists(), "removed at the normal end");
}

// ─── Night folders, aurora index, RAW during auroras ────────

#[tokio::test(start_paused = true)]
async fn each_night_has_its_folder_with_raw_jpg_and_aurora_index() {
    let (night, _) = setup(|c| {
        c.capture.output_format = OutputFormat::RawAndJpg;
        c.time_range.duration_hours = Some(0.05);
    });
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Aurora), storage, clock()).await;

    let nights: Vec<PathBuf> = std::fs::read_dir(night.capture.join("sessions")).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(nights.len(), 1);
    let dir = &nights[0];
    let jpg = std::fs::read_dir(dir.join("JPG")).unwrap().count();
    let raw = std::fs::read_dir(dir.join("RAW")).unwrap().count();
    assert!(jpg > 10 && jpg == raw, "{} JPG / {} RAW", jpg, raw);
    assert_eq!(std::fs::read_dir(dir.join("thumbs")).unwrap().count(), jpg);
    let csv = std::fs::read_to_string(dir.join("aurores.csv")).expect("aurora index written at the end of the night");
    assert_eq!(csv.lines().next(), Some("image;heure;score;couleur;jpg;raw"));
    assert_eq!(csv.lines().count(), jpg + 1, "one line per aurora frame");
    assert!(csv.lines().nth(1).unwrap().ends_with(".dng"));
}

#[tokio::test(start_paused = true)]
async fn raw_only_during_auroras_saves_the_key() {
    let (night, _) = setup(|c| c.capture.output_format = OutputFormat::JpgAuroraRaw); // 30 min
    let storage = StorageMock::new(night.capture.clone());
    // 3 calibration frames, then an aurora on camera frames 10..20 only
    run_night(&night, CameraMock::with_pattern(SkyPattern::Burst { from: 10, to: 20 }), storage, clock()).await;

    let files = images(&night.capture);
    let jpgs: Vec<&String> = files.iter().filter(|f| f.ends_with(".jpg")).collect();
    let dngs: Vec<&String> = files.iter().filter(|f| f.ends_with(".dng")).collect();
    assert!((170..=182).contains(&jpgs.len()), "JPEG all night: {}", jpgs.len());
    // RAW from the first aurora frame until 10 min (60 frames) after the last one
    assert!((60..=75).contains(&dngs.len()), "{} RAW", dngs.len());
    let first_dng = aurion::core::layout::parse_frame_number(dngs[0]).unwrap();
    let first_aurora = jpgs.iter().find(|f| f.contains("_AURORA")).map(|f| aurion::core::layout::parse_frame_number(f).unwrap()).unwrap();
    assert_eq!(first_dng, first_aurora, "no RAW before the aurora");
    assert!(dngs.iter().all(|d| jpgs.iter().any(|j| j.trim_end_matches(".jpg") == d.trim_end_matches(".dng"))),
        "every RAW has its JPEG twin (same name)");
}

#[tokio::test(start_paused = true)]
async fn resumed_night_continues_in_the_same_folder_and_numbering() {
    let (night, _) = setup(|_| {});
    let clock = clock();
    let previous = night.capture.join("sessions/2026-01-15_20-00");
    std::fs::create_dir_all(previous.join("JPG")).unwrap();
    std::fs::write(previous.join("JPG/aurora_20260115_205900_00041.jpg"), b"x").unwrap();
    std::fs::write(previous.join("JPG/.aurora_20260115_205910_00042.jpg.part"), b"half").unwrap();
    let started = clock.now_local() - chrono::Duration::hours(1);
    let mut marker = NightMarker::new(started, Some(1.5));
    marker.session = Some("2026-01-15_20-00".into());
    marker.save(&night.state.paths.night_marker).unwrap();

    let storage = StorageMock::new(night.capture.clone());
    storage.mount().await.unwrap();
    *night.state.phase.write().await = Phase::Disconnect; // "Reprendre maintenant"
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock.clone());
    tokio::time::timeout(std::time::Duration::from_secs(48 * 3600), orch.run()).await.unwrap().unwrap();

    assert_eq!(std::fs::read_dir(night.capture.join("sessions")).unwrap().count(), 1, "no second folder");
    assert!(!previous.join("JPG/.aurora_20260115_205910_00042.jpg.part").exists(), "power-cut leftovers cleaned");
    let files = images(&night.capture);
    let numbers: Vec<u64> = files.iter().map(|f| aurion::core::layout::parse_frame_number(f).unwrap()).collect();
    assert_eq!(numbers[0], 41);
    assert_eq!(numbers[1], 42, "numbering continues after the last saved frame");
    assert!(numbers.windows(2).all(|w| w[1] == w[0] + 1), "no duplicate or gap");
}

// ─── Expedition: nights without anyone touching the camera ──

fn range_from(clock: &TokioClock, start_in_min: i64, len_min: i64) -> (chrono::NaiveTime, chrono::NaiveTime) {
    let now = clock.now_local();
    ((now + chrono::Duration::minutes(start_in_min)).time(), (now + chrono::Duration::minutes(start_in_min + len_min)).time())
}

#[tokio::test(start_paused = true)]
async fn expedition_starts_alone_once_nobody_uses_the_interface() {
    let (night, _) = setup(|c| c.expedition.enabled = true);
    night.state.time_synced.store(true, std::sync::atomic::Ordering::Relaxed); // a phone set the clock
    let storage = StorageMock::new(night.capture.clone());
    storage.mount().await.unwrap();
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock());
    let run = tokio::spawn(async move { orch.run().await });

    tokio::time::sleep(std::time::Duration::from_secs(240)).await;
    assert_eq!(night.state.current_phase().await, Phase::Arm);
    assert!(matches!(*night.state.auto_start.read().await, aurion::web::AutoStart::At(_)));
    night.state.touch(); // the photographer checks the framing once more
    tokio::time::sleep(std::time::Duration::from_secs(240)).await;
    assert_eq!(night.state.current_phase().await, Phase::Arm, "activity postpones the start");
    tokio::time::sleep(std::time::Duration::from_secs(90)).await;
    assert_ne!(night.state.current_phase().await, Phase::Arm, "5 min without activity: the night starts");

    tokio::time::timeout(std::time::Duration::from_secs(48 * 3600), run).await.unwrap().unwrap().unwrap();
    assert!((170..=182).contains(&images(&night.capture).len()));
    assert_eq!(night.system.shutdown_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn expedition_never_starts_alone_with_an_unknown_clock() {
    let (night, _) = setup(|c| c.expedition.enabled = true);
    let storage = StorageMock::new(night.capture.clone());
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock());
    let run = tokio::spawn(async move { orch.run().await });
    tokio::time::sleep(std::time::Duration::from_secs(1800)).await;
    assert_eq!(night.state.current_phase().await, Phase::Arm);
    assert!(matches!(*night.state.auto_start.read().await, aurion::web::AutoStart::Blocked(_)));
    // The phone connects and sets the clock (an API request): the countdown starts
    night.state.time_synced.store(true, std::sync::atomic::Ordering::Relaxed);
    night.state.touch();
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    assert!(matches!(*night.state.auto_start.read().await, aurion::web::AutoStart::At(_)));
    run.abort();
}

#[tokio::test(start_paused = true)]
async fn pi5_expedition_programs_its_wake_up_for_the_next_night() {
    let clock = clock();
    let (start, end) = range_from(&clock, 0, 30);
    let (mut night, _) = setup(|c| {
        c.expedition.enabled = true;
        c.time_range.duration_hours = None;
        c.time_range.start = start;
        c.time_range.end = end;
    });
    night.system = SystemMock::with_wake();
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock.clone()).await;
    assert!(!images(&night.capture).is_empty());
    let wakes = night.system.wakes();
    assert_eq!(wakes.len(), 1, "one wake-up programmed");
    let expected = clock.now_local() - chrono::Duration::minutes(30) + chrono::Duration::days(1) - chrono::Duration::minutes(10);
    assert!((wakes[0].timestamp() - expected.timestamp()).abs() <= 120, "tomorrow, 10 min before the start: {} vs {}", wakes[0], expected);
    assert_eq!(night.system.shutdown_count(), 1);
}

#[tokio::test(start_paused = true)]
async fn pi5_sleeps_until_the_night_then_resumes_by_itself() {
    let clock = clock();
    let (start, end) = range_from(&clock, 5 * 60, 30); // night in 5 h
    let (mut night, _) = setup(|c| {
        c.time_range.duration_hours = None;
        c.time_range.start = start;
        c.time_range.end = end;
    });
    night.system = SystemMock::with_wake();
    let storage = StorageMock::new(night.capture.clone());
    run_night(&night, CameraMock::with_pattern(SkyPattern::Dark), storage, clock.clone()).await;
    assert!(images(&night.capture).is_empty(), "nothing captured before the night");
    assert_eq!(night.system.shutdown_count(), 1, "powered off instead of idling for 5 h");
    let wake = night.system.wakes()[0];
    assert!(night.state.paths.night_marker.exists(), "the marker brings the night back");

    // The RTC switches the board on: boot, resume window, night
    let until_wake = (wake.timestamp() - clock.now_local().timestamp()) as u64;
    tokio::time::sleep(std::time::Duration::from_secs(until_wake)).await;
    *night.state.phase.write().await = Phase::Arm;
    let storage = StorageMock::new(night.capture.clone());
    storage.mount().await.unwrap();
    let orch = Orchestrator::new(night.state.clone(), CameraMock::with_pattern(SkyPattern::Dark), storage, night.system.clone())
        .with_clock(clock.clone());
    tokio::time::timeout(std::time::Duration::from_secs(48 * 3600), orch.run()).await.unwrap().unwrap();
    let n = images(&night.capture).len();
    assert!((170..=182).contains(&n), "30 min night captured after waking: {}", n);
    assert_eq!(night.system.shutdown_count(), 2);
    assert!(!night.state.paths.night_marker.exists());
}
