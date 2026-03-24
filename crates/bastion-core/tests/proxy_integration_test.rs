use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use hyper::{Request, Response, StatusCode, body::Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use http_body_util::Full;
use bytes::Bytes;

use bastion_core::pool::PoolManager;
use bastion_core::router::RadixTrie;
use bastion_core::loadbalancer::{Backend, UpstreamGroup};
use bastion_core::proxy::ProxyServer;
use bastion_core::middleware::{MiddlewareChain, LogMiddleware, RateLimiterMiddleware, RateLimitConfig};

/// Spawn a mock HTTP server returning a fixed body and status code.
async fn spawn_mock_server(port: u16, response_body: &'static str, status: StatusCode) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                let io = TokioIo::new(stream);
                tokio::spawn(async move {
                    let _ = http1::Builder::new()
                        .serve_connection(io, service_fn(move |_req: Request<Incoming>| {
                            let body = Full::new(Bytes::from(response_body));
                            let mut res = Response::new(body);
                            *res.status_mut() = status;
                            std::future::ready(Ok::<_, hyper::Error>(res))
                        }))
                        .await;
                });
            }
        }
    });
}

/// Spawn an echo server that reflects X-Request-ID back in the body.
async fn spawn_echo_server(port: u16) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                let io = TokioIo::new(stream);
                tokio::spawn(async move {
                    let _ = http1::Builder::new()
                        .serve_connection(io, service_fn(move |req: Request<Incoming>| {
                            let req_id = req
                                .headers()
                                .get("X-Request-ID")
                                .map(|v| v.to_str().unwrap().to_string())
                                .unwrap_or_default();
                            let body = Full::new(Bytes::from(req_id));
                            let res = Response::new(body);
                            std::future::ready(Ok::<_, hyper::Error>(res))
                        }))
                        .await;
                });
            }
        }
    });
}

fn build_proxy(router: RadixTrie<UpstreamGroup>) -> ProxyServer {
    let mut chain = MiddlewareChain::new();
    chain.add(LogMiddleware::new());
    ProxyServer::new(PoolManager::default(), router, chain)
}

async fn start_proxy(proxy: ProxyServer, port: u16) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    tokio::spawn(async move {
        let _ = proxy.start(addr).await;
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// ─────────────────────────────────────────────
// Test 1: 404 Not Found for unregistered path
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_404_not_found() {
    spawn_mock_server(10001, "ok", StatusCode::OK).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("g", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10001", 1));
    router.insert("/known", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10101).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:10101/unknown").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 404);
}

// ─────────────────────────────────────────────
// Test 2: Round Robin between two backends
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_round_robin() {
    spawn_mock_server(10002, "ServerA", StatusCode::OK).await;
    spawn_mock_server(10003, "ServerB", StatusCode::OK).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("rr", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10002", 1));
    g.add_backend(Backend::new("http://127.0.0.1:10003", 1));
    router.insert("/rr/*any", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10102).await;

    let client = reqwest::Client::new();
    let r1: reqwest::Response = client.get("http://127.0.0.1:10102/rr/test").send().await.unwrap();
    let b1 = r1.text().await.unwrap();

    let r2: reqwest::Response = client.get("http://127.0.0.1:10102/rr/test").send().await.unwrap();
    let b2 = r2.text().await.unwrap();

    assert!(
        (b1 == "ServerA" && b2 == "ServerB") || (b1 == "ServerB" && b2 == "ServerA"),
        "Expected round-robin, got: {} and {}", b1, b2
    );
}

// ─────────────────────────────────────────────
// Test 3: Auth route returns 401
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_auth_route_401() {
    spawn_mock_server(10004, "Unauthorized", StatusCode::UNAUTHORIZED).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("auth", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10004", 1));
    router.insert("/auth", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10103).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:10103/auth").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 401);
    assert_eq!(res.text().await.unwrap(), "Unauthorized");
}

// ─────────────────────────────────────────────
// Test 4: X-Request-ID is injected
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_x_request_id_injected() {
    spawn_echo_server(10005).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("echo", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10005", 1));
    router.insert("/echo", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10104).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:10104/echo").send().await.unwrap();
    let body = res.text().await.unwrap();
    assert!(!body.is_empty(), "X-Request-ID should be present and non-empty");
    assert_eq!(body.len(), 36, "UUID should be 36 chars: {}", body);
}

// ─────────────────────────────────────────────
// Test 5: Backend down returns 502
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_backend_down_502() {
    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("dead", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:19999", 1));
    router.insert("/dead", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10105).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:10105/dead").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 502);
}

// ─────────────────────────────────────────────
// Test 6: Multiple rapid requests through pool
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_pool_rapid_requests() {
    spawn_mock_server(10006, "pooled", StatusCode::OK).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("pool", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10006", 1));
    router.insert("/pool/*any", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10106).await;

    let client = reqwest::Client::new();
    for i in 0..10 {
        let res: reqwest::Response = client.get(format!("http://127.0.0.1:10106/pool/{}", i)).send().await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
    }
}

// ─────────────────────────────────────────────
// Test 7: GET request works
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_get_request() {
    spawn_mock_server(10007, "get_ok", StatusCode::OK).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("get", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10007", 1));
    router.insert("/get", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10107).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:10107/get").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.text().await.unwrap(), "get_ok");
}

// ─────────────────────────────────────────────
// Test 8: POST request with body
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_post_request_with_body() {
    spawn_mock_server(10008, "post_ok", StatusCode::OK).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("post", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10008", 1));
    router.insert("/post", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10108).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client
        .post("http://127.0.0.1:10108/post")
        .body("some payload")
        .send()
        .await
        .unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.text().await.unwrap(), "post_ok");
}

// ─────────────────────────────────────────────
// Test 9: No backends returns 503
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_no_backends_returns_503() {
    let mut router = RadixTrie::new();
    let g = UpstreamGroup::new("empty", vec![]);
    router.insert("/empty", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10109).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:10109/empty").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 503);
}

// ─────────────────────────────────────────────
// Test 10: Wildcard route captures subpaths
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_wildcard_route() {
    spawn_mock_server(10010, "wildcard_hit", StatusCode::OK).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("wild", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10010", 1));
    router.insert("/files/*path", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10110).await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:10110/files/a/b/c.txt").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert_eq!(res.text().await.unwrap(), "wildcard_hit");
}

// ─────────────────────────────────────────────
// Test 11: Concurrent requests handled safely
// ─────────────────────────────────────────────
#[tokio::test]
async fn test_proxy_concurrent_requests() {
    spawn_mock_server(10011, "concurrent", StatusCode::OK).await;

    let mut router = RadixTrie::new();
    let mut g = UpstreamGroup::new("conc", vec![]);
    g.add_backend(Backend::new("http://127.0.0.1:10011", 1));
    router.insert("/conc/*any", vec![], g, None, None);

    let proxy = build_proxy(router);
    start_proxy(proxy, 10111).await;

    let client = reqwest::Client::new();
    let mut handles = vec![];
    for i in 0..5 {
        let c = client.clone();
        handles.push(tokio::spawn(async move {
            let res: reqwest::Response = c
                .get(format!("http://127.0.0.1:10111/conc/{}", i))
                .send()
                .await
                .unwrap();
            assert_eq!(res.status().as_u16(), 200);
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}
