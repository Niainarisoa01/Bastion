use bastion_core::middleware::jwt::{JwtMiddleware, JwtConfig, JwtSecret, Claims};
use bastion_core::middleware::cors::{CorsMiddleware, CorsConfig};
use bastion_core::middleware::ip_filter::{IpFilterMiddleware, IpFilterConfig, IpFilterMode};
use bastion_core::middleware::request_validation::{RequestValidationMiddleware, RequestValidationConfig};
use bastion_core::middleware::chain::{Middleware, Next, ProxyHandler, ProxyResponse};
use bastion_core::middleware::context::RequestContext;

use async_trait::async_trait;
use hyper::{Request, Response, StatusCode, Method};
use hyper::body::{Incoming, Bytes};
use hyper::header::HeaderValue;
use http_body_util::{BodyExt, Full, Empty};
use http_body_util::combinators::BoxBody;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use jsonwebtoken::{encode, EncodingKey, Header, Algorithm};

// ── Mock terminal handler ──────────────────────

struct MockHandler;

#[async_trait]
impl ProxyHandler for MockHandler {
    async fn call_proxy(
        &self,
        req: Request<Incoming>,
        _ctx: &RequestContext,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        let user_id = req.headers().get("x-user-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("none")
            .to_string();
        let role = req.headers().get("x-user-role")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("none")
            .to_string();
        let body_str = format!("user_id={},role={}", user_id, role);
        let body = Full::new(Bytes::from(body_str))
            .map_err(|never| match never {})
            .boxed();
        Ok(Response::new(body))
    }
}

fn make_ctx(ip: &str) -> RequestContext {
    RequestContext::new(
        "test-req-id".to_string(),
        ip.parse::<IpAddr>().unwrap(),
    )
}

fn make_token(sub: &str, role: &str, secret: &str) -> String {
    let claims = Claims {
        sub: sub.to_string(),
        role: role.to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).unwrap()
}

fn make_expired_token(secret: &str) -> String {
    let claims = Claims {
        sub: "user1".to_string(),
        role: "admin".to_string(),
        exp: 1000,
        iat: 900,
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).unwrap()
}

/// Spawn a simple echo server and make a real HTTP request through the middleware.
/// This approach avoids the Incoming::default() issue by using real HTTP connections.
async fn spawn_middleware_test_server<M: Middleware + 'static>(
    port: u16,
    middleware: M,
    ctx_ip: &str,
) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    let mw = Arc::new(middleware);
    let ip: IpAddr = ctx_ip.parse().unwrap();

    tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                let io = TokioIo::new(stream);
                let mw = Arc::clone(&mw);
                let ip = ip.clone();
                tokio::spawn(async move {
                    let _ = http1::Builder::new()
                        .serve_connection(io, service_fn(move |req: Request<Incoming>| {
                            let mw = Arc::clone(&mw);
                            let ip = ip.clone();
                            async move {
                                let ctx = RequestContext::new("test-id".to_string(), ip);
                                let handler = MockHandler;
                                let next = Next { middlewares: &[], final_handler: &handler };
                                let res = mw.handle(req, &ctx, next).await
                                    .unwrap_or_else(|_| {
                                        let body = Full::new(Bytes::from("500"))
                                            .map_err(|never| match never {})
                                            .boxed();
                                        let mut r = Response::new(body);
                                        *r.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                                        r
                                    });
                                Ok::<_, hyper::Error>(res)
                            }
                        }))
                        .await;
                });
            }
        }
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
}

// ════════════════════════════════════════════
//  JWT TESTS
// ════════════════════════════════════════════

