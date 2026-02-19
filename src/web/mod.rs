use std::sync::Arc;
use tokio::sync::RwLock;
use axum::{
    Router,
    routing::{get, post},
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
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            phase: Arc::new(RwLock::new(Phase::Arm)),
            latest_preview: Arc::new(RwLock::new(None)),
            log_buffer: Arc::new(RwLock::new(Vec::new())),
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

/// Start the web server on the configured port.
pub async fn start_server(state: AppState, port: u16) -> anyhow::Result<()> {
    let app = Router::new()
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
        .route("/api/config", get(api::get_config))
        .route("/api/config", post(api::update_config))
        .route("/api/presets", get(api::get_presets))
        .route("/api/presets", post(api::save_preset))
        .route("/api/presets/{name}/apply", post(api::apply_preset))
        .route("/api/disconnect", post(api::disconnect))
        .route("/api/storage", get(api::get_storage))
        .route("/api/logs", get(api::get_logs))
        // Static file serving
        .fallback_service(ServeDir::new("src/web/static"))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    info!("Web server starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
