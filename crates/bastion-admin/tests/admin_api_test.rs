use bastion_admin::{create_app, AppState};
use bastion_metrics::GatewayMetrics;
use bastion_core::router::RadixTrie;
use bastion_core::loadbalancer::UpstreamGroup;
use std::sync::{Arc, RwLock};
use axum::{body::Body, extract::Request};
use hyper::StatusCode;
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn test_admin_api_authentication() {
    let metrics = Arc::new(GatewayMetrics::default());
    let router = Arc::new(RwLock::new(RadixTrie::<UpstreamGroup>::new()));
    let state = AppState { metrics, router };
    let app = create_app(state);

    // Test without auth header
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Test with invalid auth header
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/admin/health")
                .header("Authorization", "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    // Test with valid auth header
    let response = app
        .oneshot(
            Request::builder()
                .uri("/admin/health")
                .header("Authorization", "Bearer bastion-admin-secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_admin_api_endpoints_with_valid_auth() {
    let metrics = Arc::new(GatewayMetrics::default());
    let router = Arc::new(RwLock::new(RadixTrie::<UpstreamGroup>::new()));
    let state = AppState { metrics, router };
    let app = create_app(state);

    let endpoints = vec![
        "/admin/routes",
        "/admin/upstreams",
        "/admin/metrics",
        "/admin/metrics/prometheus",
        "/admin/info",
    ];

    for ep in endpoints {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(ep)
                    .header("Authorization", "Bearer bastion-admin-secret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "Endpoint {} failed", ep);
    }
}
