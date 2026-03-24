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

    // 4. Start Core Proxy Engine
    let pool = bastion_core::pool::PoolManager::default();
    let mut router = bastion_core::router::RadixTrie::new();
    
    // Upstream group with two backends
    let mut backend_group = bastion_core::loadbalancer::UpstreamGroup::new("test_group", vec![]);
    backend_group.add_backend(bastion_core::loadbalancer::Backend::new("http://127.0.0.1:8001", 1));
    backend_group.add_backend(bastion_core::loadbalancer::Backend::new("http://127.0.0.1:8002", 1));
    
    let methods = vec![];
    router.insert("/api/*any", methods.clone(), backend_group.clone(), None, None);
    router.insert("/api", methods.clone(), backend_group.clone(), None, None);
    router.insert("/public/*any", methods.clone(), backend_group.clone(), None, None);
    router.insert("/public", methods, backend_group.clone(), None, None);

    // Build middleware chain
    let mut chain = bastion_core::middleware::MiddlewareChain::new();
    
    // 1. IP Filter (Whitelist localhost)
    chain.add(bastion_core::middleware::IpFilterMiddleware::new(
        bastion_core::middleware::IpFilterConfig {
            mode: bastion_core::middleware::IpFilterMode::Whitelist,
            rules: vec!["127.0.0.0/8".to_string(), "::1".to_string()],
        }
    ));

    // 2. CORS
    chain.add(bastion_core::middleware::CorsMiddleware::new(
        bastion_core::middleware::CorsConfig::default()
    ));

    // 3. JWT Auth
    chain.add(bastion_core::middleware::JwtMiddleware::new(
        bastion_core::middleware::JwtConfig {
            secret: bastion_core::middleware::JwtSecret::Hmac("bastion-test-secret".to_string()),
            skip_paths: vec!["/public".to_string()],
            ..Default::default()
        }
    ));

    // 4. Request Validation (Max 1MB body)
    chain.add(bastion_core::middleware::RequestValidationMiddleware::new(
        bastion_core::middleware::RequestValidationConfig {
            max_body_size: Some(1024 * 1024),
            required_content_types: vec![],
        }
    ));

    // 5. Rate Limiter
    chain.add(bastion_core::middleware::RateLimiterMiddleware::new(
        bastion_core::middleware::RateLimitConfig {
            limit: 5,
            window: std::time::Duration::from_secs(10),
            ..Default::default()
        }
    ));

    // 6. Logging
    chain.add(bastion_core::middleware::LogMiddleware::new());

    let proxy = bastion_core::proxy::ProxyServer::new(pool, router, chain);
    
    let listen_addr = config.server.listen.parse().with_context(|| "Invalid listen address in config")?;
    tokio::spawn(async move {
        tracing::info!("Starting ProxyServer on {}...", listen_addr);
        if let Err(e) = proxy.start(listen_addr).await {
            tracing::error!("Proxy server crashed: {}", e);
        }
    });

    // TODO: Init Admin Server, Telegram Bot, Metrics
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("Shutting down Bastion Gateway gracefully...");

    Ok(())
}
