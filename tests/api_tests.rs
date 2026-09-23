//! API contract tests: every endpoint used by the web interface answers
//! with the fields the pages rely on.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

#[tokio::test]
async fn status_contract() {
    let t = common::env();
    let res = t.server.get("/api/status").await;
    res.assert_status_ok();
    let body: Value = res.json();
    for key in ["phase", "time", "date", "epoch_ms", "warnings", "usb_mounted", "usb_writable", "default_password", "time_synced"] {
        assert!(body.get(key).is_some(), "missing '{}'", key);
    }
    assert_eq!(body["phase"], "ARM");
    assert!(body["warnings"].is_array());
    // Default password is flagged
    assert_eq!(body["default_password"], true);
    assert!(body["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("Mot de passe")));
}

#[tokio::test]
async fn config_contract_used_by_settings_pages() {
    let t = common::env();
    let body: Value = t.server.get("/api/config").await.json();
    let paths = [
        "/exposure/iso_min", "/exposure/iso_max", "/exposure/shutter_min_us", "/exposure/shutter_max_us",
        "/exposure/ev_step_max", "/exposure/target_brightness", "/exposure/saturation_reject",
        "/detection/roi_top_percent", "/detection/green_threshold", "/detection/luminosity_threshold",
        "/detection/variation_threshold", "/detection/consecutive_required", "/detection/detection_capture_enabled",
        "/detection/red_threshold", "/detection/blue_threshold", "/detection/area_min_percent",
        "/detection/hysteresis_on", "/detection/hysteresis_off", "/detection/moon_mask_enabled",
        "/capture/watch_interval_secs", "/capture/preview_interval_secs", "/capture/output_format",
        "/capture/focal_length_mm", "/capture/capture_interval_secs",
        "/time_range/start", "/time_range/end", "/time_range/duration_hours",
        "/storage/mount_point", "/storage/warning_percent", "/storage/critical_percent",
        "/network/ssid", "/network/password", "/network/channel", "/web/port", "/preset_name",
    ];
    for p in paths {
        assert!(body.pointer(p).is_some(), "config JSON is missing {}", p);
    }
    assert_eq!(body["network"]["password"], "********");
}

#[tokio::test]
async fn update_config_roundtrip_and_persistence() {
    let t = common::env();
    let mut cfg: Value = t.server.get("/api/config").await.json();
    cfg["exposure"]["iso_max"] = json!(1600);
    cfg["capture"]["output_format"] = json!("Jpg");

    let res = t.server.post("/api/config").json(&cfg).await;
    res.assert_status_ok();
    let saved: Value = res.json();
    assert_eq!(saved["exposure"]["iso_max"], 1600);
    assert_eq!(saved["network"]["password"], "********", "POST response must not leak the password");

    // Persisted atomically in the config dir
    let on_disk = aurion::core::config::AppConfig::load(&t.config_dir().join("aurion.json")).unwrap();
    assert_eq!(on_disk.exposure.iso_max, 1600);
    assert_eq!(on_disk.network.password, "aurora2024", "masked value must not overwrite the real password");
}

#[tokio::test]
async fn update_config_rejects_invalid_values() {
    let t = common::env();
    let mut cfg: Value = t.server.get("/api/config").await.json();
    cfg["exposure"]["iso_min"] = json!(3200);
    cfg["exposure"]["iso_max"] = json!(100);
    let res = t.server.post("/api/config").json(&cfg).await;
    res.assert_status(StatusCode::BAD_REQUEST);
    assert!(res.text().contains("ISO"));
    // Unchanged
    let now: Value = t.server.get("/api/config").await.json();
    assert_eq!(now["exposure"]["iso_max"], 3200);
}

#[tokio::test]
async fn update_config_changes_wifi_password() {
    let t = common::env();
    let mut cfg: Value = t.server.get("/api/config").await.json();
    cfg["network"]["password"] = json!("UnMotDePasseSolide42");
    t.server.post("/api/config").json(&cfg).await.assert_status_ok();
    assert_eq!(t.state.config.read().await.network.password, "UnMotDePasseSolide42");
    let body: Value = t.server.get("/api/config/is-default-password").await.json();
    assert_eq!(body["default"], false);
}

#[tokio::test]
async fn logs_are_timestamped_array() {
    let t = common::env();
    t.state.add_log("hello".into()).await;
    let body: Value = t.server.get("/api/logs").await.json();
    let logs = body["logs"].as_array().unwrap();
    assert!(logs[0].as_str().unwrap().ends_with("hello"));
    assert!(logs[0].as_str().unwrap().starts_with('['));
}

#[tokio::test]
async fn log_buffer_is_bounded() {
    let t = common::env();
    for i in 0..700 {
        t.state.add_log(format!("line {}", i)).await;
    }
    let body: Value = t.server.get("/api/logs").await.json();
    let logs = body["logs"].as_array().unwrap();
    assert_eq!(logs.len(), 500);
    assert!(logs.last().unwrap().as_str().unwrap().ends_with("line 699"));
}

#[tokio::test]
async fn disconnect_starts_the_night_only_once() {
    let t = common::env();
    t.server.post("/api/disconnect").await.assert_status_ok();
    let body: Value = t.server.get("/api/status").await.json();
    assert_eq!(body["phase"], "DISCONNECT");
    // Second click (or click during the night) is refused
    t.server.post("/api/disconnect").await.assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn presets_flow() {
    let t = common::env();
    let list: Value = t.server.get("/api/presets").await.json();
    assert_eq!(list["presets"].as_array().unwrap().len(), 3);

    t.server.post("/api/presets").json(&json!({"name": "Nuit Laponie"})).await.assert_status(StatusCode::CREATED);
    let list: Value = t.server.get("/api/presets").await.json();
    assert!(list["presets"].as_array().unwrap().iter().any(|p| p["name"] == "Nuit Laponie" && p["is_builtin"] == false));

    // The preset file never contains the Wi-Fi password
    let file = std::fs::read_to_string(t.config_dir().join("presets/Nuit_Laponie.json")).unwrap();
    assert!(!file.contains("aurora2024"));

    t.server.post("/api/presets/Moonlight/apply").await.assert_status_ok();
    assert_eq!(t.state.config.read().await.exposure.iso_max, 800);
    t.server.post("/api/presets/Nuit%20Laponie/apply").await.assert_status_ok();
    assert_eq!(t.state.config.read().await.exposure.iso_max, 3200);

    t.server.delete("/api/presets/Nuit%20Laponie").await.assert_status(StatusCode::NO_CONTENT);
    t.server.delete("/api/presets/Moonlight").await.assert_status(StatusCode::FORBIDDEN);
    t.server.post("/api/presets/Inconnu/apply").await.assert_status(StatusCode::NOT_FOUND);
    t.server.post("/api/presets").json(&json!({"name": "FullDark"})).await.assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn storage_endpoint_reports_capture_drive() {
    let t = common::env();
    let body: Value = t.server.get("/api/storage").await.json();
    assert!(body["total_gb"].as_f64().unwrap() > 0.0);
    assert!(body["summary"].as_str().unwrap().contains("libre"));
}

#[tokio::test]
async fn storage_endpoint_when_drive_missing() {
    let t = common::env_with(|c| c.storage.mount_point = "/nonexistent/aurion".into());
    let body: Value = t.server.get("/api/storage").await.json();
    assert_eq!(body["status"], "Error");
    let status: Value = t.server.get("/api/status").await.json();
    assert_eq!(status["usb_mounted"], false);
    assert!(status["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap().contains("USB")));
}

#[tokio::test]
async fn diagnostics_has_version() {
    let t = common::env();
    let body: Value = t.server.get("/api/diagnostics").await.json();
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert!(body.get("timezone").is_some());
}

#[tokio::test]
async fn system_actions_disabled_off_device() {
    let t = common::env();
    t.server.post("/api/system/shutdown").await.assert_status(StatusCode::NOT_IMPLEMENTED);
    t.server.get("/api/wifi/scan").await.assert_status(StatusCode::NOT_IMPLEMENTED);
    t.server.post("/api/wifi/hotspot").await.assert_status(StatusCode::NOT_IMPLEMENTED);
    t.server.get("/api/wifi/status").await.assert_status_ok();
}

#[tokio::test]
async fn time_sync_rules() {
    let t = common::env();
    let now_ms = chrono::Utc::now().timestamp_millis();
    // Auto sync with a clock already right: nothing to do, marked as synced
    let res = t.server.post("/api/system/time").json(&json!({"epoch_ms": now_ms, "auto": true})).await;
    res.assert_status_ok();
    assert_eq!(res.json::<Value>()["changed"], false);
    assert_eq!(t.server.get("/api/status").await.json::<Value>()["time_synced"], true);

    // Large drift requires the privileged helper (disabled here)
    t.server.post("/api/system/time").json(&json!({"epoch_ms": now_ms + 3_600_000})).await
        .assert_status(StatusCode::NOT_IMPLEMENTED);

    // Invalid inputs
    t.server.post("/api/system/time").json(&json!({"datetime": "now; reboot"})).await.assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/system/time").json(&json!({"epoch_ms": 0})).await.assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/system/time").json(&json!({"epoch_ms": now_ms, "timezone": "../../etc/shadow"})).await
        .assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/system/time").json(&json!({})).await.assert_status(StatusCode::BAD_REQUEST);

    // Never during a capture
    *t.state.phase.write().await = aurion::core::models::Phase::Run;
    t.server.post("/api/system/time").json(&json!({"epoch_ms": now_ms + 3_600_000})).await
        .assert_status(StatusCode::CONFLICT);
    let auto = t.server.post("/api/system/time").json(&json!({"epoch_ms": now_ms + 3_600_000, "auto": true})).await;
    auto.assert_status_ok();
    assert_eq!(auto.json::<Value>()["changed"], false);
}

#[tokio::test]
async fn preview_refused_during_capture() {
    let t = common::env();
    *t.state.phase.write().await = aurion::core::models::Phase::Watch;
    t.server.post("/api/preview/capture").await.assert_status(StatusCode::CONFLICT);
    t.server.get("/api/preview").await.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn static_pages_are_served_from_the_binary() {
    let t = common::env();
    for page in ["/", "/index.html", "/dashboard.html", "/gallery.html", "/settings.html",
                 "/settings_advanced.html", "/presets.html", "/preview.html", "/storage.html", "/diagnostics.html"] {
        let res = t.server.get(page).await;
        res.assert_status_ok();
        assert!(res.header("content-type").to_str().unwrap().starts_with("text/html"), "{}", page);
        let html = res.text();
        if page == "/dashboard.html" {
            assert!(html.contains("url=/index.html"), "old dashboard redirects to the home page");
            continue;
        }
        assert!(html.contains("/js/common.js"), "{} must load common.js", page);
        assert!(html.contains(r#"id="sideMenu""#), "{} must have the shared menu", page);
        assert!(!html.contains("fonts.googleapis.com"), "{} must work offline", page);
    }
    let css = t.server.get("/css/style.css").await;
    css.assert_status_ok();
    assert!(css.header("content-type").to_str().unwrap().starts_with("text/css"));
    t.server.get("/nope.html").await.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn captive_portal_redirects_every_os() {
    let t = common::env();
    for probe in ["/hotspot-detect.html", "/library/test/success.html", "/generate_204", "/gen_204",
                  "/connecttest.txt", "/ncsi.txt", "/redirect", "/canonical.html", "/success.txt"] {
        let res = t.server.get(probe).await;
        res.assert_status(StatusCode::TEMPORARY_REDIRECT);
        assert_eq!(res.header("location"), "http://192.168.4.1:8080/", "{}", probe);
    }
}

#[tokio::test]
async fn dark_frames_rules() {
    let t = common::env();
    // No preview yet and no explicit settings
    t.server.post("/api/darks").json(&json!({})).await.assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/darks").json(&json!({"count": 0, "iso": 800, "shutter_us": 5_000_000})).await
        .assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/darks").json(&json!({"count": 50, "iso": 800, "shutter_us": 5_000_000})).await
        .assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/darks").json(&json!({"iso": 800, "shutter_us": 999_000_000})).await
        .assert_status(StatusCode::BAD_REQUEST);
    // Never during the night
    *t.state.phase.write().await = aurion::core::models::Phase::Run;
    t.server.post("/api/darks").json(&json!({"iso": 800, "shutter_us": 5_000_000})).await
        .assert_status(StatusCode::CONFLICT);
    // Status endpoint
    t.server.get("/api/darks").await.assert_status_ok();
}

#[tokio::test]
async fn dark_frames_start_in_background() {
    let t = common::env();
    let res = t.server.post("/api/darks").json(&json!({"count": 2, "iso": 800, "shutter_us": 1_000_000})).await;
    res.assert_status_ok();
    let body: Value = res.json();
    assert_eq!(body["total"], 2);
    assert_eq!(body["running"], true);
    // Second series refused while the first one runs (or fails fast without camera)
    for _ in 0..50 {
        let st: Value = t.server.get("/api/darks").await.json();
        if st["running"] == false {
            // No camera on the test machine: a clear error is reported
            assert!(st["error"].is_string());
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("dark series never finished");
}

#[tokio::test]
async fn best_auroras_endpoint() {
    let t = common::env();
    let dir = t.add_session("2026-03-05_21-30");
    let ev = |n: u64, score: f64| format!(
        "{{\"timestamp\":\"2026-03-05T22:0{}:00\",\"phase\":\"Run\",\"capture_mode\":\"SAFE\",\"exposure_us\":5000000,\"iso\":800,\"format\":\"RawAndJpg\",\"roi_excluded_percent\":35,\"aurora_score\":{},\"aurora_detected\":true,\"aurora_color\":\"green\",\"consecutive_hits\":0,\"moon_mask_active\":false,\"frame_number\":{}}}\n", n, score, n);
    std::fs::write(dir.join("event.jsonl"), format!("{}{}{}", ev(0, 1.2), ev(1, 3.5), ev(2, 0.0))).unwrap();
    for n in 0..3 {
        t.add_file(&format!("aurora_20260305_220{}00_0000{}_AURORA.jpg", n, n), b"j");
        t.add_file(&format!("aurora_20260305_220{}00_0000{}_AURORA.dng", n, n), b"d");
    }
    let best: Value = t.server.get("/api/gallery/best").await.json();
    let arr = best.as_array().unwrap();
    assert_eq!(arr.len(), 2, "score 0 excluded");
    assert_eq!(arr[0]["score"], 3.5);
    assert_eq!(arr[0]["dng"], "aurora_20260305_220100_00001_AURORA.dng");
    assert_eq!(arr[0]["jpg"], "aurora_20260305_220100_00001_AURORA.jpg");
}

#[tokio::test]
async fn preflight_ready_with_usb_and_warns_on_default_password() {
    let t = common::env();
    let pf: Value = t.server.get("/api/preflight").await.json();
    assert_eq!(pf["ready"], true, "{}", pf);
    assert_eq!(pf["phase"], "ARM");
    let ids: Vec<&str> = pf["checks"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
    for id in ["camera", "usb", "clock", "password"] {
        assert!(ids.contains(&id), "{} missing in {:?}", id, ids);
    }
    let pw = pf["checks"].as_array().unwrap().iter().find(|c| c["id"] == "password").unwrap();
    assert_eq!(pw["level"], "warn");
    assert!(pf["capacity_hours"].as_f64().unwrap() > 0.0);
    assert_eq!(pf["planned_hours"], 9.0, "21:00 to 06:00");
}

#[tokio::test]
async fn preflight_blocks_without_usb_drive() {
    let t = common::env_with(|c| {
        c.storage.mount_point = "/nonexistent/aurion".into();
        c.network.password = "UnMotDePassePerso".into();
        c.time_range.duration_hours = Some(4.0);
    });
    let pf: Value = t.server.get("/api/preflight").await.json();
    assert_eq!(pf["ready"], false);
    let usb = pf["checks"].as_array().unwrap().iter().find(|c| c["id"] == "usb").unwrap();
    assert_eq!(usb["level"], "error");
    assert!(pf["checks"].as_array().unwrap().iter().all(|c| c["id"] != "password"));
    assert_eq!(pf["planned_hours"], 4.0);
}

#[tokio::test]
async fn preflight_in_expedition_mode_counts_nights_and_questions_the_clock() {
    let t = common::env_with(|c| {
        c.expedition.enabled = true;
        c.capture.output_format = aurion::core::models::OutputFormat::JpgAuroraRaw;
    });
    let pf: Value = t.server.get("/api/preflight").await.json();
    assert_eq!(pf["expedition"], true);
    assert!(pf["nights_capacity"].as_f64().unwrap() > 0.0, "{}", pf);
    let checks = pf["checks"].as_array().unwrap();
    let exp = checks.iter().find(|c| c["id"] == "expedition").expect("expedition line");
    assert!(exp["detail"].as_str().unwrap().contains("RAW des aurores"));
    let clock = checks.iter().find(|c| c["id"] == "clock").unwrap();
    assert_eq!(clock["level"], "warn", "no RTC and no phone sync yet");
    assert_eq!(pf["ready"], true, "a warning never blocks the night");

    // The phone sets the clock → trusted
    t.state.time_synced.store(true, std::sync::atomic::Ordering::Relaxed);
    let pf: Value = t.server.get("/api/preflight").await.json();
    let clock = pf["checks"].as_array().unwrap().iter().find(|c| c["id"] == "clock").unwrap().clone();
    assert_eq!(clock["level"], "ok");
}
