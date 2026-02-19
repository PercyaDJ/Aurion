use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "aurora-cam", about = "Autonomous aurora capture camera")]
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
            tracing::info!("Starting AuroraCam web server on port {}", port);
            let config = aurora_cam::core::config::AppConfig::load(
                &aurora_cam::core::config::AppConfig::default_path(),
            )
            .unwrap_or_default();
            let state = aurora_cam::web::AppState::new(config);
            aurora_cam::web::start_server(state, port).await?;
        }
        Commands::Simulate => {
            aurora_cam::cli::simulate::run_simulation().await?;
        }
    }

    Ok(())
}
