use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "aurion", about = "Aurion — Autonomous aurora capture camera")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Production mode: web server + autonomous night capture loop
    Run,
    /// Start the web server only (UI testing, no capture loop)
    Serve {
        /// Port to listen on
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
    },
    /// Simulate a full night cycle using PC mocks
    Simulate,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        // ─── RUN: production mode ───────────────────────────
        Commands::Run => {
            tracing::info!("Aurion: starting in production mode (Run)");

            let config = aurion::core::config::AppConfig::load(
                &aurion::core::config::AppConfig::default_path(),
            )
            .unwrap_or_default();

            let port = config.web.port;
            let state = aurion::web::AppState::new(config);

            // Start Wi-Fi AP
            #[cfg(feature = "rpi")]
            {
                use aurion::ports::network::NetworkApPort;
                let config_r = state.config.read().await;
                let network = aurion::adapters::rpi::NetworkRpi::new();
                let ssid = config_r.network.ssid.clone();
                let password = config_r.network.password.clone();
                let channel = config_r.network.channel;
                drop(config_r);
                tracing::info!("Starting Wi-Fi AP: {} (channel {})", ssid, channel);
                if let Err(e) = network.start_ap(&ssid, &password, channel).await {
                    tracing::warn!("Failed to start Wi-Fi AP: {} — continuing", e);
                }
            }

            // Create hardware adapters
            #[cfg(feature = "rpi")]
            let camera = aurion::adapters::rpi::CameraRpi::new();
            #[cfg(not(feature = "rpi"))]
            let camera = aurion::adapters::pc::CameraMock::synthetic();

            #[cfg(feature = "rpi")]
            let storage = {
                let cfg = state.config.read().await;
                aurion::adapters::rpi::StorageRpi::new(cfg.storage.mount_point.clone())
            };
            #[cfg(not(feature = "rpi"))]
            let storage = aurion::adapters::pc::StorageMock::new(std::path::PathBuf::from("./output"));

            #[cfg(feature = "rpi")]
            let system = aurion::adapters::rpi::SystemRpi::new();
            #[cfg(not(feature = "rpi"))]
            let system = aurion::adapters::pc::SystemMock::new();

            // Mount storage
            use aurion::ports::storage::StoragePort;
            if let Err(e) = storage.mount().await {
                tracing::warn!("Storage mount failed: {} — continuing", e);
            }

            let orchestrator = aurion::core::orchestrator::Orchestrator::new(
                state.clone(), camera, storage, system,
            );

            // Run web server + orchestrator + signal handler in parallel
            let web_state = state.clone();
            let shutdown_state = state.clone();

            tokio::select! {
                result = aurion::web::start_server(web_state, port) => {
                    if let Err(e) = result {
                        tracing::error!("Web server error: {}", e);
                    }
                }
                result = orchestrator.run() => {
                    if let Err(e) = result {
                        tracing::error!("Orchestrator error: {}", e);
                    }
                }
                _ = async {
                    // Signal handler: SIGTERM / SIGINT / Ctrl+C
                    let _ = tokio::signal::ctrl_c().await;
                    tracing::info!("Shutdown signal received (Ctrl+C / SIGTERM)");
                    let mut phase = shutdown_state.phase.write().await;
                    *phase = aurion::core::models::Phase::Shutdown;
                } => {
                    tracing::info!("Graceful shutdown initiated by signal");
                }
            }
        }

        // ─── SERVE: web server only (debug/UI testing) ──────
        Commands::Serve { port } => {
            tracing::info!("Starting Aurion web server on port {}", port);
            let config = aurion::core::config::AppConfig::load(
                &aurion::core::config::AppConfig::default_path(),
            )
            .unwrap_or_default();

            // Start Wi-Fi AP automatically on RPi
            #[cfg(feature = "rpi")]
            {
                use aurion::ports::network::NetworkApPort;
                let network = aurion::adapters::rpi::NetworkRpi::new();
                let ssid = config.network.ssid.clone();
                let password = config.network.password.clone();
                let channel = config.network.channel;
                tracing::info!("Starting Wi-Fi AP: {} (channel {})", ssid, channel);
                if let Err(e) = network.start_ap(&ssid, &password, channel).await {
                    tracing::warn!("Failed to start Wi-Fi AP: {} — continuing without hotspot", e);
                }
            }

            let state = aurion::web::AppState::new(config);
            aurion::web::start_server(state, port).await?;
        }

        // ─── SIMULATE: PC mock simulation ───────────────────
        Commands::Simulate => {
            aurion::cli::simulate::run_simulation().await?;
        }
    }

    Ok(())
}
