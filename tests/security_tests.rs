//! Security tests: path traversal, injection, CSRF / DNS rebinding,
//! secret leaks, upload validation, headers.

mod common;

use axum::http::{HeaderName, HeaderValue, StatusCode};
use serde_json::{json, Value};

const TRAVERSALS: [&str; 8] = [
    "..%2F..%2Fetc%2Fpasswd",
    "..%2Fconfig%2Faurion.json",
    "%2Fetc%2Fpasswd",
    "..",
    ".hidden.jpg",
    "a%5Cb.jpg",
    "evil%0Aname.jpg",
    "x%22.jpg",
];

#[tokio::test]
async fn image_download_rejects_traversal() {
    let t = common::env();
    // A secret one level above the capture dir must stay unreachable
    std::fs::write(t.dir.path().join("secret.jpg"), b"secret").unwrap();
    for name in TRAVERSALS.iter().chain(["..%2Fsecret.jpg"].iter()) {
        let res = t.server.get(&format!("/api/gallery/{}", name)).await;
        assert!(res.status_code() == StatusCode::BAD_REQUEST || res.status_code() == StatusCode::NOT_FOUND,
            "{} → {}", name, res.status_code());
        assert!(!res.text().contains("secret"), "{} leaked data", name);
    }
}

#[tokio::test]
async fn thumbnail_rejects_traversal() {
    let t = common::env();
    for name in TRAVERSALS {
        let res = t.server.get(&format!("/api/gallery/thumbnail/{}", name)).await;
        assert!(matches!(res.status_code(), StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "{}", name);
    }
}

#[tokio::test]
async fn non_image_files_are_never_served() {
    let t = common::env();
    t.add_file("notes.txt", b"private");
    t.server.get("/api/gallery/notes.txt").await.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn delete_rejects_traversal_and_foreign_files() {
    let t = common::env();
    let outside = t.dir.path().join("victim.jpg");
    std::fs::write(&outside, b"x").unwrap();
    let config_file = t.config_dir().join("aurion.json");
    std::fs::write(&config_file, b"{}").unwrap();
    let txt = t.add_file("keep.txt", b"x");

    let payload = json!({ "filenames": [
        "../victim.jpg", "/etc/passwd", "../config/aurion.json", "sessions/../../victim.jpg",
        "subdir/evil.jpg", "keep.txt", "..\\victim.jpg", ""
    ]});
    let body: Value = t.server.post("/api/gallery/delete").json(&payload).await.json();
    assert_eq!(body["deleted"], 0);
    assert_eq!(body["errors"].as_array().unwrap().len(), 8);
    assert!(outside.exists() && config_file.exists() && txt.exists());
}

#[tokio::test]
async fn session_routes_reject_traversal() {
    let t = common::env();
    std::fs::create_dir_all(t.dir.path().join("important")).unwrap();
    for name in ["..", "..%2F..%2Fimportant", "%2Ftmp", ".git"] {
        let del = t.server.delete(&format!("/api/gallery/sessions/{}", name)).await;
        assert!(matches!(del.status_code(), StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED), "{} → {}", name, del.status_code());
        let dl = t.server.get(&format!("/api/gallery/sessions/{}/download", name)).await;
        assert!(matches!(dl.status_code(), StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "{}", name);
    }
    assert!(t.dir.path().join("important").exists());
}

#[tokio::test]
async fn zip_ignores_traversal_entries() {
    let t = common::env();
    std::fs::write(t.dir.path().join("secret.jpg"), b"secret").unwrap();
    let res = t.server.post("/api/gallery/download-zip")
        .form(&[("filenames", "../secret.jpg,/etc/passwd")])
        .await;
    res.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn preset_names_cannot_escape_the_presets_dir() {
    let t = common::env();
    for name in ["../aurion", "../../evil", "a/b", "x.json", "", "   "] {
        let res = t.server.post("/api/presets").json(&json!({ "name": name })).await;
        res.assert_status(StatusCode::BAD_REQUEST);
    }
    // No file created outside presets/
    assert!(!t.config_dir().join("aurion.json").exists());
    assert!(!t.dir.path().join("evil.json").exists());
    t.server.post("/api/presets/..%2Faurion/apply").await.assert_status(StatusCode::BAD_REQUEST);
    t.server.delete("/api/presets/..%2Faurion").await.assert_status(StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn wifi_password_is_never_exposed() {
    let t = common::env_with(|c| c.network.password = "SuperSecretWifi99".into());
    for url in ["/api/config", "/api/status", "/api/diagnostics", "/api/logs", "/api/presets"] {
        let body = t.server.get(url).await.text();
        assert!(!body.contains("SuperSecretWifi99"), "{} leaks the Wi-Fi password", url);
    }
    // The old endpoint returning the clear password no longer exists
    t.server.get("/api/config/wifi-password").await.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn wifi_config_injection_rejected() {
    let t = common::env();
    let mut cfg: Value = t.server.get("/api/config").await.json();
    for (field, value) in [
        ("ssid", json!("Aurion\nwpa=0")),
        ("ssid", json!("")),
        ("ssid", json!("a".repeat(40))),
        ("password", json!("court")),
        ("password", json!("mot\nde passe long")),
        ("password", json!("x".repeat(64))),
        ("channel", json!(0)),
        ("channel", json!(200)),
    ] {
        let mut c = cfg.clone();
        c["network"][field] = value.clone();
        let res = t.server.post("/api/config").json(&c).await;
        res.assert_status(StatusCode::BAD_REQUEST);
    }
    cfg["network"]["ssid"] = json!("Aurion");
    t.server.post("/api/config").json(&cfg).await.assert_status_ok();
}

#[tokio::test]
async fn wifi_connect_validates_before_anything() {
    let t = common::env();
    for body in [
        json!({"ssid": "Maison\nnetwork={", "password": "motdepasse"}),
        json!({"ssid": "Maison", "password": "court"}),
        json!({"ssid": "Maison", "password": "pass\"word\nkey"}),
    ] {
        t.server.post("/api/wifi/connect").json(&body).await.assert_status(StatusCode::BAD_REQUEST);
    }
}

fn header(name: &'static str, value: &str) -> (HeaderName, HeaderValue) {
    (HeaderName::from_static(name), HeaderValue::from_str(value).unwrap())
}

#[tokio::test]
async fn csrf_cross_origin_writes_are_blocked() {
    let t = common::env();
    let (k, v) = header("origin", "http://evil.example.com");
    t.server.post("/api/disconnect").add_header(k.clone(), v.clone()).await.assert_status(StatusCode::FORBIDDEN);
    t.server.post("/api/gallery/delete").add_header(k.clone(), v.clone())
        .json(&json!({"filenames": []})).await.assert_status(StatusCode::FORBIDDEN);
    let (k2, v2) = header("sec-fetch-site", "cross-site");
    t.server.post("/api/system/shutdown").add_header(k2, v2).await.assert_status(StatusCode::FORBIDDEN);
    // Nothing happened
    assert_eq!(t.state.current_phase().await, aurion::core::models::Phase::Arm);
    // Reads stay allowed (and harmless)
    t.server.get("/api/status").add_header(k, v).await.assert_status_ok();
}

#[tokio::test]
async fn same_origin_writes_are_allowed() {
    let t = common::env();
    let (h, hv) = header("host", "192.168.4.1:8080");
    let (o, ov) = header("origin", "http://192.168.4.1:8080");
    t.server.post("/api/disconnect").add_header(h, hv).add_header(o, ov).await.assert_status_ok();
}

#[tokio::test]
async fn dns_rebinding_hosts_are_blocked() {
    let t = common::env();
    for host in ["evil.example.com", "192.168.4.1.nip.io.evil.com:8080"] {
        let (k, v) = header("host", host);
        t.server.get("/api/config").add_header(k, v).await.assert_status(StatusCode::FORBIDDEN);
    }
    for host in ["192.168.4.1:8080", "aurion.local:8080", "localhost:8080", "[::1]:8080"] {
        let (k, v) = header("host", host);
        t.server.get("/api/config").add_header(k, v).await.assert_status_ok();
    }
}

#[tokio::test]
async fn security_headers_present() {
    let t = common::env();
    for url in ["/", "/api/status"] {
        let res = t.server.get(url).await;
        assert_eq!(res.header("x-content-type-options"), "nosniff");
        assert_eq!(res.header("x-frame-options"), "DENY");
        assert!(res.header("content-security-policy").to_str().unwrap().contains("frame-ancestors 'none'"));
    }
}

#[tokio::test]
async fn oversized_body_rejected() {
    let t = common::env();
    let names: Vec<String> = (0..200_000).map(|i| format!("aurora_{}.jpg", i)).collect();
    let res = t.server.post("/api/gallery/delete").json(&json!({ "filenames": names })).await;
    res.assert_status(StatusCode::PAYLOAD_TOO_LARGE);
}

fn multipart_with(bytes: Vec<u8>) -> axum_test::multipart::MultipartForm {
    axum_test::multipart::MultipartForm::new()
        .add_part("binary", axum_test::multipart::Part::bytes(bytes).file_name("aurion"))
}

#[tokio::test]
async fn ota_update_rejects_garbage_and_foreign_binaries() {
    let t = common::env();
    let target = t.dir.path().join("bin/aurion");

    let res = t.server.post("/api/system/update").multipart(multipart_with(b"#!/bin/sh\nrm -rf /\n".to_vec())).await;
    res.assert_status(StatusCode::BAD_REQUEST);
    assert!(res.text().contains("ELF"));

    // Valid ELF header but wrong architecture
    let mut foreign = std::fs::read("/bin/true").unwrap();
    let other: u16 = if cfg!(target_arch = "aarch64") { 62 } else { 183 };
    foreign[18..20].copy_from_slice(&other.to_le_bytes());
    t.server.post("/api/system/update").multipart(multipart_with(foreign)).await.assert_status(StatusCode::BAD_REQUEST);

    // Empty upload
    t.server.post("/api/system/update").multipart(multipart_with(Vec::new())).await.assert_status(StatusCode::BAD_REQUEST);

    assert!(!target.exists(), "nothing must be installed");
}

#[tokio::test]
async fn ota_update_refused_during_capture() {
    let t = common::env();
    *t.state.phase.write().await = aurion::core::models::Phase::Run;
    let res = t.server.post("/api/system/update").multipart(multipart_with(std::fs::read("/bin/true").unwrap())).await;
    res.assert_status(StatusCode::CONFLICT);
}

#[tokio::test]
async fn ota_update_installs_and_keeps_backup() {
    let t = common::env();
    let target = t.dir.path().join("bin/aurion");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"old version").unwrap();

    // /bin/true is a real executable for this machine that accepts --version
    let new_bin = std::fs::read("/bin/true").unwrap();
    let res = t.server.post("/api/system/update").multipart(multipart_with(new_bin.clone())).await;
    res.assert_status_ok();

    assert_eq!(std::fs::read(&target).unwrap(), new_bin);
    assert_eq!(std::fs::read(target.with_extension("prev")).unwrap(), b"old version");
    assert!(!target.with_extension("new").exists());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o111, 0o111);
}

#[tokio::test]
async fn ota_update_rejects_binary_that_does_not_start() {
    let t = common::env();
    let target = t.dir.path().join("bin/aurion");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, b"old version").unwrap();
    // /bin/false: valid ELF for this arch, but exits with an error
    let res = t.server.post("/api/system/update").multipart(multipart_with(std::fs::read("/bin/false").unwrap())).await;
    res.assert_status(StatusCode::BAD_REQUEST);
    assert_eq!(std::fs::read(&target).unwrap(), b"old version", "current binary must be untouched");
}
