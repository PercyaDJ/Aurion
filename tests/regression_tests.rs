//! Non-regression tests: one test per bug found during the review, plus
//! "golden" values of the detection so any change of the algorithm is
//! noticed.

mod common;

use aurion::adapters::pc::{CameraMock, SkyPattern};
use aurion::core::config::AppConfig;
use aurion::core::detection::AuroraDetector;
use aurion::core::models::ExposureSettings;
use aurion::ports::camera::CameraPort;
use serde_json::{json, Value};

/// Bug: the masked password "********" (8 chars) was validated before being
/// replaced by the real one, so every save from the settings page failed.
#[tokio::test]
async fn saving_settings_with_masked_password_succeeds() {
    let t = common::env();
    let cfg: Value = t.server.get("/api/config").await.json();
    assert_eq!(cfg["network"]["password"], "********");
    t.server.post("/api/config").json(&cfg).await.assert_status_ok();
    assert_eq!(t.state.config.read().await.network.password, "aurora2024");
}

/// Bug: listing the sessions deleted the folders that had no image yet
/// (current night in FILTER mode, or nights without aurora).
#[tokio::test]
async fn listing_sessions_never_deletes_anything() {
    let t = common::env();
    let dir = t.add_session("2026-03-05_21-30");
    let sessions: Value = t.server.get("/api/gallery/sessions").await.json();
    assert_eq!(sessions[0]["image_count"], 0);
    assert_eq!(sessions[0]["has_log"], true);
    assert!(dir.join("event.jsonl").exists(), "GET must be read-only");
}

/// Bug: a session folder with a short name made the server panic.
#[tokio::test]
async fn odd_session_names_do_not_panic() {
    let t = common::env();
    std::fs::create_dir_all(t.capture_dir().join("sessions/abc")).unwrap();
    std::fs::write(t.capture_dir().join("sessions/abc/session.log"), b"x").unwrap();
    t.add_file("aurora_20260305_213100_00000.jpg", b"x");
    t.server.get("/api/gallery/sessions").await.assert_status_ok();
    t.server.get("/api/gallery/sessions/abc/download").await.assert_status_ok();
}

/// Bug: applying a built-in preset reset the Wi-Fi password to the factory
/// value and was lost at reboot (not persisted).
#[tokio::test]
async fn applying_a_preset_keeps_password_and_persists() {
    let t = common::env_with(|c| c.network.password = "MonMotDePassePerso".into());
    t.server.post("/api/presets/Moonlight/apply").await.assert_status_ok();
    assert_eq!(t.state.config.read().await.network.password, "MonMotDePassePerso");
    let disk = AppConfig::load(&t.config_dir().join("aurion.json")).unwrap();
    assert_eq!(disk.exposure.iso_max, 800);
    assert_eq!(disk.network.password, "MonMotDePassePerso");
}

/// Bug: POST /api/disconnect during the night overwrote the phase shown in
/// the UI (stuck on DISCONNECT).
#[tokio::test]
async fn disconnect_during_run_is_refused() {
    let t = common::env();
    *t.state.phase.write().await = aurion::core::models::Phase::Run;
    t.server.post("/api/disconnect").await.assert_status(axum::http::StatusCode::CONFLICT);
    assert_eq!(t.state.current_phase().await, aurion::core::models::Phase::Run);
}

/// Bug: a missing USB key was reported as "mounted" because the probe
/// file was written on the SD card instead.
#[test]
fn plain_folder_is_not_reported_as_usb_drive() {
    let dir = tempfile::tempdir().unwrap();
    let h = aurion::sys::storage_health(dir.path());
    assert!(h.writable && !h.is_mountpoint);
}

