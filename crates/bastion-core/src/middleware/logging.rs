use async_trait::async_trait;
use hyper::Request;
use hyper::body::Incoming;
use std::time::Instant;
use tracing::{info, error, info_span, Instrument};

use crate::middleware::chain::{Middleware, Next, ProxyResponse};
use crate::middleware::context::RequestContext;

/// Structured logging middleware. Logs method, URI, status, latency.
pub struct LogMiddleware;

impl LogMiddleware {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Middleware for LogMiddleware {
    fn name(&self) -> &str {
        "logger"
    }

    fn priority(&self) -> i32 {
        -100 // Run last (outermost — wraps everything)
    }

    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        let method = req.method().clone();
        let uri = req.uri().clone();
        let version = req.version();
        let start = Instant::now();

        let span = info_span!("request",
            req_id = %ctx.request_id,
            method = %method,
            uri = %uri,
            version = ?version,
        );

        async move {
            let res = next.run(req, ctx).await;
            let elapsed = start.elapsed().as_millis();

            match &res {
                Ok(response) => {
                    info!(
                        status = %response.status().as_u16(),
                        latency_ms = %elapsed,
                        "Request completed"
                    );
                }
                Err(e) => {
                    error!(
                        latency_ms = %elapsed,
                        error = %e,
                        "Request failed"
                    );
                }
            }
            res
        }
        .instrument(span)
        .await
    }
}
