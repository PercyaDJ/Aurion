use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use axum::{
    extract::DefaultBodyLimit,
    routing::{delete, get, post},
    Router,
};
use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::core::config::AppConfig;
use crate::core::models::Phase;

pub mod api;
pub mod captive;
pub mod gallery;
pub mod security;
pub mod static_files;
pub mod update;

/// Maximum size of a regular API request body.
const API_BODY_LIMIT: usize = 1024 * 1024;
/// Maximum size of an uploaded Aurion binary (OTA update).
pub const UPDATE_BODY_LIMIT: usize = 128 * 1024 * 1024;
/// Number of log lines kept in memory for the diagnostics page.
const LOG_CAPACITY: usize = 500;

/// Filesystem locations used by the web layer. Injected so tests can run
/// against temporary directories instead of the real device paths.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Persistent config file (`config/aurion.json`).
    pub config_file: PathBuf,
    /// Directory holding user presets.
    pub presets_dir: PathBuf,
    /// Cache for gallery thumbnails generated on demand.
    pub thumb_cache_dir: PathBuf,
    /// Scratch directory for preview captures.
    pub tmp_dir: PathBuf,
    /// Marker of the night in progress (resume after a power cut).
    pub night_marker: PathBuf,
}

impl Paths {
    /// Paths relative to a config directory (production: `config/`).
    pub fn from_config_dir(config_dir: &Path) -> Self {
        Self {
            config_file: config_dir.join("aurion.json"),
            presets_dir: config_dir.join("presets"),
            thumb_cache_dir: std::env::temp_dir().join("aurion_thumbnails"),
            tmp_dir: std::env::temp_dir(),
            night_marker: config_dir.join("night.json"),
        }
    }
}

impl Default for Paths {
    fn default() -> Self {
        Self::from_config_dir(Path::new("config"))
    }
}

/// How an uploaded binary is installed.
#[derive(Debug, Clone)]
pub struct UpdateSettings {
    /// Binary to replace. `None` = the running executable.
    pub target: Option<PathBuf>,
    /// Exit the process after a successful update so systemd restarts the
    /// new binary (`Restart=always`). Disabled in tests.
    pub restart: bool,
    /// GitHub API used by the online update (a local server in tests).
    pub github_api: String,
    /// Downloads must start with this prefix (the project's releases).
    pub download_prefix: String,
}

impl Default for UpdateSettings {
    fn default() -> Self {
        Self {
            target: None,
            restart: true,
            github_api: "https://api.github.com".into(),
            download_prefix: format!("https://github.com/{}/releases/download/", crate::web::update::REPO),
        }
    }
}

/// Shared application state for web handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<RwLock<AppConfig>>,
    pub phase: Arc<RwLock<Phase>>,
    pub latest_preview: Arc<RwLock<Option<Vec<u8>>>>,
    pub log_buffer: Arc<RwLock<Vec<String>>>,
    /// Indique si l'heure système a été synchronisée depuis le téléphone cette session.
    pub time_synced: Arc<AtomicBool>,
    pub paths: Arc<Paths>,
    pub update: Arc<UpdateSettings>,
    /// Serialises camera access for previews (rpicam-still is exclusive).
    pub camera_lock: Arc<Mutex<()>>,
    /// Allow real system actions (shutdown, Wi-Fi, clock) through the
    /// privileged helper. False on PC builds and in tests.
    pub system_actions: bool,
    /// Exposure of the last preview (default settings for dark frames).
    pub last_preview: Arc<RwLock<Option<crate::core::models::ExposureSettings>>>,
    /// Progress of a dark frame series: (done, total, last error).
    pub darks: Arc<RwLock<Option<DarkProgress>>>,
    /// Last camera detection result (checking costs a process launch).
    pub camera_check: Arc<RwLock<Option<(std::time::Instant, bool)>>>,
    /// An interrupted night will resume at this instant unless cancelled.
    pub resume_at: Arc<RwLock<Option<tokio::time::Instant>>>,
    /// Last request to the API (someone is using the interface).
    pub last_activity: Arc<std::sync::Mutex<tokio::time::Instant>>,
    /// The system clock was set from the hardware clock (RTC) at startup.
    pub clock_from_rtc: Arc<AtomicBool>,
    /// Expedition mode: state of the automatic start.
    pub auto_start: Arc<RwLock<AutoStart>>,
    /// An online update is running in this process.
    pub online_update_running: Arc<AtomicBool>,
    /// Seconds after power-on when the Aurion Wi-Fi was ready.
    pub hotspot_ready_at: Arc<std::sync::Mutex<Option<f64>>>,
}

