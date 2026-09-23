//! Online update from GitHub (against a local fake GitHub) and rollback.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

/// A fake GitHub: release description + a "binary" that is not an ELF.
async fn fake_github() -> String {
    use axum::{routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let release = json!({"tag_name": "edge", "assets": [
        {"name": "aurion-arm64", "browser_download_url": format!("{}/dl/edge/aurion-arm64", base)}]});
    let app = Router::new()
        .route("/repos/PercyaDJ/Aurion/releases/tags/edge", get(move || { let r = release.clone(); async move { axum::Json(r) } }))
        .route("/dl/edge/aurion-arm64", get(|| async { "#!/bin/sh\necho not an aurion binary\n" }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

async fn wait_done(t: &common::TestEnv) -> Value {
    for _ in 0..100 {
        let s: Value = t.server.get("/api/system/update/status").await.json();
        if s["last"]["running"] == false {
            return s;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("update never finished");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_downloads_from_github_and_checks_the_binary() {
    let base = fake_github().await;
    let t = common::env_custom(|_| {}, |u| {
        u.github_api = base.clone();
        u.download_prefix = format!("{}/dl/", base);
    });
    // Pi already on a network with internet: no Wi-Fi to join
    let res = t.server.post("/api/system/update/online").json(&json!({"channel": "dev"})).await;
    res.assert_status_ok();
    let s = wait_done(&t).await;
    assert_eq!(s["last"]["ok"], false, "{}", s);
    assert!(s["last"]["message"].as_str().unwrap().contains("ELF"), "downloaded then refused: {}", s);
    assert!(!t.dir.path().join("bin/aurion").exists(), "nothing installed");
    assert_eq!(s["version"], aurion::VERSION);
}

#[tokio::test]
async fn online_update_rejects_bad_requests() {
    let t = common::env();
    t.server.post("/api/system/update/online").json(&json!({"channel": "nightly"})).await.assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/system/update/online").json(&json!({"channel": "dev", "ssid": "Tel\nevil", "password": "12345678"}))
        .await.assert_status(StatusCode::BAD_REQUEST);
    t.server.post("/api/system/update/online").json(&json!({"channel": "dev", "ssid": "MonTel", "password": "court"}))
        .await.assert_status(StatusCode::BAD_REQUEST);
    // Valid hotspot but no system actions on a PC: refused, and the flag is released
    t.server.post("/api/system/update/online").json(&json!({"channel": "dev", "ssid": "MonTel", "password": "motdepasse"}))
        .await.assert_status(StatusCode::NOT_IMPLEMENTED);
    assert!(!t.state.online_update_running.load(std::sync::atomic::Ordering::SeqCst));
    // The hotspot credentials were saved, the password never comes back
    let cfg: Value = t.server.get("/api/config").await.json();
    assert_eq!(cfg["online_update"]["ssid"], "MonTel");
    assert_ne!(cfg["online_update"]["password"], "motdepasse");
    // Never during a night
    *t.state.phase.write().await = aurion::core::models::Phase::Run;
    t.server.post("/api/system/update/online").json(&json!({"channel": "stable"})).await.assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn rollback_swaps_current_and_previous() {
    let t = common::env();
    let bin = t.dir.path().join("bin/aurion");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    t.server.post("/api/system/rollback").await.assert_status(StatusCode::NOT_FOUND);
    std::fs::write(&bin, b"NEW").unwrap();
    std::fs::write(bin.with_extension("prev"), b"OLD").unwrap();
    let s: Value = t.server.get("/api/system/update/status").await.json();
    assert_eq!(s["rollback_available"], true);
    t.server.post("/api/system/rollback").await.assert_status_ok();
    assert_eq!(std::fs::read(&bin).unwrap(), b"OLD");
    assert_eq!(std::fs::read(bin.with_extension("prev")).unwrap(), b"NEW", "the trial can be redone the same way");
}
