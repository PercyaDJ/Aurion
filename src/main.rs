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
    /// Start the web server (ARM phase for UI testing)
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
        Commands::Simulate => {
            aurion::cli::simulate::run_simulation().await?;
        }
    }

    Ok(())
}
