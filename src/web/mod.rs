use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tokio::sync::RwLock;
use axum::{
    Router,
    routing::{get, post, delete},
};
use tower_http::services::ServeDir;
use tracing::info;

use crate::core::config::AppConfig;
use crate::core::models::Phase;

pub mod routes;
pub mod api;

/// Shared application state for web handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub phase: Arc<RwLock<Phase>>,
    pub latest_preview: Arc<RwLock<Option<Vec<u8>>>>,
    pub log_buffer: Arc<RwLock<Vec<String>>>,
    /// Indique si l'heure système a été synchronisée depuis le téléphone cette session.
    pub time_synced: Arc<AtomicBool>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            phase: Arc::new(RwLock::new(Phase::Arm)),
            latest_preview: Arc::new(RwLock::new(None)),
            log_buffer: Arc::new(RwLock::new(Vec::new())),
            time_synced: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn add_log(&self, msg: String) {
        let mut logs = self.log_buffer.write().await;
        logs.push(msg);
        // Keep last 500 lines
        let len = logs.len();
        if len > 500 {
            logs.drain(0..len - 500);
        }
    }
}

/// Résoudre le dossier des fichiers statiques.
/// En production (systemd), WorkingDirectory = AURION_DIR et les statics
/// sont copiés dans static/ à la racine du projet.
/// En développement, fallback sur src/web/static/.
fn resolve_static_dir() -> std::path::PathBuf {
    let prod = std::path::Path::new("static");
    if prod.exists() {
        prod.to_path_buf()
    } else {
        std::path::PathBuf::from("src/web/static")
    }
}

/// Build the full Axum router with all routes and the given state.
/// Exposed as pub so tests can create a TestServer without binding a port.
pub fn build_router(state: AppState) -> Router<()> {
    let static_dir = resolve_static_dir();
    info!("Static files served from: {:?}", static_dir);

    Router::new()
        // Captive portal detection routes (must be before fallback)
        .route("/hotspot-detect.html", get(api::captive_apple))
        .route("/library/test/success.html", get(api::captive_apple))
        .route("/generate_204", get(api::captive_android))
        .route("/gen_204", get(api::captive_android))
        .route("/connecttest.txt", get(api::captive_windows))
        .route("/redirect", get(api::captive_windows))
        .route("/canonical.html", get(api::captive_firefox))
        .route("/success.txt", get(api::captive_fallback))
        // API routes
        .route("/api/status", get(api::get_status))
        .route("/api/preview", get(api::get_preview))
        .route("/api/preview/capture", post(api::capture_preview))
        .route("/api/config", get(api::get_config))
        .route("/api/config", post(api::update_config))
        .route("/api/config/wifi-password", get(api::get_wifi_password))
        .route("/api/config/is-default-password", get(api::is_default_password))
        .route("/api/presets", get(api::get_presets))
        .route("/api/presets", post(api::save_preset))
        .route("/api/presets/:name/apply", post(api::apply_preset))
        .route("/api/disconnect", post(api::disconnect))
        .route("/api/storage", get(api::get_storage))
        .route("/api/logs", get(api::get_logs))
        .route("/api/diagnostics", get(api::get_diagnostics))
        .route("/api/system/time", post(api::set_system_time))
        .route("/api/system/shutdown", post(api::system_shutdown))
        .route("/api/system/update", post(api::system_update))
        // Wi-Fi client (maintenance mode)
        .route("/api/wifi/scan", get(api::wifi_scan))
        .route("/api/wifi/connect", post(api::wifi_connect))
        .route("/api/wifi/status", get(api::wifi_status))
        // Gallery (Recovery mode)
        .route("/api/gallery", get(api::get_gallery))
        .route("/api/gallery/sessions", get(api::get_gallery_sessions))
        .route("/api/gallery/stats", get(api::get_gallery_stats))
        .route("/api/gallery/delete", post(api::delete_gallery_images))
        .route("/api/gallery/download-zip", post(api::download_gallery_zip))
        .route("/api/gallery/sessions/:name/download", get(api::download_gallery_session_zip))
        .route("/api/gallery/sessions/:name", delete(api::delete_gallery_session))
        .route("/api/gallery/:filename", get(api::get_gallery_image))
        .route("/api/gallery/thumbnail/:filename", get(api::get_gallery_thumbnail))
        // Static file serving
        .fallback_service(ServeDir::new(static_dir))
        .with_state(state)
}

/// Start the web server on the configured port.
pub async fn start_server(state: AppState, port: u16) -> anyhow::Result<()> {
    let app = build_router(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("Web server starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
