use axum::{routing::get, Router, extract::State};
use bastion_metrics::GatewayMetrics;
use std::sync::Arc;
use tokio::net::TcpListener;

pub async fn start_admin_server(listen_addr: String, metrics: Arc<GatewayMetrics>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new()
        .route("/metrics", get(metrics_handler))
        .route("/health", get(health_handler))
        .with_state(metrics);

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!("Admin API server started on {}", listen_addr);
    
    axum::serve(listener, app).await?;
    Ok(())
}

async fn metrics_handler(State(metrics): State<Arc<GatewayMetrics>>) -> String {
    bastion_metrics::prometheus::export_metrics(&metrics)
}

async fn health_handler() -> &'static str {
    "OK"
}