/// Automatic start of the night (expedition mode), shown on the home screen.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AutoStart {
    /// Not in expedition mode, or the night already started.
    #[default]
    Off,
    /// The night starts at this instant if nobody uses the interface.
    At(tokio::time::Instant),
    /// Waiting for a condition (reason shown to the user).
    Blocked(&'static str),
}

/// Progress of the dark frame capture (see `api::capture_darks`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DarkProgress {
    pub done: u32,
    pub total: u32,
    pub iso: u32,
    pub shutter_us: u64,
    pub running: bool,
    pub error: Option<String>,
}

impl AppState {
    pub fn new(config: AppConfig) -> Self {
        Self::with_paths(config, Paths::default())
    }

    pub fn with_paths(config: AppConfig, paths: Paths) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            phase: Arc::new(RwLock::new(Phase::Arm)),
            latest_preview: Arc::new(RwLock::new(None)),
            log_buffer: Arc::new(RwLock::new(Vec::new())),
            time_synced: Arc::new(AtomicBool::new(false)),
            paths: Arc::new(paths),
            update: Arc::new(UpdateSettings::default()),
            camera_lock: Arc::new(Mutex::new(())),
            system_actions: cfg!(feature = "rpi"),
            last_preview: Arc::new(RwLock::new(None)),
            darks: Arc::new(RwLock::new(None)),
            camera_check: Arc::new(RwLock::new(None)),
            resume_at: Arc::new(RwLock::new(None)),
            last_activity: Arc::new(std::sync::Mutex::new(tokio::time::Instant::now())),
            clock_from_rtc: Arc::new(AtomicBool::new(false)),
            auto_start: Arc::new(RwLock::new(AutoStart::Off)),
            online_update_running: Arc::new(AtomicBool::new(false)),
            hotspot_ready_at: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Someone used the interface: postpones the automatic start.
    pub fn touch(&self) {
        if let Ok(mut t) = self.last_activity.lock() {
            *t = tokio::time::Instant::now();
        }
    }

    /// Time since the last API request.
    pub fn idle_for(&self) -> std::time::Duration {
        self.last_activity.lock().map(|t| t.elapsed()).unwrap_or_default()
    }

    /// The clock can be trusted: set from the phone this session, or from
    /// the hardware clock at startup.
    pub fn clock_trusted(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.time_synced.load(Relaxed) || self.clock_from_rtc.load(Relaxed)
    }

    /// Override the OTA update behaviour (tests).
    pub fn with_update_settings(mut self, update: UpdateSettings) -> Self {
        self.update = Arc::new(update);
        self
    }

    /// Enable or disable real system actions (tests use `false`).
    pub fn with_system_actions(mut self, enabled: bool) -> Self {
        self.system_actions = enabled;
        self
    }

    pub async fn add_log(&self, msg: String) {
        let line = format!("[{}] {}", chrono::Local::now().format("%H:%M:%S"), msg);
        let mut logs = self.log_buffer.write().await;
        logs.push(line);
        let len = logs.len();
        if len > LOG_CAPACITY {
            logs.drain(0..len - LOG_CAPACITY);
        }
    }

    pub async fn current_phase(&self) -> Phase {
        *self.phase.read().await
    }
}

