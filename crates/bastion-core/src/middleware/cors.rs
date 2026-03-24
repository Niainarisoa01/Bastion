use async_trait::async_trait;
use hyper::{Request, Response, StatusCode, Method};
use hyper::body::{Incoming, Bytes};
use hyper::header::HeaderValue;
use http_body_util::{BodyExt, Full};
use http_body_util::combinators::BoxBody;

use crate::middleware::chain::{Middleware, Next, ProxyResponse};
use crate::middleware::context::RequestContext;

#[derive(Clone, Debug)]
pub struct CorsConfig {
    pub allowed_origins: Vec<String>,
    pub allowed_methods: Vec<String>,
    pub allowed_headers: Vec<String>,
    pub max_age: u64,
    pub allow_credentials: bool,
}

impl Default for CorsConfig {
    fn default() -> Self {
        Self {
            allowed_origins: vec!["*".to_string()],
            allowed_methods: vec![
                "GET".to_string(), "POST".to_string(), "PUT".to_string(),
                "DELETE".to_string(), "PATCH".to_string(), "OPTIONS".to_string(),
            ],
            allowed_headers: vec![
                "Content-Type".to_string(), "Authorization".to_string(),
                "X-Requested-With".to_string(), "Accept".to_string(),
            ],
            max_age: 86400,
            allow_credentials: false,
        }
    }
}

pub struct CorsMiddleware {
    config: CorsConfig,
}

impl CorsMiddleware {
    pub fn new(config: CorsConfig) -> Self {
        Self { config }
    }

    fn is_origin_allowed(&self, origin: &str) -> bool {
        self.config.allowed_origins.iter().any(|o| o == "*" || o == origin)
    }

    fn apply_cors_headers(&self, res: &mut Response<BoxBody<Bytes, hyper::Error>>, origin: &str) {
        let headers = res.headers_mut();

        let origin_value = if self.config.allowed_origins.contains(&"*".to_string()) {
            "*"
        } else {
            origin
        };

        if let Ok(v) = HeaderValue::from_str(origin_value) {
            headers.insert("Access-Control-Allow-Origin", v);
        }

        let methods = self.config.allowed_methods.join(", ");
        if let Ok(v) = HeaderValue::from_str(&methods) {
            headers.insert("Access-Control-Allow-Methods", v);
        }

        let hdrs = self.config.allowed_headers.join(", ");
        if let Ok(v) = HeaderValue::from_str(&hdrs) {
            headers.insert("Access-Control-Allow-Headers", v);
        }

        if let Ok(v) = HeaderValue::from_str(&self.config.max_age.to_string()) {
            headers.insert("Access-Control-Max-Age", v);
        }

        if self.config.allow_credentials {
            headers.insert("Access-Control-Allow-Credentials", HeaderValue::from_static("true"));
        }
    }
}

#[async_trait]
impl Middleware for CorsMiddleware {
    fn name(&self) -> &str {
        "cors"
    }

    fn priority(&self) -> i32 {
        95 // Run very early, before auth
    }

    async fn handle(
        &self,
        req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        let origin = req.headers()
            .get("Origin")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // Check if origin is allowed
        if !origin.is_empty() && !self.is_origin_allowed(&origin) {
            let body = Full::new(Bytes::from("403 Forbidden - Origin not allowed\n"))
                .map_err(|never| match never {})
                .boxed();
            let mut res = Response::new(body);
            *res.status_mut() = StatusCode::FORBIDDEN;
            return Ok(res);
        }

        // Handle preflight OPTIONS
        if req.method() == Method::OPTIONS {
            let body = Full::new(Bytes::from(""))
                .map_err(|never| match never {})
                .boxed();
            let mut res = Response::new(body);
            *res.status_mut() = StatusCode::NO_CONTENT;
            self.apply_cors_headers(&mut res, &origin);
            return Ok(res);
        }

        // Process request and attach CORS headers to response
        let mut response = next.run(req, ctx).await?;
        if !origin.is_empty() {
            self.apply_cors_headers(&mut response, &origin);
        }
        Ok(response)
    }
}
