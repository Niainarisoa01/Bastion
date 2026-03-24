use anyhow::{Context, Result};
use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use bastion_config::{load_config, GatewayConfig};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the configuration file
    #[arg(short, long, default_value = "config/bastion.toml")]
    config: String,
}

fn setup_tracing(config: &GatewayConfig) -> Result<()> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.logging.level));

    let format = config.logging.format.to_lowercase();
    
    match format.as_str() {
        "json" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().json())
                .try_init()?;
        }
        "pretty" => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().pretty())
                .try_init()?;
        }
        _ => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .try_init()?;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // 1. Parse CLI arguments
    let args = Args::parse();

    // 2. Load and validate configuration
    let config = load_config(&args.config)
        .with_context(|| format!("Failed to load configuration from {}", args.config))?;

    // 3. Setup Tracing
    setup_tracing(&config).context("Failed to initialize tracing")?;

    tracing::info!("Starting Bastion Gateway...");
    tracing::info!("Loaded configuration from {}", args.config);
    tracing::info!("Server listening on {}", config.server.listen);
    tracing::info!("Admin API listening on {}", config.server.admin_listen);

    // TODO: Init core gateway loops (HTTP Server, Admin Server, Telegram Bot, Metrics)
    
    // Park the main thread for now to keep the binary alive
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("Shutting down Bastion Gateway gracefully...");

    Ok(())
}