#[tokio::test]
async fn test_jwt_valid_token() {
    let secret = "my-secret-key";
    let jwt = JwtMiddleware::new(JwtConfig {
        secret: JwtSecret::Hmac(secret.to_string()),
        ..Default::default()
    });
    spawn_middleware_test_server(11001, jwt, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let token = make_token("user42", "admin", secret);
    let res: reqwest::Response = client.get("http://127.0.0.1:11001/api/data")
        .header("Authorization", format!("Bearer {}", token))
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    let body = res.text().await.unwrap();
    assert!(body.contains("user_id=user42"), "Body: {}", body);
    assert!(body.contains("role=admin"), "Body: {}", body);
}

#[tokio::test]
async fn test_jwt_missing_token() {
    let jwt = JwtMiddleware::new(JwtConfig::default());
    spawn_middleware_test_server(11002, jwt, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11002/api/data")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn test_jwt_invalid_signature() {
    let jwt = JwtMiddleware::new(JwtConfig {
        secret: JwtSecret::Hmac("correct-secret".to_string()),
        ..Default::default()
    });
    spawn_middleware_test_server(11003, jwt, "127.0.0.1").await;
    let bad_token = make_token("user", "admin", "wrong-secret");

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11003/api/data")
        .header("Authorization", format!("Bearer {}", bad_token))
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn test_jwt_expired_token() {
    let secret = "my-secret";
    let jwt = JwtMiddleware::new(JwtConfig {
        secret: JwtSecret::Hmac(secret.to_string()),
        ..Default::default()
    });
    spawn_middleware_test_server(11004, jwt, "127.0.0.1").await;
    let token = make_expired_token(secret);

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11004/api/data")
        .header("Authorization", format!("Bearer {}", token))
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 401);
}

#[tokio::test]
async fn test_jwt_skip_path_login() {
    let jwt = JwtMiddleware::new(JwtConfig::default());
    spawn_middleware_test_server(11005, jwt, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11005/login")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn test_jwt_skip_path_register() {
    let jwt = JwtMiddleware::new(JwtConfig::default());
    spawn_middleware_test_server(11006, jwt, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11006/register")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn test_jwt_claims_injected_as_headers() {
    let secret = "test-key";
    let jwt = JwtMiddleware::new(JwtConfig {
        secret: JwtSecret::Hmac(secret.to_string()),
        ..Default::default()
    });
    spawn_middleware_test_server(11007, jwt, "127.0.0.1").await;
    let token = make_token("user99", "moderator", secret);

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11007/api/resource")
        .header("Authorization", format!("Bearer {}", token))
        .send().await.unwrap();
    let body = res.text().await.unwrap();
    assert!(body.contains("user_id=user99"));
    assert!(body.contains("role=moderator"));
}

// ════════════════════════════════════════════
//  CORS TESTS
// ════════════════════════════════════════════

#[tokio::test]
async fn test_cors_preflight_options() {
    let cors = CorsMiddleware::new(CorsConfig::default());
    spawn_middleware_test_server(11008, cors, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.request(Method::OPTIONS, "http://127.0.0.1:11008/api")
        .header("Origin", "http://example.com")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 204);
    assert!(res.headers().get("Access-Control-Allow-Origin").is_some());
    assert!(res.headers().get("Access-Control-Allow-Methods").is_some());
}

#[tokio::test]
async fn test_cors_headers_on_normal_request() {
    let cors = CorsMiddleware::new(CorsConfig::default());
    spawn_middleware_test_server(11009, cors, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11009/api")
        .header("Origin", "http://example.com")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    assert!(res.headers().get("Access-Control-Allow-Origin").is_some());
}

#[tokio::test]
async fn test_cors_disallowed_origin() {
    let cors = CorsMiddleware::new(CorsConfig {
        allowed_origins: vec!["http://trusted.com".to_string()],
        ..Default::default()
    });
    spawn_middleware_test_server(11010, cors, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11010/api")
        .header("Origin", "http://evil.com")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn test_cors_no_origin_passthrough() {
    let cors = CorsMiddleware::new(CorsConfig::default());
    spawn_middleware_test_server(11011, cors, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11011/api")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

// ════════════════════════════════════════════
//  IP FILTER TESTS
// ════════════════════════════════════════════

#[tokio::test]
async fn test_ip_whitelist_allowed() {
    let filter = IpFilterMiddleware::new(IpFilterConfig {
        mode: IpFilterMode::Whitelist,
        rules: vec!["127.0.0.1".to_string()],
    });
    spawn_middleware_test_server(11012, filter, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11012/")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn test_ip_whitelist_blocked() {
    let filter = IpFilterMiddleware::new(IpFilterConfig {
        mode: IpFilterMode::Whitelist,
        rules: vec!["10.0.0.1".to_string()],
    });
    // Simulate as if client IP is not in whitelist — ctx IP = 192.168.1.1
    spawn_middleware_test_server(11013, filter, "192.168.1.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11013/")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn test_ip_blacklist_blocks() {
    let filter = IpFilterMiddleware::new(IpFilterConfig {
        mode: IpFilterMode::Blacklist,
        rules: vec!["127.0.0.1".to_string()],
    });
    spawn_middleware_test_server(11014, filter, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11014/")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 403);
}

#[tokio::test]
async fn test_ip_cidr_whitelist() {
    let filter = IpFilterMiddleware::new(IpFilterConfig {
        mode: IpFilterMode::Whitelist,
        rules: vec!["10.0.0.0/24".to_string()],
    });
    spawn_middleware_test_server(11015, filter, "10.0.0.42").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11015/")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}

#[tokio::test]
async fn test_ip_cidr_outside_range() {
    let filter = IpFilterMiddleware::new(IpFilterConfig {
        mode: IpFilterMode::Whitelist,
        rules: vec!["10.0.0.0/24".to_string()],
    });
    spawn_middleware_test_server(11016, filter, "10.0.1.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11016/")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 403);
}

// ════════════════════════════════════════════
//  REQUEST VALIDATION TESTS
// ════════════════════════════════════════════

#[tokio::test]
async fn test_request_validation_body_too_large() {
    let validator = RequestValidationMiddleware::new(RequestValidationConfig {
        max_body_size: Some(1024),
        required_content_types: vec![],
    });
    spawn_middleware_test_server(11017, validator, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.post("http://127.0.0.1:11017/upload")
        .header("Content-Length", "999999")
        .body("x")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 413);
}

#[tokio::test]
async fn test_request_validation_body_within_limit() {
    let validator = RequestValidationMiddleware::new(RequestValidationConfig {
        max_body_size: Some(10240),
        required_content_types: vec![],
    });
    spawn_middleware_test_server(11018, validator, "127.0.0.1").await;

    let client = reqwest::Client::new();
    let res: reqwest::Response = client.get("http://127.0.0.1:11018/data")
        .send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
}
