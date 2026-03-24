use async_trait::async_trait;
use hyper::{Request, Response, StatusCode};
use hyper::body::{Incoming, Bytes};
use hyper::header::{HeaderValue, HeaderName};
use http_body_util::{BodyExt, Full};
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};
use serde::{Deserialize, Serialize};

use crate::middleware::chain::{Middleware, Next, ProxyResponse};
use crate::middleware::context::RequestContext;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    #[serde(default)]
    pub role: String,
    pub exp: usize,
    #[serde(default)]
    pub iat: usize,
}

#[derive(Clone, Debug)]
pub enum JwtSecret {
    Hmac(String),
    Rsa(String), // PEM-encoded public key
}

#[derive(Clone, Debug)]
pub struct JwtConfig {
    pub secret: JwtSecret,
    pub skip_paths: Vec<String>,
    pub algorithms: Vec<Algorithm>,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            secret: JwtSecret::Hmac("default-secret-change-me".to_string()),
            skip_paths: vec!["/login".to_string(), "/register".to_string(), "/health".to_string()],
            algorithms: vec![Algorithm::HS256],
        }
    }
}

pub struct JwtMiddleware {
    config: JwtConfig,
}

impl JwtMiddleware {
    pub fn new(config: JwtConfig) -> Self {
        Self { config }
    }

    fn should_skip(&self, path: &str) -> bool {
        self.config.skip_paths.iter().any(|p| path.starts_with(p))
    }

    fn validate_token(&self, token: &str) -> Result<Claims, String> {
        let mut validation = Validation::new(self.config.algorithms[0]);
        validation.set_required_spec_claims(&["sub", "exp"]);

        let key = match &self.config.secret {
            JwtSecret::Hmac(secret) => DecodingKey::from_secret(secret.as_bytes()),
            JwtSecret::Rsa(pem) => {
                DecodingKey::from_rsa_pem(pem.as_bytes())
                    .map_err(|e| format!("Invalid RSA key: {}", e))?
            }
        };

        let token_data = decode::<Claims>(token, &key, &validation)
            .map_err(|e| format!("JWT validation failed: {}", e))?;

        Ok(token_data.claims)
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
impl Middleware for JwtMiddleware {
    fn name(&self) -> &str {
        "jwt_auth"
    }

    fn priority(&self) -> i32 {
        90 // Run after rate limiter but before request processing
    }

    async fn handle(
        &self,
        mut req: Request<Incoming>,
        ctx: &RequestContext,
        next: Next<'_>,
    ) -> Result<ProxyResponse, Box<dyn std::error::Error + Send + Sync>> {
        let path = req.uri().path().to_string();

        // Skip paths (login, register, health)
        if self.should_skip(&path) {
            return next.run(req, ctx).await;
        }

        // Extract token from Authorization: Bearer <token>
        let auth_header = req.headers().get("Authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let token = match auth_header {
            Some(ref h) if h.starts_with("Bearer ") => &h[7..],
            _ => {
                tracing::warn!(path = %path, "Missing or invalid Authorization header");
                return Ok(Self::error_response(
                    StatusCode::UNAUTHORIZED,
                    "401 Unauthorized - Missing Bearer token",
                ));
            }
        };

        // Validate JWT
        match self.validate_token(token) {
            Ok(claims) => {
                // Inject claims as headers for upstream
                let headers = req.headers_mut();
                if let Ok(v) = HeaderValue::from_str(&claims.sub) {
                    headers.insert(
                        HeaderName::from_static("x-user-id"),
                        v,
                    );
                }
                if let Ok(v) = HeaderValue::from_str(&claims.role) {
                    headers.insert(
                        HeaderName::from_static("x-user-role"),
                        v,
                    );
                }

                // Store in metadata for other middlewares
                ctx.metadata.insert("user_id".to_string(), claims.sub);
                ctx.metadata.insert("user_role".to_string(), claims.role);

                next.run(req, ctx).await
            }
            Err(e) => {
                tracing::warn!(path = %path, error = %e, "JWT validation failed");
                Ok(Self::error_response(
                    StatusCode::UNAUTHORIZED,
                    "401 Unauthorized - Invalid token",
                ))
            }
        }
    }
}
