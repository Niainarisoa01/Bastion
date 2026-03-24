use async_trait::async_trait;
use hyper::{Request, Response, StatusCode};
use hyper::body::{Incoming, Bytes};
use http_body_util::{BodyExt, Full};

use crate::middleware::chain::{Middleware, Next, ProxyResponse};
use crate::middleware::context::RequestContext;

#[derive(Clone, Debug)]
pub struct RequestValidationConfig {
    pub max_body_size: Option<u64>,       // in bytes
    pub required_content_types: Vec<String>,  // e.g., ["application/json"]
}

impl Default for RequestValidationConfig {
    fn default() -> Self {
        Self {
            max_body_size: Some(10 * 1024 * 1024), // 10 MB
            required_content_types: vec![],
        }
    }
}

pub struct RequestValidationMiddleware {
    config: RequestValidationConfig,
}

impl RequestValidationMiddleware {
    pub fn new(config: RequestValidationConfig) -> Self {
        Self { config }
    }

    fn error_response(status: StatusCode, msg: &str) -> ProxyResponse {
        let body = Full::new(Bytes::from(format!("{}\n", msg)))
            .map_err(|never| match never {})
            .boxed();
        let mut res = Response::new(body);
        *res.status_mut() = status;
        res
    }
}

#[async_trait]
impl Middleware for RequestValidationMiddleware {
    fn name(&self) -> &str {
        "request_validation"
    }

    fn priority(&self) -> i32 {
        85
    }

    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        // 1. Check Content-Length against max body size
        if let Some(max_size) = self.config.max_body_size {
            if let Some(content_length) = req.headers()
                .get("Content-Length")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
            {
                if content_length > max_size {
                    tracing::warn!(
                        size = content_length,
                        max = max_size,
                        "Request body too large"
                    );
                    return Ok(Self::error_response(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        "413 Payload Too Large",
                    ));
                }
            }
        }

        // 2. Validate required Content-Type
        if !self.config.required_content_types.is_empty() {
            let has_body = req.headers().contains_key("Content-Length")
                || req.headers().get("Transfer-Encoding")
                    .map(|v| v.to_str().unwrap_or("").contains("chunked"))
                    .unwrap_or(false);

            if has_body {
                let content_type = req.headers()
                    .get("Content-Type")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");

                let type_ok = self.config.required_content_types.iter()
                    .any(|allowed| content_type.starts_with(allowed));

                if !type_ok {
                    tracing::warn!(
                        content_type = content_type,
                        "Unsupported Content-Type"
                    );
                    return Ok(Self::error_response(
                        StatusCode::UNSUPPORTED_MEDIA_TYPE,
                        "415 Unsupported Media Type",
                    ));
                }
            }
        }

        next.run(req, ctx).await
    }
}
