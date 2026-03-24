use crate::middleware::{Middleware, Next, RequestContext};
use async_trait::async_trait;
use hyper::{Request, Response};
use hyper::body::Incoming;
use http_body_util::combinators::BoxBody;
use bytes::Bytes;
use std::sync::Arc;
use bastion_metrics::GatewayMetrics;
use std::time::Instant;

pub struct MetricsMiddleware {
    pub metrics: Arc<GatewayMetrics>,
}

impl MetricsMiddleware {
    pub fn new(metrics: Arc<GatewayMetrics>) -> Self {
        Self { metrics }
    }
}

#[async_trait]
impl Middleware for MetricsMiddleware {
    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<Response<BoxBody<Bytes, hyper::Error>>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        self.metrics.active_requests.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        
        let path = req.uri().path().to_string();
        let start = Instant::now();
        
        // Execute pipeline
        let res = next.run(req, ctx).await;
        
        let duration = start.elapsed();
        let latency_us = duration.as_micros() as u64;

        self.metrics.active_requests.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);

        let is_error = match &res {
            Ok(response) => response.status().is_server_error(),
            Err(_) => true,
        };

        // Read backend URL from context metadata (set by ProxyTerminal after load balancing)
        let backend_url = ctx.metadata.get("backend_url").map(|v| v.clone());
        self.metrics.record_request(&path, backend_url.as_deref(), latency_us, is_error);

        res
    }

    fn name(&self) -> &'static str {
        "MetricsMiddleware"
    }

    fn priority(&self) -> i32 {
        1000 // Highest priority to measure full latency
    }
}
