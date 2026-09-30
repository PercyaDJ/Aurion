//! Online update from GitHub (against a local fake GitHub) and rollback.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

/// Hex SHA-256, as `sha256sum` writes it.
fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(data).iter().map(|b| format!("{:02x}", b)).collect()
}

/// A fake GitHub: release description, the binary, and optionally its
/// published checksum (`sha256sum` format) and signature.
async fn fake_github_with(binary: Vec<u8>, checksum: Option<String>, signature: Option<Vec<u8>>) -> String {
    use axum::{routing::get, Router};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut assets = vec![json!({"name": "aurion-arm64", "browser_download_url": format!("{}/dl/edge/aurion-arm64", base)})];
    if checksum.is_some() {
        assets.push(json!({"name": "aurion-arm64.sha256", "browser_download_url": format!("{}/dl/edge/aurion-arm64.sha256", base)}));
    }
    if signature.is_some() {
        assets.push(json!({"name": "aurion-arm64.sig", "browser_download_url": format!("{}/dl/edge/aurion-arm64.sig", base)}));
    }
    let release = json!({"tag_name": "edge", "assets": assets});
    let sum = checksum.unwrap_or_default();
    let sig = signature.unwrap_or_default();
    let app = Router::new()
        .route("/repos/PercyaDJ/Aurion/releases/tags/edge", get(move || { let r = release.clone(); async move { axum::Json(r) } }))
        .route("/dl/edge/aurion-arm64", get(move || { let b = binary.clone(); async move { b } }))
        .route("/dl/edge/aurion-arm64.sha256", get(move || { let s = sum.clone(); async move { s } }))
        .route("/dl/edge/aurion-arm64.sig", get(move || { let s = sig.clone(); async move { s } }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

/// A binary that is not an ELF, with its correct checksum.
async fn fake_github() -> String {
    let bin = b"#!/bin/sh\necho not an aurion binary\n".to_vec();
    let sum = format!("{}  aurion-arm64\n", sha256_hex(&bin));
    let sig = common::sign(&bin);
    fake_github_with(bin, Some(sum), Some(sig)).await
}

/// Run an online update against `base` and wait for its result.
async fn online_update(base: String) -> (common::TestEnv, Value) {
    let t = common::env_custom(|_| {}, |u| {
        u.github_api = base.clone();
        u.download_prefix = format!("{}/dl/", base);
    });
    t.server.post("/api/system/update/online").json(&json!({"channel": "dev"})).await.assert_status_ok();
    let s = wait_done(&t).await;
    (t, s)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_installs_a_binary_matching_its_checksum() {
    let bin = std::fs::read("/bin/bash").unwrap(); // a real executable for this machine
    let sum = format!("{}  aurion-arm64\n", sha256_hex(&bin));
    let sig = common::sign(&bin);
    let (t, s) = online_update(fake_github_with(bin.clone(), Some(sum), Some(sig)).await).await;
    assert_eq!(s["last"]["ok"], true, "{}", s);
    assert_eq!(std::fs::read(t.dir.path().join("bin/aurion")).unwrap(), bin);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_refuses_a_binary_not_matching_its_checksum() {
    let bin = std::fs::read("/bin/bash").unwrap();
    let wrong = format!("{}  aurion-arm64\n", sha256_hex(b"another file"));
    let sig = common::sign(&bin);
    let (t, s) = online_update(fake_github_with(bin, Some(wrong), Some(sig)).await).await;
    assert_eq!(s["last"]["ok"], false, "{}", s);
    assert!(s["last"]["message"].as_str().unwrap().contains("Empreinte"), "{}", s);
    assert!(!t.dir.path().join("bin/aurion").exists(), "nothing installed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_refuses_a_release_without_checksum() {
    let bin = std::fs::read("/bin/bash").unwrap();
    let sig = common::sign(&bin);
    let (t, s) = online_update(fake_github_with(bin, None, Some(sig)).await).await;
    assert_eq!(s["last"]["ok"], false, "{}", s);
    assert!(s["last"]["message"].as_str().unwrap().contains("absente"), "{}", s);
    assert!(!t.dir.path().join("bin/aurion").exists(), "nothing installed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_refuses_an_unsigned_release() {
    let bin = std::fs::read("/bin/bash").unwrap();
    let sum = format!("{}  aurion-arm64\n", sha256_hex(&bin));
    let (t, s) = online_update(fake_github_with(bin, Some(sum), None).await).await;
    assert_eq!(s["last"]["ok"], false, "{}", s);
    assert!(s["last"]["message"].as_str().unwrap().contains("non signée"), "{}", s);
    assert!(!t.dir.path().join("bin/aurion").exists(), "nothing installed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_refuses_a_release_signed_by_someone_else() {
    // A compromised GitHub account could publish a binary and its checksum,
    // but not the signature: the private key is not in the repository
    let bin = std::fs::read("/bin/bash").unwrap();
    let sum = format!("{}  aurion-arm64\n", sha256_hex(&bin));
    let stranger = ed25519_compact::KeyPair::from_seed(ed25519_compact::Seed::new([7; 32]));
    let sig = stranger.sk.sign(&bin, None).to_vec();
    let (t, s) = online_update(fake_github_with(bin, Some(sum), Some(sig)).await).await;
    assert_eq!(s["last"]["ok"], false, "{}", s);
    assert!(s["last"]["message"].as_str().unwrap().contains("Signature invalide"), "{}", s);
    assert!(!t.dir.path().join("bin/aurion").exists(), "nothing installed");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_new_version_runs_on_trial_until_confirmed() {
    let bin = std::fs::read("/bin/bash").unwrap();
    let sum = format!("{}  aurion-arm64\n", sha256_hex(&bin));
    let sig = common::sign(&bin);
    let (t, s) = online_update(fake_github_with(bin, Some(sum), Some(sig)).await).await;
    assert_eq!(s["last"]["ok"], true, "{}", s);
    let trial = t.dir.path().join("bin/aurion.trial");
    assert!(trial.exists(), "installed on trial: the helper can put the previous version back");
    // Next start: once it has run long enough, the version confirms itself
    aurion::web::update::startup_checks(t.state.clone(), std::time::Duration::from_millis(10)).await;
    assert!(!trial.exists(), "confirmed");
}

#[tokio::test]
async fn an_automatic_rollback_is_reported_on_the_diagnostics_page() {
    let t = common::env();
    let bin = t.dir.path().join("bin/aurion");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, b"OLD").unwrap();
    // Written by aurion-helper app-rollback after the trial version failed
    std::fs::write(bin.with_extension("rolled-back"), "1.12.1\n").unwrap();
    aurion::web::update::startup_checks(t.state.clone(), std::time::Duration::from_millis(10)).await;
    let s: Value = t.server.get("/api/system/update/status").await.json();
    assert_eq!(s["last"]["ok"], false, "{}", s);
    let msg = s["last"]["message"].as_str().unwrap();
    assert!(msg.contains("1.12.1") && msg.contains("retour automatique"), "{}", msg);
    assert!(!bin.with_extension("rolled-back").exists(), "reported once");
}

#[tokio::test]
async fn a_manual_rollback_ends_the_trial() {
    let t = common::env();
    let bin = t.dir.path().join("bin/aurion");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::fs::write(&bin, b"NEW").unwrap();
    std::fs::write(bin.with_extension("prev"), b"OLD").unwrap();
    std::fs::write(bin.with_extension("trial"), "1.12.1").unwrap();
    t.server.post("/api/system/rollback").await.assert_status_ok();
    assert!(!bin.with_extension("trial").exists(), "no automatic swap back to the version just left");
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
