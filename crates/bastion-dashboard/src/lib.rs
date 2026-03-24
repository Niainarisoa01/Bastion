use axum::{
    Router,
    extract::{State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
    routing::get,
};
use futures::{SinkExt, StreamExt};
use bastion_metrics::GatewayMetrics;
use std::sync::Arc;
use tower_http::services::ServeDir;

#[derive(Clone)]
pub struct DashboardState {
    pub metrics: Arc<GatewayMetrics>,
}

pub async fn start_dashboard_server(
    listen_addr: String,
    metrics: Arc<GatewayMetrics>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = DashboardState { metrics };

    let static_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("static");

    let app = Router::new()
        .route("/ws/metrics", get(ws_handler))
        .with_state(state)
        .fallback_service(ServeDir::new(static_dir));

    let listener = tokio::net::TcpListener::bind(&listen_addr).await?;
    tracing::info!("Dashboard server started on http://{}", listen_addr);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<DashboardState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state))
}

async fn handle_ws(socket: WebSocket, state: DashboardState) {
    let (mut sender, mut receiver) = socket.split();

    // Spawn a task to read (and discard) client messages / detect disconnect
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(_msg)) = receiver.next().await {
            // We only receive pings/pongs or close frames
        }
    });

    // Streaming metrics every second
    let mut send_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        loop {
            interval.tick().await;

            let payload = build_metrics_payload(&state.metrics);
            let json = match serde_json::to_string(&payload) {
                Ok(j) => j,
                Err(_) => continue,
            };

            if sender.send(Message::Text(json.into())).await.is_err() {
                break; // Client disconnected
            }
        }
    });

    // Wait for either task to finish (disconnect)
    tokio::select! {
        _ = &mut send_task => { recv_task.abort(); }
        _ = &mut recv_task => { send_task.abort(); }
    }
}

fn build_metrics_payload(metrics: &GatewayMetrics) -> serde_json::Value {
    use std::sync::atomic::Ordering::Relaxed;

    let (p50, p95, p99) = {
        let hist = metrics.global_latency.lock();
        (
            hist.value_at_quantile(0.50),
            hist.value_at_quantile(0.95),
            hist.value_at_quantile(0.99),
        )
    };

    let mut backends = Vec::new();
    for entry in metrics.backends.iter() {
        let bm = entry.value();
        let bh = bm.latency.lock();
        backends.push(serde_json::json!({
            "url": entry.key(),
            "requests": bm.total_requests.load(Relaxed),
            "errors": bm.total_errors.load(Relaxed),
            "active": bm.active_connections.load(Relaxed),
            "p95_us": bh.value_at_quantile(0.95),
        }));
    }

    let mut routes = Vec::new();
    for entry in metrics.routes.iter() {
        let rm = entry.value();
        let rh = rm.latency.lock();
        routes.push(serde_json::json!({
            "path": entry.key(),
            "requests": rm.total_requests.load(Relaxed),
            "errors": rm.total_errors.load(Relaxed),
            "p95_us": rh.value_at_quantile(0.95),
        }));
    }

    serde_json::json!({
        "type": "metrics",
        "timestamp": chrono_timestamp(),
        "global": {
            "total_requests": metrics.total_requests.load(Relaxed),
            "active_requests": metrics.active_requests.load(Relaxed),
            "total_errors": metrics.total_errors.load(Relaxed),
            "p50_us": p50,
            "p95_us": p95,
            "p99_us": p99,
        },
        "backends": backends,
        "routes": routes,
    })
}

fn chrono_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", now.as_secs())
}
