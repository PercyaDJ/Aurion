//! "Faites vos darks" reminder (Q2): API contract and disappearance of the
//! reminder once the darks are done or dismissed.
//!
//! A fake `rpicam-still` on the PATH writes a small DNG, so the whole dark
//! series runs without a camera. It lives in its own test binary: the other
//! API tests expect `rpicam-still` to be missing.

mod common;

use std::sync::OnceLock;
use std::time::Duration;

use aurion::core::dark_reminder::{DarkReminder, NightStats};
use axum::http::StatusCode;
use serde_json::{json, Value};

/// Hot pixels of the template DNG (x, y).
const HOT: [(u32, u32); 2] = [(5, 6), (50, 40)];

fn fake_camera() {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        // A real (small) DNG with the rpicam-apps layout and two hot pixels
        let (w, h) = (64u32, 48u32);
        let mut px: Vec<u16> = (0..w * h).map(|i| 256 + (i % 3) as u16).collect();
        for (x, y) in HOT {
            px[(y * w + x) as usize] = 2000;
        }
        let template = dir.path().join("template.dng");
        std::fs::write(&template, aurion::core::dng::test_dng_bytes(w, h, &px, &Default::default())).unwrap();
        // Metadata as rpicam-still writes it (not valid JSON: unquoted size)
        let bin = dir.path().join("rpicam-still");
        let script = format!(
            "#!/bin/sh\nwhile [ $# -gt 0 ]; do case \"$1\" in -o) out=\"$2\";; --metadata) meta=\"$2\";; esac; shift; done\n\
             cp '{}' \"${{out%.*}}.dng\"\n\
             [ -n \"$meta\" ] && printf '{{\\n    \"ExposureTime\": 4999990,\\n    \"SensorTemperature\": 33.000000,\\n    \"AnalogueGain\": 8.000000,\\n    \"OutputSize\": 4056x3040\\n}}\\n' > \"$meta\"\n\
             exit 0\n",
            template.display()
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = format!("{}:{}", dir.path().display(), std::env::var("PATH").unwrap_or_default());
        std::env::set_var("PATH", path);
        dir
    });
}

fn reminder(iso: u32, shutter_us: u64) -> DarkReminder {
    let mut s = NightStats::new();
    s.add(iso, shutter_us, Some(40.0));
    s.reminder(Some("2026-03-05_21-30".into()), 1_772_000_000_000).unwrap()
}

async fn wait_darks(t: &common::TestEnv) -> Value {
    for _ in 0..200 {
        let st: Value = t.server.get("/api/darks").await.json();
        if st["running"] == false {
            return st;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("dark series never finished");
}

#[tokio::test]
async fn no_reminder_before_the_first_night() {
    let t = common::env();
    let body: Value = t.server.get("/api/darks/reminder").await.json();
    assert!(body.is_null());
    // Dismissing nothing is harmless
    t.server.post("/api/darks/reminder/dismiss").await.assert_status(StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn reminder_is_shown_until_dismissed() {
    let t = common::env();
    reminder(1600, 5_000_000).save(&t.state.paths.dark_reminder).unwrap();
    let body: Value = t.server.get("/api/darks/reminder").await.json();
    assert_eq!(body["iso"], 1600);
    assert_eq!(body["shutter_us"], 5_000_000);
    assert_eq!(body["temp_c"], 40.0);
    assert_eq!(body["night"], "2026-03-05_21-30");
    assert_eq!(body["frames"], 1);

    t.server.post("/api/darks/reminder/dismiss").await.assert_status(StatusCode::NO_CONTENT);
    assert!(!t.state.paths.dark_reminder.exists());
    assert!(t.server.get("/api/darks/reminder").await.json::<Value>().is_null());
}

#[tokio::test]
async fn reminder_disappears_once_the_darks_are_done() {
    fake_camera();
    let t = common::env();
    reminder(1600, 5_000_000).save(&t.state.paths.dark_reminder).unwrap();

    // Darks at other settings (framing page): the reminder stays
    t.server.post("/api/darks").json(&json!({"count": 1, "iso": 800, "shutter_us": 5_000_000})).await.assert_status_ok();
    let st = wait_darks(&t).await;
    assert!(st["error"].is_null(), "{}", st);
    assert!(t.state.paths.dark_reminder.exists());

    // Darks at the settings of the night: done
    t.server.post("/api/darks").json(&json!({"count": 2, "iso": 1600, "shutter_us": 5_000_000})).await.assert_status_ok();
    let st = wait_darks(&t).await;
    assert!(st["error"].is_null(), "{}", st);
    assert_eq!(st["done"], 2);
    assert!(!t.state.paths.dark_reminder.exists());
    let darks: Vec<_> = std::fs::read_dir(t.capture_dir().join("darks")).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .filter(|n| n.contains("_ISO1600_5.0s_"))
        .collect();
    assert_eq!(darks.len(), 2, "{:?}", darks);
}

#[tokio::test]
async fn a_dark_series_gives_a_master_dark_and_a_hot_pixel_map() {
    fake_camera();
    let t = common::env();
    t.server.post("/api/darks").json(&json!({"count": 3, "iso": 800, "shutter_us": 5_000_000})).await.assert_status_ok();
    let st = wait_darks(&t).await;
    assert!(st["error"].is_null(), "{}", st);
    // Sensor temperature from the camera metadata, not the processor
    assert_eq!(st["temp_c"], 33.0);
    assert_eq!(st["temp_source"], "capteur");
    let m = &st["master"];
    assert_eq!(m["state"], "done", "{}", st);
    assert_eq!(m["rows_done"], 48);
    assert_eq!(m["rows_total"], 48);
    assert_eq!(m["fits"], "darks/master/master_dark_ISO800_5.0s_T35.fits");
    assert_eq!(m["hot_pixels_file"], "darks/master/hot_pixels_ISO800_5.0s_T35.txt");
    assert_eq!(m["hot_pixels"], 2);

    let fits = std::fs::read(t.capture_dir().join("darks/master/master_dark_ISO800_5.0s_T35.fits")).unwrap();
    assert_eq!(fits.len() % 2880, 0);
    let header = String::from_utf8_lossy(&fits[..2880]).to_string();
    assert!(header.contains("CCD-TEMP=                 33.0"), "{}", header);
    assert!(header.contains("STACKCNT=                    3"));
    let map = std::fs::read_to_string(t.capture_dir().join("darks/master/hot_pixels_ISO800_5.0s_T35.txt")).unwrap();
    assert!(map.lines().any(|l| l == "5 6 2000"), "{}", map);
    assert!(map.lines().any(|l| l == "50 40 2000"), "{}", map);
    // The 3 original DNGs are still there, untouched
    let dngs = std::fs::read_dir(t.capture_dir().join("darks")).unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".dng"))
        .count();
    assert_eq!(dngs, 3);

    // Invalid k refused before anything starts
    t.server.post("/api/darks").json(&json!({"count": 1, "iso": 800, "shutter_us": 5_000_000, "hot_k": 0.5})).await
        .assert_status(StatusCode::BAD_REQUEST);
}
