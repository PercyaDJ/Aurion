//! Gallery / recovery mode tests on a simulated USB drive.

mod common;

use axum::http::StatusCode;
use serde_json::{json, Value};

async fn unzip(bytes: Vec<u8>) -> Vec<(String, Vec<u8>)> {
    use futures_lite::io::AsyncReadExt;
    let reader = async_zip::base::read::mem::ZipFileReader::new(bytes).await.expect("valid zip");
    let mut out = Vec::new();
    for i in 0..reader.file().entries().len() {
        let name = reader.file().entries()[i].filename().as_str().unwrap().to_string();
        let mut entry = reader.reader_with_entry(i).await.unwrap();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).await.unwrap();
        out.push((name, buf));
    }
    out
}

#[tokio::test]
async fn gallery_lists_only_images() {
    let t = common::env();
    t.add_file("aurora_20260305_213100_00000.jpg", &common::tiny_jpeg(8, 8));
    t.add_file("aurora_20260305_213110_00001_AURORA.jpg", &common::tiny_jpeg(8, 8));
    t.add_file("aurora_20260305_213110_00001_AURORA.dng", b"DNG");
    t.add_file("notes.txt", b"x");
    t.add_file("thumbs/aurora_20260305_213100_00000.jpg", &common::tiny_jpeg(4, 4));

    let body: Value = t.server.get("/api/gallery").await.json();
    assert_eq!(body["total_count"], 3);
    let names: Vec<&str> = body["images"].as_array().unwrap().iter().map(|i| i["filename"].as_str().unwrap()).collect();
    assert_eq!(names, ["aurora_20260305_213100_00000.jpg", "aurora_20260305_213110_00001_AURORA.dng", "aurora_20260305_213110_00001_AURORA.jpg"]);
    assert_eq!(body["images"][2]["aurora"], true);

    let stats: Value = t.server.get("/api/gallery/stats").await.json();
    assert_eq!(stats["total_images"], 3);
    assert!(stats["last_session_date"].is_string());
}

#[tokio::test]
async fn gallery_empty_or_missing_drive() {
    let t = common::env_with(|c| c.storage.mount_point = "/nonexistent/aurion".into());
    let body: Value = t.server.get("/api/gallery").await.json();
    assert_eq!(body["total_count"], 0);
    let sessions: Value = t.server.get("/api/gallery/sessions").await.json();
    assert_eq!(sessions, json!([]));
}

