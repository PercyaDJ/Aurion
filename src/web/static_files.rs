//! Web interface embedded in the binary.
//!
//! The HTML/CSS/JS files of `src/web/static/` are compiled into the
//! executable: deploying Aurion means copying a single file, and an OTA
//! binary update also updates the interface. In debug builds rust-embed
//! reads the files from disk, so the UI can be edited without recompiling.

use axum::{
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "src/web/static/"]
struct Assets;

/// Resolve a request path to an embedded asset name.
fn asset_name(path: &str) -> String {
    let p = path.trim_start_matches('/');
    if p.is_empty() || p.ends_with('/') {
        format!("{}index.html", p)
    } else {
        p.to_string()
    }
}

pub async fn serve_static(uri: Uri) -> Response {
    let name = asset_name(uri.path());
    if name.contains("..") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    match Assets::get(&name) {
        Some(file) => {
            let mime = file.metadata.mimetype().to_string();
            let cache = if mime.starts_with("text/html") {
                "no-cache"
            } else {
                "public, max-age=3600"
            };
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, cache.to_string())],
                file.data.into_owned(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Page introuvable").into_response(),
    }
}

/// Names of all embedded assets (used by tests).
pub fn asset_names() -> Vec<String> {
    Assets::iter().map(|n| n.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_resolution() {
        assert_eq!(asset_name("/"), "index.html");
        assert_eq!(asset_name("/css/style.css"), "css/style.css");
    }

    #[test]
    fn all_pages_are_embedded() {
        let names = asset_names();
        for page in [
            "index.html", "dashboard.html", "gallery.html", "settings.html",
            "settings_advanced.html", "presets.html", "preview.html", "storage.html",
            "diagnostics.html", "css/style.css", "js/toast.js", "js/common.js", "manifest.json",
        ] {
            assert!(names.iter().any(|n| n == page), "{} manquant dans le binaire", page);
        }
    }
}