/// Build the full Axum router with all routes and the given state.
/// Exposed as pub so tests can create a TestServer without binding a port.
pub fn build_router(state: AppState) -> Router<()> {
    let api = Router::new()
        .route("/api/status", get(api::get_status))
        .route("/api/preflight", get(api::get_preflight))
        .route("/api/night/last", get(gallery::get_last_night))
        .route("/api/night/resume/cancel", post(api::cancel_resume))
        .route("/api/preview", get(api::get_preview))
        .route("/api/preview/capture", post(api::capture_preview))
        .route("/api/config", get(api::get_config).post(api::update_config))
        .route("/api/config/is-default-password", get(api::is_default_password))
        .route("/api/presets", get(api::get_presets).post(api::save_preset))
        .route("/api/presets/:name/apply", post(api::apply_preset))
        .route("/api/presets/:name", delete(api::delete_preset))
        .route("/api/disconnect", post(api::disconnect))
        .route("/api/darks", get(api::get_darks).post(api::capture_darks))
        .route("/api/storage", get(api::get_storage))
        .route("/api/storage/usb", get(api::get_usb_devices))
        .route("/api/storage/format", post(api::format_usb))
        .route("/api/logs", get(api::get_logs))
        .route("/api/diagnostics", get(api::get_diagnostics))
        .route("/api/system/time", post(api::set_system_time))
        .route("/api/system/shutdown", post(api::system_shutdown))
        .route("/api/system/update/online", post(update::start_online_update))
        .route("/api/system/update/status", get(update::get_update_status))
        .route("/api/system/rollback", post(update::rollback))
        .route(
            "/api/system/update",
            post(api::system_update).layer(DefaultBodyLimit::max(UPDATE_BODY_LIMIT)),
        )
        // Wi-Fi client (maintenance mode)
        .route("/api/wifi/scan", get(api::wifi_scan))
        .route("/api/wifi/connect", post(api::wifi_connect))
        .route("/api/wifi/status", get(api::wifi_status))
        .route("/api/wifi/hotspot", post(api::wifi_hotspot))
        // Gallery (Recovery mode)
        .route("/api/gallery", get(gallery::get_gallery))
        .route("/api/gallery/sessions", get(gallery::get_gallery_sessions))
        .route("/api/gallery/stats", get(gallery::get_gallery_stats))
        .route("/api/gallery/best", get(gallery::get_best_auroras))
        .route("/api/gallery/delete", post(gallery::delete_gallery_images))
        .route("/api/gallery/download-zip", post(gallery::download_gallery_zip))
        .route("/api/gallery/sessions/:name/download", get(gallery::download_gallery_session_zip))
        .route("/api/gallery/sessions/:name", delete(gallery::delete_gallery_session))
        .route("/api/gallery/thumbnail/:filename", get(gallery::get_gallery_thumbnail))
        .route("/api/gallery/:filename", get(gallery::get_gallery_image))
        .layer(axum::middleware::from_fn(security::api_guard));

    Router::new()
        // Captive portal detection routes
        .route("/hotspot-detect.html", get(captive::captive_portal))
        .route("/library/test/success.html", get(captive::captive_portal))
        .route("/generate_204", get(captive::captive_portal))
        .route("/gen_204", get(captive::captive_portal))
        .route("/connecttest.txt", get(captive::captive_portal))
        .route("/ncsi.txt", get(captive::captive_portal))
        .route("/redirect", get(captive::captive_portal))
        .route("/canonical.html", get(captive::captive_portal))
        .route("/success.txt", get(captive::captive_portal))
        .merge(api)
        // Embedded web interface (single binary deployment)
        .fallback(static_files::serve_static)
        .layer(DefaultBodyLimit::max(API_BODY_LIMIT))
        .layer(axum::middleware::from_fn_with_state(state.clone(), track_activity))
        .layer(axum::middleware::from_fn(security::security_headers))
        .with_state(state)
}

/// Record API use (captive-portal probes from the phone do not count).
async fn track_activity(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if req.uri().path().starts_with("/api/") {
        state.touch();
    }
    next.run(req).await
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