#[tokio::test]
async fn image_download_and_inline() {
    let t = common::env();
    let jpg = common::tiny_jpeg(16, 16);
    t.add_file("aurora_20260305_213100_00000.jpg", &jpg);
    let res = t.server.get("/api/gallery/aurora_20260305_213100_00000.jpg").await;
    res.assert_status_ok();
    assert_eq!(res.header("content-type"), "image/jpeg");
    assert!(res.header("content-disposition").to_str().unwrap().starts_with("attachment"));
    assert_eq!(res.as_bytes().to_vec(), jpg);

    let inline = t.server.get("/api/gallery/aurora_20260305_213100_00000.jpg?inline=1").await;
    assert!(inline.header("content-disposition").to_str().unwrap().starts_with("inline"));
    t.server.get("/api/gallery/aurora_20990101_000000_00000.jpg").await.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn thumbnails_prefer_capture_thumbs_then_generate() {
    let t = common::env();
    let pre = common::tiny_jpeg(4, 4);
    t.add_file("aurora_20260305_213100_00000.jpg", &common::tiny_jpeg(1000, 800));
    t.add_file("thumbs/aurora_20260305_213100_00000.jpg", &pre);
    let res = t.server.get("/api/gallery/thumbnail/aurora_20260305_213100_00000.jpg").await;
    res.assert_status_ok();
    assert_eq!(res.as_bytes().to_vec(), pre, "the thumbnail written during the capture is reused");

    // No pre-generated thumb: generated at 320 px and cached
    t.add_file("aurora_20260305_213200_00001.jpg", &common::tiny_jpeg(1000, 800));
    let res = t.server.get("/api/gallery/thumbnail/aurora_20260305_213200_00001.jpg").await;
    res.assert_status_ok();
    let img = image::load_from_memory(res.as_bytes()).unwrap();
    assert!(img.width() <= 320 && img.height() <= 240 && img.width() >= 200);
    assert!(t.dir.path().join("thumb_cache/aurora_20260305_213200_00001.jpg").exists());

    // RAW has no thumbnail, corrupted JPEG is reported
    t.add_file("aurora_20260305_213300_00002.dng", b"DNG");
    t.server.get("/api/gallery/thumbnail/aurora_20260305_213300_00002.dng").await.assert_status(StatusCode::NO_CONTENT);
    t.add_file("aurora_20260305_213400_00003.jpg", b"not a jpeg");
    t.server.get("/api/gallery/thumbnail/aurora_20260305_213400_00003.jpg").await.assert_status(StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn delete_images_also_removes_thumbnails() {
    let t = common::env();
    let img = t.add_file("aurora_20260305_213100_00000.jpg", &common::tiny_jpeg(8, 8));
    let thumb = t.add_file("thumbs/aurora_20260305_213100_00000.jpg", &common::tiny_jpeg(4, 4));
    let body: Value = t.server.post("/api/gallery/delete")
        .json(&json!({"filenames": ["aurora_20260305_213100_00000.jpg", "aurora_20990101_000000_00000.jpg"]}))
        .await.json();
    assert_eq!(body["deleted"], 1);
    assert_eq!(body["errors"].as_array().unwrap().len(), 1);
    assert!(!img.exists() && !thumb.exists());
}

#[tokio::test]
async fn sessions_group_images_and_count_auroras() {
    let t = common::env();
    t.add_session("2026-03-05_21-30");
    t.add_session("2026-03-06_22-00");
    for f in [
        "aurora_20260305_213100_00000.jpg",
        "aurora_20260306_013100_00001_AURORA.jpg",
        "aurora_20260306_013100_00001_AURORA.dng",
        "aurora_20260306_220500_00000.jpg",
    ] {
        t.add_file(f, b"data");
    }
    let sessions: Value = t.server.get("/api/gallery/sessions").await.json();
    let arr = sessions.as_array().unwrap();
    assert_eq!(arr.len(), 2);
    // Most recent first
    assert_eq!(arr[0]["name"], "2026-03-06_22-00");
    assert_eq!(arr[0]["image_count"], 1);
    assert_eq!(arr[1]["image_count"], 3);
    assert_eq!(arr[1]["aurora_count"], 1, "RAW+JPG pair counts as one aurora");
    assert_eq!(arr[1]["duration_minutes"], 240);
    assert_eq!(arr[1]["has_log"], true);
    assert_eq!(arr[1]["date"], "2026/03/05");
}

#[tokio::test]
async fn session_zip_contains_images_and_logs() {
    let t = common::env();
    t.add_session("2026-03-05_21-30");
    t.add_file("aurora_20260305_213100_00000.jpg", b"IMG-A");
    t.add_file("aurora_20260305_223100_00001.jpg", b"IMG-B");
    t.add_file("aurora_20260310_223100_00001.jpg", b"OTHER NIGHT");

    let res = t.server.get("/api/gallery/sessions/2026-03-05_21-30/download").await;
    res.assert_status_ok();
    assert_eq!(res.header("content-type"), "application/zip");
    let entries = unzip(res.as_bytes().to_vec()).await;
    let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, [
        "aurora_20260305_213100_00000.jpg",
        "aurora_20260305_223100_00001.jpg",
        "sessions/2026-03-05_21-30/event.jsonl",
        "sessions/2026-03-05_21-30/session.log",
    ]);
    assert_eq!(entries[0].1, b"IMG-A");
    assert_eq!(entries[1].1, b"IMG-B");
}

#[tokio::test]
async fn selection_zip_download() {
    let t = common::env();
    t.add_file("aurora_20260305_213100_00000.jpg", b"A");
    t.add_file("aurora_20260305_213200_00001.jpg", b"B");
    let res = t.server.post("/api/gallery/download-zip")
        .form(&[("filenames", "aurora_20260305_213200_00001.jpg, aurora_20260305_213100_00000.jpg,missing.jpg,../x.jpg")])
        .await;
    res.assert_status_ok();
    assert!(res.header("content-disposition").to_str().unwrap().contains("aurion_"));
    let entries = unzip(res.as_bytes().to_vec()).await;
    assert_eq!(entries.len(), 2, "missing and invalid files are skipped");
    assert_eq!(entries[0], ("aurora_20260305_213100_00000.jpg".to_string(), b"A".to_vec()));
}

#[tokio::test]
async fn delete_session_removes_its_images_only() {
    let t = common::env();
    let dir = t.add_session("2026-03-05_21-30");
    let mine = t.add_file("aurora_20260305_213100_00000.jpg", b"A");
    let other = t.add_file("aurora_20260310_213100_00000.jpg", b"B");
    t.server.delete("/api/gallery/sessions/2026-03-05_21-30").await.assert_status_ok();
    assert!(!dir.exists() && !mine.exists());
    assert!(other.exists(), "images of other nights are kept");
    t.server.delete("/api/gallery/sessions/2026-03-05_21-30").await.assert_status(StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_zip_raw_only_or_jpg_only() {
    let t = common::env();
    t.add_session("2026-03-05_21-30");
    t.add_file("aurora_20260305_213100_00000.jpg", b"J");
    t.add_file("aurora_20260305_213100_00000.dng", b"R");
    t.add_file("aurora_20260305_223100_00001_AURORA.dng", b"R2");

    let res = t.server.get("/api/gallery/sessions/2026-03-05_21-30/download?only=raw").await;
    res.assert_status_ok();
    assert!(res.header("content-disposition").to_str().unwrap().contains("aurion_2026-03-05_21-30_RAW.zip"));
    let names: Vec<String> = unzip(res.as_bytes().to_vec()).await.into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().all(|n| !n.ends_with(".jpg")), "{:?}", names);
    assert!(names.contains(&"aurora_20260305_223100_00001_AURORA.dng".to_string()));

    let res = t.server.get("/api/gallery/sessions/2026-03-05_21-30/download?only=jpg").await;
    let names: Vec<String> = unzip(res.as_bytes().to_vec()).await.into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().all(|n| !n.ends_with(".dng")), "{:?}", names);
    assert!(names.contains(&"aurora_20260305_213100_00000.jpg".to_string()));

    // Unknown filter value is rejected, not silently ignored
    let res = t.server.get("/api/gallery/sessions/2026-03-05_21-30/download?only=exe").await;
    assert!(res.status_code().is_client_error());
}

#[tokio::test]
async fn last_night_summary() {
    let t = common::env();
    let none: Value = t.server.get("/api/night/last").await.json();
    assert!(none.is_null(), "no night yet");

    t.add_session("2026-03-05_21-30");
    t.add_session("2026-03-06_22-00");
    std::fs::write(
        t.capture_dir().join("sessions/2026-03-06_22-00/event.jsonl"),
        b"{\"aurora_score\":2.5,\"aurora_detected\":true}\n{\"aurora_score\":6.25,\"aurora_detected\":true}\n",
    )
    .unwrap();
    for f in ["aurora_20260305_213100_00000.jpg", "aurora_20260306_220500_00000_AURORA.jpg", "aurora_20260306_220500_00000_AURORA.dng"] {
        t.add_file(f, b"data");
    }
    let n: Value = t.server.get("/api/night/last").await.json();
    assert_eq!(n["session"]["name"], "2026-03-06_22-00", "most recent night");
    assert_eq!(n["raw_count"], 1);
    assert_eq!(n["session"]["aurora_count"], 1);
    assert_eq!(n["best_score"], 6.25);
}
