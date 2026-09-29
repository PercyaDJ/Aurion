//! Online update from GitHub (against a local fake GitHub) and rollback.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

const FAKE_BINARY: &str = "#!/bin/sh\necho not an aurion binary\n";

/// Signature published next to the binary by the fake GitHub.
#[derive(Clone, Copy)]
enum Sig {
    /// Signed with the key the test installs as release key.
    Valid,
    /// Signed with another key.
    OtherKey,
    /// Signature file of the right key, corrupted.
    Corrupted,
    /// No `aurion-arm64.sig` in the release.
    Missing,
}

fn release_key() -> (ed25519_dalek::SigningKey, String) {
    use ed25519_dalek::pkcs8::{spki::der::pem::LineEnding, EncodePublicKey};
    let key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
    let pem = key.verifying_key().to_public_key_pem(LineEnding::LF).unwrap();
    (key, pem)
}

/// A fake GitHub: release description, a "binary" that is not an ELF and
/// its signature.
async fn fake_github(sig: Sig) -> String {
    use axum::{routing::get, Router};
    use ed25519_dalek::Signer;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut assets = vec![json!({"name": "aurion-arm64", "browser_download_url": format!("{}/dl/edge/aurion-arm64", base)})];
    if !matches!(sig, Sig::Missing) {
        assets.push(json!({"name": "aurion-arm64.sig", "browser_download_url": format!("{}/dl/edge/aurion-arm64.sig", base)}));
    }
    let release = json!({"tag_name": "edge", "assets": assets});
    let signer = match sig {
        Sig::OtherKey => ed25519_dalek::SigningKey::from_bytes(&[43; 32]),
        _ => release_key().0,
    };
    let mut signature = signer.sign(FAKE_BINARY.as_bytes()).to_bytes().to_vec();
    if matches!(sig, Sig::Corrupted) {
        signature[0] ^= 0xff;
    }
    let app = Router::new()
        .route("/repos/PercyaDJ/Aurion/releases/tags/edge", get(move || { let r = release.clone(); async move { axum::Json(r) } }))
        .route("/dl/edge/aurion-arm64", get(|| async { FAKE_BINARY }))
        .route("/dl/edge/aurion-arm64.sig", get(move || { let s = signature.clone(); async move { s } }));
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

/// Run a "dev" update against a fake GitHub, return the final status.
async fn online_update(sig: Sig) -> (common::TestEnv, Value) {
    let base = fake_github(sig).await;
    let t = common::env_custom(|_| {}, |u| {
        u.github_api = base.clone();
        u.download_prefix = format!("{}/dl/", base);
        u.release_public_key = release_key().1;
    });
    // Pi already on a network with internet: no Wi-Fi to join
    let res = t.server.post("/api/system/update/online").json(&json!({"channel": "dev"})).await;
    res.assert_status_ok();
    let s = wait_done(&t).await;
    assert_eq!(s["last"]["ok"], false, "{}", s);
    assert!(!t.dir.path().join("bin/aurion").exists(), "nothing installed");
    (t, s)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_downloads_from_github_and_checks_the_binary() {
    // Signature accepted: the binary itself is then checked, and refused
    let (_t, s) = online_update(Sig::Valid).await;
    assert!(s["last"]["message"].as_str().unwrap().contains("ELF"), "downloaded, signature ok, then refused: {}", s);
    assert_eq!(s["version"], aurion::VERSION);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn online_update_refuses_bad_or_missing_signatures() {
    for (sig, expected) in [
        (Sig::Missing, "n'est pas signée"),
        (Sig::OtherKey, "Signature invalide"),
        (Sig::Corrupted, "Signature invalide"),
    ] {
        let (_t, s) = online_update(sig).await;
        let message = s["last"]["message"].as_str().unwrap();
        assert!(message.contains(expected), "expected '{}': {}", expected, s);
        assert!(message.contains("refusée"), "{}", s);
    }
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
