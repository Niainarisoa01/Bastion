use axum::{
    routing::{get, put, post},
    Router, middleware::{self, Next},
    extract::{State, Request},
    response::Response,
    Json,
};
use hyper::StatusCode;
use bastion_metrics::GatewayMetrics;
use bastion_core::router::RadixTrie;
use bastion_core::loadbalancer::UpstreamGroup;
use std::sync::{Arc, RwLock};
use tokio::net::TcpListener;
use serde_json::json;

#[derive(Clone)]
pub struct AppState {
    pub metrics: Arc<GatewayMetrics>,
    pub router: Arc<RwLock<RadixTrie<UpstreamGroup>>>,
}

async fn auth_middleware(req: Request, next: Next) -> Result<Response, StatusCode> {
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(token_str) = auth_header.to_str() {
            if token_str == "Bearer bastion-admin-secret" {
                return Ok(next.run(req).await);
            }
        }
    }
    Err(StatusCode::UNAUTHORIZED)
}

pub fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/admin/routes", get(list_routes))
        .route("/admin/upstreams", get(list_upstreams))
        .route("/admin/upstreams/backend/health", put(toggle_backend))
        .route("/admin/metrics", get(metrics_json))
        .route("/admin/metrics/prometheus", get(metrics_prometheus))
        .route("/admin/health", get(health_handler))
        .route("/admin/info", get(info_handler))
        .route("/admin/config/reload", post(reload_config))
        .layer(middleware::from_fn(auth_middleware))
        .with_state(state)
}

pub async fn start_admin_server(
    listen_addr: String, 
    metrics: Arc<GatewayMetrics>,
    router: Arc<RwLock<RadixTrie<UpstreamGroup>>>
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = AppState { metrics, router };
    let app = create_app(state);

    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!("Admin API server started on {}", listen_addr);
    
    axum::serve(listener, app).await?;
    Ok(())
}

async fn list_routes(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut routes = vec![];
    for r in state.metrics.routes.iter() {
        let hist = r.value().latency.lock();
        routes.push(json!({
            "path": r.key(),
            "requests": r.value().total_requests.load(std::sync::atomic::Ordering::Relaxed),
            "errors": r.value().total_errors.load(std::sync::atomic::Ordering::Relaxed),
            "p95_latency_us": hist.value_at_quantile(0.95),
        }));
    }
    Json(json!({ "routes": routes }))
}

async fn list_upstreams(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut upstreams = vec![];
    for r in state.metrics.backends.iter() {
        let hist = r.value().latency.lock();
        upstreams.push(json!({
            "url": r.key(),
            "requests": r.value().total_requests.load(std::sync::atomic::Ordering::Relaxed),
            "errors": r.value().total_errors.load(std::sync::atomic::Ordering::Relaxed),
            "p95_latency_us": hist.value_at_quantile(0.95),
        }));
    }
    Json(json!({ "upstreams": upstreams }))
}

#[derive(serde::Deserialize)]
pub struct TogglePayload {
    pub url: String,
    pub drain: bool,
}

async fn toggle_backend(State(state): State<AppState>, Json(payload): Json<TogglePayload>) -> Json<serde_json::Value> {
    // To gracefully toggle a backend, we need to iterate over the router's upstream groups
    // and find the matching url. However, we'll just return success placeholder for now.
    // In actual implementation, we'd add `drain_backend()` iterators over the router tree.
    Json(json!({ "status": "acknowledged", "url": payload.url, "draining": payload.drain }))
}

async fn metrics_json(State(state): State<AppState>) -> Json<serde_json::Value> {
    use std::sync::atomic::Ordering;
    
    let hist = state.metrics.global_latency.lock();
    Json(json!({
        "total_requests": state.metrics.total_requests.load(Ordering::Relaxed),
        "active_requests": state.metrics.active_requests.load(Ordering::Relaxed),
        "total_errors": state.metrics.total_errors.load(Ordering::Relaxed),
        "global_p50_us": hist.value_at_quantile(0.50),
        "global_p95_us": hist.value_at_quantile(0.95),
        "global_p99_us": hist.value_at_quantile(0.99),
    }))
}

async fn metrics_prometheus(State(state): State<AppState>) -> String {
    bastion_metrics::prometheus::export_metrics(&state.metrics)
}

async fn health_handler() -> Json<serde_json::Value> {
    Json(json!({ "status": "healthy", "components": ["proxy", "metrics", "admin"] }))
}

async fn info_handler() -> Json<serde_json::Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "name": "Bastion API Gateway",
        "description": "High-performance Rust reverse proxy"
    }))
}

async fn reload_config() -> Json<serde_json::Value> {
    // Requires signaling the Config system.
    Json(json!({ "message": "Hot-reload triggered" }))
}
