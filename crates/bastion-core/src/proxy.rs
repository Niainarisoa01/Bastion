use hyper::{Request, Response, StatusCode, Uri};
use hyper::body::{Incoming, Bytes};
use http_body_util::{BodyExt, Full};
use http_body_util::combinators::BoxBody;
use std::net::SocketAddr;
use std::convert::Infallible;
use tokio::net::TcpListener;
use uuid::Uuid;
use hyper::header::HeaderValue;
use hyper_util::server::conn::auto::Builder as AutoBuilder;
use hyper_util::rt::{TokioIo, TokioExecutor};
use hyper::service::service_fn;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use async_trait::async_trait;
use std::sync::RwLock; // Added for RwLock

use crate::pool::PoolManager;
use crate::router::{RadixTrie, RouteMatch}; // Modified to include RouteMatch
use crate::loadbalancer::{LoadBalancerContext, UpstreamGroup}; // Modified to include LoadBalancerContext
use crate::middleware::{
    MiddlewareChain, RequestContext, Next, ProxyHandler, ProxyResponse,
};

#[derive(Clone)]
pub struct ProxyServer {
    pool: PoolManager,
    router: Arc<RwLock<RadixTrie<UpstreamGroup>>>, // Modified type
    chain: Arc<MiddlewareChain>,
}

fn full_body<T: Into<Bytes>>(chunk: T) -> BoxBody<Bytes, hyper::Error> {
    Full::new(chunk.into())
        .map_err(|never| match never {})
        .boxed()
}

impl ProxyServer {
    pub fn new(pool: PoolManager, router: Arc<RwLock<RadixTrie<UpstreamGroup>>>, chain: MiddlewareChain) -> Self { // Modified parameter type
        Self {
            pool,
            router,
            chain: Arc::new(chain),
        }
    }

    pub async fn start(&self, addr: SocketAddr) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(addr).await?;
        println!("🚀 Bastion Proxy Engine listening on http://{}", addr);
        println!("🔄 HTTP/1.1 + HTTP/2 (h2c) enabled");
        println!("🛡️  {} middleware(s) loaded", self.chain.middlewares.len());

        loop {
            let (stream, client_addr) = listener.accept().await?;
            let io = TokioIo::new(stream);
            let proxy = self.clone();

            tokio::spawn(async move {
                if let Err(err) = AutoBuilder::new(TokioExecutor::new())
                    .serve_connection_with_upgrades(io, service_fn(move |req| {
                        proxy.clone().handle_request(req, client_addr)
                    }))
                    .await
                {
                    tracing::error!("Error serving connection: {:?}", err);
                }
            });
        }
    }

    async fn handle_request(
        self,
        req: Request<Incoming>,
        client_addr: SocketAddr,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Infallible> {
        let request_id = Uuid::new_v4().to_string();

        let ctx = RequestContext::new(request_id, client_addr.ip());

        // Build the Next chain with middlewares + final handler
        let terminal = ProxyTerminal {
            pool: self.pool.clone(),
            router: self.router.clone(),
        };

        let next = Next {
            middlewares: &self.chain.middlewares,
            final_handler: &terminal,
        };

        match next.run(req, &ctx).await {
            Ok(response) => Ok(response),
            Err(e) => {
                tracing::error!("Unhandled proxy error: {}", e);
                let mut error_res = Response::new(full_body("500 Internal Server Error\n"));
                *error_res.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                Ok(error_res)
            }
        }
    }
}

/// Terminal handler — the actual proxy logic that forwards to backends.
struct ProxyTerminal {
    pool: PoolManager,
    router: Arc<RwLock<RadixTrie<UpstreamGroup>>>,
}

#[async_trait]
impl ProxyHandler for ProxyTerminal {
    async fn call_proxy(
        &self,
        mut req: Request<Incoming>,
        ctx: &RequestContext,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        let path = req.uri().path().to_string();
        let method = req.method().clone();

        // 1. Router Lookup
        let lookup_result = {
            let r = self.router.read().unwrap();
            let x = if let Some(m) = r.lookup(&method, &path) {
                Some((m.value.clone(), m.rewritten_path.clone()))
            } else {
                None
            };
            x
        };

        let (upstream_group, rewritten_path) = match lookup_result {
            Some(res) => res,
            None => {
                let mut res = Response::new(full_body("404 Not Found - Bastion Gateway\n"));
                *res.status_mut() = StatusCode::NOT_FOUND;
                return Ok(res);
            }
        };

        // 2. Load Balancing (Round Robin)
        let backend = match upstream_group.next() {
            Some(b) => b,
            None => {
                let mut res = Response::new(full_body("503 Service Unavailable - No backends\n"));
                *res.status_mut() = StatusCode::SERVICE_UNAVAILABLE;
                return Ok(res);
            }
        };

        // Track active connection
        backend.active_connections.fetch_add(1, Ordering::SeqCst);

        // Store backend URL in context for MetricsMiddleware
        ctx.metadata.insert("backend_url".to_string(), backend.url.clone());

        // 3. Header Manipulation
        let headers = req.headers_mut();
        if let Ok(id_val) = HeaderValue::from_str(&ctx.request_id) {
            headers.insert("X-Request-ID", id_val);
        }
        let ip_str = ctx.client_ip.to_string();
        if let Ok(ip_val) = HeaderValue::from_str(&ip_str) {
            headers.insert("X-Real-IP", ip_val.clone());
            if let Some(existing) = headers.get("X-Forwarded-For") {
                if let Ok(existing_str) = existing.to_str() {
                    let new_xff = format!("{}, {}", existing_str, ip_str);
                    if let Ok(new_val) = HeaderValue::from_str(&new_xff) {
                        headers.insert("X-Forwarded-For", new_val);
                    }
                }
            } else {
                headers.insert("X-Forwarded-For", ip_val);
            }
        }

        // 4. URI re-writing
        let mut parts = req.uri().clone().into_parts();
        let backend_uri_parsed = backend.url.parse::<Uri>()?;
        parts.scheme = backend_uri_parsed.scheme().cloned();
        parts.authority = backend_uri_parsed.authority().cloned();

        if let Some(ref rewritten) = rewritten_path {
            let new_path_and_query = match req.uri().query() {
                Some(q) => format!("{}?{}", rewritten, q),
                None => rewritten.clone(),
            };
            parts.path_and_query = Some(new_path_and_query.parse()?);
        }

        if let Ok(new_uri) = Uri::from_parts(parts) {
            *req.uri_mut() = new_uri;
        }

        // 5. Proxy via Connection Pool
        let proxy_result = self.pool.client.request(req).await;
        backend.active_connections.fetch_sub(1, Ordering::SeqCst);

        match proxy_result {
            Ok(res) => {
                if res.status().is_server_error() {
                    backend.health.record_failure();
                } else {
                    backend.health.record_success();
                }

                let (parts, body) = res.into_parts();
                Ok(Response::from_parts(parts, body.boxed()))
            }
            Err(e) => {
                tracing::error!("Backend {} failed: {}", backend.url, e);
                backend.health.record_failure();
                let mut res = Response::new(full_body("502 Bad Gateway - Backend connection failed\n"));
                *res.status_mut() = StatusCode::BAD_GATEWAY;
                Ok(res)
            }
        }
    }
}
