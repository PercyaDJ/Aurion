//! Shared helpers for the integration tests: every test gets its own
//! temporary config directory and capture drive, so nothing touches the
//! repository or the real device paths.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use axum_test::TestServer;
use aurion::core::config::AppConfig;
use aurion::web::{AppState, Paths, UpdateSettings};

pub struct TestEnv {
    pub dir: tempfile::TempDir,
    pub state: AppState,
    pub server: TestServer,
}

impl TestEnv {
    pub fn capture_dir(&self) -> PathBuf {
        self.dir.path().join("capture")
    }

    pub fn config_dir(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    /// Write a fake image on the capture drive.
    pub fn add_file(&self, name: &str, content: &[u8]) -> PathBuf {
        let p = self.capture_dir().join(name);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&p, content).unwrap();
        p
    }

    pub fn add_session(&self, name: &str) -> PathBuf {
        let p = self.capture_dir().join("sessions").join(name);
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(p.join("event.jsonl"), b"{\"aurora_detected\":true}\n").unwrap();
        std::fs::write(p.join("session.log"), b"=== session ===\n").unwrap();
        p
    }
}

pub fn test_config(capture: &Path) -> AppConfig {
    let mut config = AppConfig::default();
    config.storage.mount_point = capture.to_string_lossy().to_string();
    config
}

pub fn env_with(config_fn: impl FnOnce(&mut AppConfig)) -> TestEnv {
    let dir = tempfile::tempdir().unwrap();
    let capture = dir.path().join("capture");
    std::fs::create_dir_all(&capture).unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    let mut config = test_config(&capture);
    config_fn(&mut config);

    let mut paths = Paths::from_config_dir(&config_dir);
    paths.thumb_cache_dir = dir.path().join("thumb_cache");
    paths.tmp_dir = dir.path().join("tmp");
    std::fs::create_dir_all(&paths.tmp_dir).unwrap();

    let state = AppState::with_paths(config, paths)
        .with_system_actions(false)
        .with_update_settings(UpdateSettings { target: Some(dir.path().join("bin/aurion")), restart: false });
    let server = TestServer::new(aurion::web::build_router(state.clone())).unwrap();
    TestEnv { dir, state, server }
}

pub fn env() -> TestEnv {
    env_with(|_| {})
}

/// A small valid JPEG (solid colour).
pub fn tiny_jpeg(w: u32, h: u32) -> Vec<u8> {
    let img = image::RgbImage::from_pixel(w, h, image::Rgb([20, 120, 30]));
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Jpeg).unwrap();
    buf.into_inner()
}
