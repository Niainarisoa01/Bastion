use axum::{routing::get, Router};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Spawn 3 rapid servers on ports 8001, 8002, 8003
    let ports = vec![8001, 8002, 8003];

    for port in ports {
        tokio::spawn(async move {
            let app = Router::new()
                .route("/", get(|| async { "Hello from Fast Backend\n" }))
                .route("/api", get(|| async { "{\"status\": \"ok\", \"service\": \"fast-backend\"}" }));

            let addr = SocketAddr::from(([127, 0, 0, 1], port));
            println!("🚀 Fast Backend listening on {}", addr);
            let listener = TcpListener::bind(addr).await.unwrap();
            axum::serve(listener, app).await.unwrap();
        });
    }

    // Keep main thread alive
    let _ = tokio::signal::ctrl_c().await;
    println!("\nShutting down Fast Backends");
    Ok(())
}
