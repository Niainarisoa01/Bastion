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
    
    // Upstream group with 3 weighted backends
    let health_cfg = bastion_core::health::HealthConfig {
        path: "/api".to_string(),
        ..Default::default()
    };
    let mut backend_group = bastion_core::loadbalancer::UpstreamGroup::new("test_group", vec![]);
    let b1 = bastion_core::loadbalancer::Backend {
        url: "http://127.0.0.1:8001".to_string(),
        weight: 3,
        active_connections: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        health: std::sync::Arc::new(bastion_core::health::BackendHealth::new(health_cfg.clone())),
    };
    let b2 = bastion_core::loadbalancer::Backend {
        url: "http://127.0.0.1:8002".to_string(),
        weight: 1,
        active_connections: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        health: std::sync::Arc::new(bastion_core::health::BackendHealth::new(health_cfg.clone())),
    };
    let b3 = bastion_core::loadbalancer::Backend {
        url: "http://127.0.0.1:8003".to_string(),
        weight: 1,
        active_connections: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        health: std::sync::Arc::new(bastion_core::health::BackendHealth::new(health_cfg)),
    };
    backend_group.add_backend(b1);
    backend_group.add_backend(b2);
    backend_group.add_backend(b3);
    
    backend_group.start_health_check();
    
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
            skip_paths: vec!["/public".to_string(), "/api".to_string()],
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

    // 5. Rate Limiter (Increased for LB tests)
    chain.add(bastion_core::middleware::RateLimiterMiddleware::new(
        bastion_core::middleware::RateLimitConfig {
            limit: 50000,
            window: std::time::Duration::from_secs(10),
            ..Default::default()
        }
    ));

    // 6. Logging
    chain.add(bastion_core::middleware::LogMiddleware::new());

    // 7. Caching
    let cache = std::sync::Arc::new(bastion_cache::ShardedLruCache::new(8, 1000, 0));
    chain.add(bastion_core::middleware::cache::CacheMiddleware::new(cache));

    let metrics = std::sync::Arc::new(bastion_metrics::GatewayMetrics::default());
    
    // Inject metrics middleware into proxy chain
    chain.add(bastion_core::middleware::metrics::MetricsMiddleware::new(metrics.clone()));

    let shared_router = std::sync::Arc::new(std::sync::RwLock::new(router));
    
    // Hot-reload setup
    let config_watcher = bastion_config::ConfigWatcher::new(&args.config, config.clone());
    config_watcher.start_watching(|new_cfg| {
        tracing::info!("Bastion Main: Received hot-reload event (config revision loaded)");
        // TODO: Map `new_cfg.routes` into `shared_router.write()` down the line.
    });

    let proxy = bastion_core::proxy::ProxyServer::new(pool, shared_router.clone(), chain);

    let listen_addr = config.server.listen.parse().with_context(|| "Invalid listen address in config")?;
    tokio::spawn(async move {
        tracing::info!("Starting ProxyServer on {}...", listen_addr);
        if let Err(e) = proxy.start(listen_addr).await {
            tracing::error!("Proxy server crashed: {}", e);
        }
    });

    let admin_listen = config.server.admin_listen.clone();
    let metrics_for_admin = metrics.clone();
    let router_for_admin = shared_router.clone();
    tokio::spawn(async move {
        if let Err(e) = bastion_admin::start_admin_server(admin_listen, metrics_for_admin, router_for_admin).await {
            tracing::error!("Admin server crashed: {}", e);
        }
    });

    if config.telegram.enabled && !config.telegram.token.is_empty() {
        let tg_ctx = bastion_telegram::BotContext {
            metrics: metrics.clone(),
            router: shared_router.clone(),
            admin_chat_ids: config.telegram.admin_chat_ids.clone(),
        };
        let token = config.telegram.token.clone();
        tokio::spawn(async move {
            bastion_telegram::start_telegram_bot(token, tg_ctx).await;
        });
    }

    // Await termination (Ctrl+C)
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("Shutting down Bastion Gateway gracefully...");

    Ok(())
}