/// Bug: the orchestrator passed the already-cropped ROI to the detector,
/// which cropped it again (65 % × 65 % = 42 % of the sky analysed).
/// An aurora only visible between 42 % and 65 % of the height must be seen.
#[test]
fn detection_uses_the_full_roi() {
    let (w, h) = (200u32, 100u32);
    let mut data = vec![5u8; (w * h * 3) as usize];
    for y in 44..64 {
        for x in 0..w {
            let i = ((y * w + x) * 3) as usize;
            data[i] = 30;
            data[i + 1] = 160;
            data[i + 2] = 40;
        }
    }
    let cfg = AppConfig::default();
    let mut det = AuroraDetector::from_config(&cfg.detection);
    let full = det.analyze(&data, w, h);
    assert!(full.green_score > 0.0, "band inside the 65 % ROI must be analysed");

    // What the old code did: detector on the cropped ROI → band invisible
    let roi = aurion::core::orchestrator::crop_roi(&data, w, h, cfg.detection.roi_top_percent);
    let mut det2 = AuroraDetector::from_config(&cfg.detection);
    let old = det2.analyze(roi, w, 65);
    assert_eq!(old.green_score, 0.0);
}

/// Golden values of the detection on the synthetic sky (tolerance 5 %).
#[tokio::test]
async fn golden_detection_scores() {
    let cfg = AppConfig::default();
    let settings = ExposureSettings::new(800, 5_000_000);
    let tmp = std::path::Path::new("/tmp");

    let cam = CameraMock::with_pattern(SkyPattern::Aurora);
    let mut det = AuroraDetector::from_config(&cfg.detection);
    let f = cam.capture_jpg(&settings, tmp).await.unwrap();
    let r = det.analyze(&f.data, f.width, f.height);
    assert!(r.detected);
    assert_eq!(r.aurora_color, aurion::core::models::AuroraColor::Green);
    assert!((r.aurora_score - 1.56).abs() < 0.08, "score {}", r.aurora_score);
    assert!((r.green_score - 23.4).abs() < 1.2, "green {}", r.green_score);
    assert!((r.area_percent - 49.6).abs() < 2.5, "area {}", r.area_percent);

    let dark = CameraMock::with_pattern(SkyPattern::Dark);
    let mut det = AuroraDetector::from_config(&cfg.detection);
    let f = dark.capture_jpg(&settings, tmp).await.unwrap();
    let r = det.analyze(&f.data, f.width, f.height);
    assert!(!r.detected);
    assert_eq!(r.aurora_score, 0.0);
}

/// The API rejects a config the UI can never produce (ROI must match the
/// detector range 50-99 %).
#[tokio::test]
async fn roi_bounds_match_the_ui() {
    let t = common::env();
    let mut cfg: Value = t.server.get("/api/config").await.json();
    cfg["detection"]["roi_top_percent"] = json!(30);
    t.server.post("/api/config").json(&cfg).await.assert_status(axum::http::StatusCode::BAD_REQUEST);
    cfg["detection"]["roi_top_percent"] = json!(99);
    t.server.post("/api/config").json(&cfg).await.assert_status_ok();
}

/// An old config file (fields added later missing) still loads.
#[test]
fn legacy_config_file_loads_with_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("aurion.json");
    std::fs::write(&path, r#"{
      "exposure": {"iso_min":100,"iso_max":3200,"shutter_min_us":1000000,"shutter_max_us":30000000,"ev_step_max":0.33},
      "detection": {"roi_top_percent":65,"green_threshold":15.0,"luminosity_threshold":30.0,"variation_threshold":10.0,"consecutive_required":2},
      "capture": {"watch_interval_secs":60,"preview_interval_secs":30,"output_format":"Jpg","focal_length_mm":2.7},
      "time_range": {"start":"21:00:00","end":"06:00:00"},
      "storage": {"mount_point":"/mnt/capture","warning_percent":15,"critical_percent":5},
      "network": {"ssid":"Aurion","password":"aurora2024","channel":6},
      "web": {"port":8080},
      "preset_name":"Default"
    }"#).unwrap();
    let cfg = AppConfig::load(&path).unwrap();
    assert_eq!(cfg.capture.capture_interval_secs, 10);
    assert!(cfg.detection.moon_mask_enabled);
    assert!(!cfg.detection.detection_capture_enabled);
}
