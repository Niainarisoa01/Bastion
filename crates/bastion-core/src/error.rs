use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Upstream error: {0}")]
    Upstream(String),
    
    #[error("Route not found: {0}")]
    RouteNotFound(String),

    #[error("Hyper error: {0}")]
    Hyper(#[from] hyper::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] hyper::http::Error),

    #[error("Rate limit exceeded")]
    RateLimitExceeded,

    #[error("Unauthorized: {0}")]
    Unauthorized(String),
    
    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Service unavailable (Circuit breaker open)")]
    ServiceUnavailable,

    #[error("Timeout occurred")]
    Timeout,

    #[error("Internal gateway error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, GatewayError>;
