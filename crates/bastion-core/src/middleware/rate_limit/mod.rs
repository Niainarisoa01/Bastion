pub mod store;

use async_trait::async_trait;
use hyper::{Request, Response, StatusCode};
use hyper::body::{Incoming, Bytes};
use hyper::header::HeaderValue;
use http_body_util::{BodyExt, Full};
use http_body_util::combinators::BoxBody;
use std::time::Duration;

use crate::middleware::chain::{Middleware, Next, ProxyResponse};
use crate::middleware::context::RequestContext;
use self::store::{TokenBucket, SlidingWindow};

/// Strategy for rate limiting.
#[derive(Clone, Debug)]
pub enum RateLimitStrategy {
    TokenBucket,
    SlidingWindow,
}

/// How to extract the key for rate limit bucketing.
#[derive(Clone, Debug)]
pub enum KeyExtractor {
    /// Use client IP address
    Ip,
    /// Use a specific header value
    Header(String),
    /// Use a specific API key header
    ApiKey(String),
    /// Composite: IP + path
    Composite,
}

/// Configuration for a rate limiter.
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    pub limit: u64,
    pub window: Duration,
    pub burst: u64,
    pub strategy: RateLimitStrategy,
    pub key_extractor: KeyExtractor,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            limit: 100,
            window: Duration::from_secs(60),
            burst: 10,
            strategy: RateLimitStrategy::SlidingWindow,
            key_extractor: KeyExtractor::Ip,
        }
    }
}

/// Rate Limiter Middleware
pub struct RateLimiterMiddleware {
    name: String,
    config: RateLimitConfig,
    token_bucket: Option<TokenBucket>,
    sliding_window: Option<SlidingWindow>,
}

impl RateLimiterMiddleware {
    pub fn new(config: RateLimitConfig) -> Self {
        let token_bucket = match config.strategy {
            RateLimitStrategy::TokenBucket => {
                let refill_rate = config.limit as f64 / config.window.as_secs_f64();
                Some(TokenBucket::new(config.burst, refill_rate))
            }
            _ => None,
        };

        let sliding_window = match config.strategy {
            RateLimitStrategy::SlidingWindow => {
                Some(SlidingWindow::new(config.limit, config.window))
            }
            _ => None,
        };

        Self {
            name: "rate_limiter".to_string(),
            config,
            token_bucket,
            sliding_window,
        }
    }

    fn extract_key(&self, req: &Request<Incoming>, ctx: &RequestContext) -> String {
        match &self.config.key_extractor {
            KeyExtractor::Ip => ctx.client_ip.to_string(),
            KeyExtractor::Header(header_name) => {
                req.headers()
                    .get(header_name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("unknown")
                    .to_string()
            }
            KeyExtractor::ApiKey(header_name) => {
                req.headers()
                    .get(header_name)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("anonymous")
                    .to_string()
            }
            KeyExtractor::Composite => {
                format!("{}:{}", ctx.client_ip, req.uri().path())
            }
        }
    }

    fn build_429_response(
        &self,
        remaining: u64,
        retry_after: Option<u64>,
    ) -> ProxyResponse {
        let body = Full::new(Bytes::from("429 Too Many Requests\n"))
            .map_err(|never| match never {})
            .boxed();
        let mut res = Response::new(body);
        *res.status_mut() = StatusCode::TOO_MANY_REQUESTS;

        let headers = res.headers_mut();
        if let Ok(v) = HeaderValue::from_str(&self.config.limit.to_string()) {
            headers.insert("X-RateLimit-Limit", v);
        }
        if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
            headers.insert("X-RateLimit-Remaining", v);
        }
        if let Some(retry_ms) = retry_after {
            let secs = (retry_ms as f64 / 1000.0).ceil() as u64;
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                headers.insert("Retry-After", v);
            }
        }

        res
    }
}



#[async_trait]
impl Middleware for RateLimiterMiddleware {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> i32 {
        100 // Run early in the chain
    }

    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        let key = self.extract_key(&req, ctx);

        // Check rate limit
        match self.config.strategy {
            RateLimitStrategy::TokenBucket => {
                if let Some(ref bucket) = self.token_bucket {
                    let (allowed, remaining) = bucket.try_acquire(&key);
                    if !allowed {
                        tracing::warn!(key = %key, "Rate limit exceeded (token bucket)");
                        return Ok(self.build_429_response(remaining, None));
                    }
                    // Add rate limit info headers to response
                    let mut response = next.run(req, ctx).await?;
                    let headers = response.headers_mut();
                    if let Ok(v) = HeaderValue::from_str(&self.config.burst.to_string()) {
                        headers.insert("X-RateLimit-Limit", v);
                    }
                    if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
                        headers.insert("X-RateLimit-Remaining", v);
                    }
                    return Ok(response);
                }
            }
            RateLimitStrategy::SlidingWindow => {
                if let Some(ref window) = self.sliding_window {
                    let (allowed, remaining, retry_after) = window.try_acquire(&key);
                    if !allowed {
                        tracing::warn!(key = %key, "Rate limit exceeded (sliding window)");
                        return Ok(self.build_429_response(remaining, retry_after));
                    }
                    let mut response = next.run(req, ctx).await?;
                    let headers = response.headers_mut();
                    if let Ok(v) = HeaderValue::from_str(&self.config.limit.to_string()) {
                        headers.insert("X-RateLimit-Limit", v);
                    }
                    if let Ok(v) = HeaderValue::from_str(&remaining.to_string()) {
                        headers.insert("X-RateLimit-Remaining", v);
                    }
                    return Ok(response);
                }
            }
        }

        // Fallback — no limiter configured, pass through
        next.run(req, ctx).await
    }
}
