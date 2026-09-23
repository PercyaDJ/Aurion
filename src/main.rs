use std::path::PathBuf;

use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

use aurion::core::config::{AppConfig, DEFAULT_WIFI_PASSWORD};
use aurion::web::{AppState, Paths};

#[derive(Parser)]
#[command(name = "aurion", version, about = "Aurion — Autonomous aurora capture camera")]
struct Cli {
    /// Configuration directory (default: $AURION_CONFIG_DIR or ./config)
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,

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
    /// Check the configuration file and print it (password masked)
    CheckConfig,
    /// Measure the CPU cost of the image treatments on this machine
    Bench {
        #[arg(long, default_value_t = 4056)]
        width: u32,
        #[arg(long, default_value_t = 3040)]
        height: u32,
    },
}

fn config_dir(cli: &Cli) -> PathBuf {
    cli.config_dir
        .clone()
        .or_else(|| std::env::var_os("AURION_CONFIG_DIR").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("config"))
}

/// Load the config; an invalid file is reported loudly (it used to be
/// silently replaced by the defaults, including the default password).
fn load_config(paths: &Paths) -> AppConfig {
    match AppConfig::load(&paths.config_file) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!(
                "Configuration {} invalide ({}) — valeurs par défaut utilisées",
                paths.config_file.display(),
                e
            );
            AppConfig::default()
        }
    }
}

/// Minimal sd_notify(3): tell systemd we are ready / alive.
fn sd_notify(msg: &str) {
    let Ok(socket_path) = std::env::var("NOTIFY_SOCKET") else { return };
    let path = if let Some(rest) = socket_path.strip_prefix('@') {
        format!("\0{}", rest)
    } else {
        socket_path
    };
    if let Ok(socket) = std::os::unix::net::UnixDatagram::unbound() {
        let _ = socket.send_to(msg.as_bytes(), &path);
    }
}

/// Resolves on SIGINT (Ctrl+C) or SIGTERM (`systemctl stop`).
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

// Two worker threads are plenty (camera and web requests mostly wait):
// fewer threads, fewer CPU wake-ups on battery.
#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let cli = Cli::parse();
    let paths = Paths::from_config_dir(&config_dir(&cli));

    match cli.command {
        // ─── RUN: production mode ───────────────────────────
        Commands::Run => {
            tracing::info!("Aurion {}: production mode (config: {})", env!("CARGO_PKG_VERSION"), paths.config_file.display());

            let mut config = load_config(&paths);
            // "Mot de passe oublié" : fichier aurion-reset-wifi.txt sur la clé USB
            if aurion::core::config::apply_usb_wifi_reset(&mut config) {
                tracing::warn!("Mot de passe Wi-Fi réinitialisé par le fichier de la clé USB");
                if let Err(e) = config.save(&paths.config_file) {
                    tracing::error!("Sauvegarde de la config impossible: {}", e);
                }
            }
            if config.network.password == DEFAULT_WIFI_PASSWORD {
                tracing::warn!("[Securite] Mot de passe Wi-Fi par defaut. Changez-le dans Reglages avances.");
            }
            let port = config.web.port;
            let state = AppState::with_paths(config, paths);

            // Start Wi-Fi AP
            #[cfg(feature = "rpi")]
            {
                use aurion::ports::network::NetworkApPort;
                let net = state.config.read().await.network.clone();
                tracing::info!("Starting Wi-Fi AP: {} (channel {})", net.ssid, net.channel);
                if let Err(e) = aurion::adapters::rpi::NetworkRpi::new().start_ap(&net.ssid, &net.password, net.channel).await {
                    tracing::warn!("Failed to start Wi-Fi AP: {} — continuing", e);
                    state.add_log(format!("Hotspot Wi-Fi non démarré: {}", e)).await;
                }
            }

            // Hardware adapters
            #[cfg(feature = "rpi")]
            let camera = {
                let cfg = state.config.read().await;
                aurion::adapters::rpi::CameraRpi::new()
                    .with_isp_denoise(&cfg.capture.denoise.isp_denoise)
                    .with_awb(&cfg.capture.awb)
            };
            #[cfg(not(feature = "rpi"))]
            let camera = aurion::adapters::pc::CameraMock::synthetic();

            #[cfg(feature = "rpi")]
            let storage = aurion::adapters::rpi::StorageRpi::new(state.config.read().await.storage.mount_point.clone());
            #[cfg(not(feature = "rpi"))]
            let storage = aurion::adapters::pc::StorageMock::new(PathBuf::from(state.config.read().await.storage.mount_point.clone()));

            #[cfg(feature = "rpi")]
            let system = aurion::adapters::rpi::SystemRpi::new();
            #[cfg(not(feature = "rpi"))]
            let system = aurion::adapters::pc::SystemMock::new();

            use aurion::ports::storage::StoragePort;
            if !storage.is_available() {
                if let Err(e) = storage.mount().await {
                    tracing::warn!("Storage mount failed: {} — continuing", e);
                }
            }

            #[allow(unused_mut)]
            let mut orchestrator = aurion::core::orchestrator::Orchestrator::new(state.clone(), camera, storage, system);
            #[cfg(feature = "rpi")]
            {
                orchestrator = orchestrator.with_network(Box::new(aurion::adapters::rpi::NetworkRpi::new()));
            }

            sd_notify("READY=1");

            tokio::select! {
                result = aurion::web::start_server(state.clone(), port) => {
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
                    // systemd watchdog (WatchdogSec in aurion.service)
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
                    loop {
                        interval.tick().await;
                        sd_notify("WATCHDOG=1");
                    }
                } => {}
                _ = shutdown_signal() => {
                    tracing::info!("Shutdown signal received: flushing storage");
                    *state.phase.write().await = aurion::core::models::Phase::Shutdown;
                    sd_notify("STOPPING=1");
                    let _ = aurion::sys::run("sync", &[], std::time::Duration::from_secs(30)).await;
                }
            }
        }

        // ─── SERVE: web server only (debug/UI testing) ──────
        Commands::Serve { port } => {
            tracing::info!("Starting Aurion web server on port {}", port);
            let config = load_config(&paths);

            #[cfg(feature = "rpi")]
            {
                use aurion::ports::network::NetworkApPort;
                let net = config.network.clone();
                tracing::info!("Starting Wi-Fi AP: {} (channel {})", net.ssid, net.channel);
                if let Err(e) = aurion::adapters::rpi::NetworkRpi::new().start_ap(&net.ssid, &net.password, net.channel).await {
                    tracing::warn!("Failed to start Wi-Fi AP: {} — continuing without hotspot", e);
                }
            }

            let state = AppState::with_paths(config, paths);
            tokio::select! {
                result = aurion::web::start_server(state, port) => result?,
                _ = shutdown_signal() => tracing::info!("Stopped"),
            }
        }

        // ─── SIMULATE: PC mock simulation ───────────────────
        Commands::Simulate => {
            aurion::cli::simulate::run_simulation().await?;
        }

        Commands::Bench { width, height } => {
            aurion::cli::bench::run_bench(width, height)?;
        }

        Commands::CheckConfig => {
            let config = AppConfig::load(&paths.config_file)
                .map_err(|e| anyhow::anyhow!("{}: {}", paths.config_file.display(), e))?;
            println!("{}", serde_json::to_string_pretty(&config.masked())?);
            println!("OK: {}", paths.config_file.display());
        }
    }

    Ok(())
}
