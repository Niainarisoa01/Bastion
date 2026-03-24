use crate::middleware::{Middleware, Next, RequestContext};
use async_trait::async_trait;
use hyper::{Request, Response, StatusCode, Method, header::HeaderValue};
use hyper::body::Incoming;
use http_body_util::{BodyExt, Full};
use http_body_util::combinators::BoxBody;
use bytes::Bytes;
use std::sync::Arc;
use bastion_cache::ShardedLruCache;

pub struct CacheMiddleware {
    pub cache: Arc<ShardedLruCache>,
}

impl CacheMiddleware {
    pub fn new(cache: Arc<ShardedLruCache>) -> Self {
        Self { cache }
    }

    fn generate_key(req: &Request<Incoming>) -> String {
        format!("{}:{}", req.method(), req.uri().path())
    }

    fn invalidation_key(req: &Request<Incoming>) -> String {
        format!("GET:{}", req.uri().path())
    }
}

#[async_trait]
impl Middleware for CacheMiddleware {
    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        let method = req.method().clone();

        // Bypass for non-GET methods and invalidate cache
        if method != Method::GET {
            if method == Method::POST || method == Method::PUT || method == Method::DELETE {
                let inv_key = Self::invalidation_key(&req);
                self.cache.remove(&inv_key);
            }

            let mut res = next.run(req, ctx).await?;
            res.headers_mut().insert(
                "X-Cache",
                HeaderValue::from_static("BYPASS"),
            );
            return Ok(res);
        }

        // Cache Key Generation
        let key = Self::generate_key(&req);

        // Respect Cache-Control: no-cache
        let skip_cache = req.headers()
            .get("Cache-Control")
            .and_then(|v| v.to_str().ok())
            .map(|v| v.contains("no-cache") || v.contains("no-store"))
            .unwrap_or(false);

        // Check Cache HIT (only if not bypassed)
        if !skip_cache {
            if let Some(cached_body) = self.cache.get(&key) {
                let mut res = Response::new(
                    Full::new(cached_body)
                        .map_err(|never| match never {})
                        .boxed(),
                );
                res.headers_mut().insert(
                    "X-Cache",
                    HeaderValue::from_static("HIT"),
                );
                return Ok(res);
            }
        }

        // Cache MISS: forward the request
        let mut res = next.run(req, ctx).await?;

        // Only cache 200 OK responses
        if res.status() == StatusCode::OK {
            // Read body to bytes
            let (parts, body) = res.into_parts();
            let bytes = body.collect().await?.to_bytes();

            // Store in cache
            self.cache.put(key, bytes.clone(), None); // No specific TTL for now, handled by Cache Config

            // Reconstruct response
            let body_stream = Full::new(bytes)
                .map_err(|never| match never {})
                .boxed();
            
            res = Response::from_parts(parts, body_stream);
        }

        res.headers_mut().insert(
            "X-Cache",
            HeaderValue::from_static("MISS"),
        );
        Ok(res)
    }

    fn name(&self) -> &'static str {
        "CacheMiddleware"
    }

    fn priority(&self) -> i32 {
        // Cache should be after Auth but before RateLimit, or after RateLimit.
        // Let's set priority to 10 (Auth is 50, IP is 100, RateLimit is 20).
        10
    }
}
