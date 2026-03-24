use std::time::Duration;
use hyper::Method;
use rand::Rng;

#[derive(Clone, Debug)]
pub enum BackoffStrategy {
    Constant(Duration),
    Linear(Duration),        // base * attempt
    Exponential(Duration),   // base * 2^attempt
}

#[derive(Clone, Debug)]
pub struct RetryConfig {
    pub max_retries: u32,
    pub backoff: BackoffStrategy,
    pub jitter: bool,
    /// HTTP status codes that trigger a retry
    pub retryable_status_codes: Vec<u16>,
    /// Only retry idempotent methods (GET, HEAD, PUT, DELETE, OPTIONS)
    pub idempotent_only: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            backoff: BackoffStrategy::Exponential(Duration::from_millis(100)),
            jitter: true,
            retryable_status_codes: vec![502, 503, 504],
            idempotent_only: true,
        }
    }
}

impl RetryConfig {
    /// Check if a method is idempotent (safe to retry)
    pub fn is_idempotent(method: &Method) -> bool {
        matches!(
            *method,
            Method::GET | Method::HEAD | Method::PUT | Method::DELETE | Method::OPTIONS
        )
    }

    /// Check if a status code should trigger a retry
    pub fn should_retry_status(&self, status: u16) -> bool {
        self.retryable_status_codes.contains(&status)
    }

    /// Calculate the delay for a given attempt (0-indexed)
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base_delay = match &self.backoff {
            BackoffStrategy::Constant(d) => *d,
            BackoffStrategy::Linear(base) => *base * (attempt + 1),
            BackoffStrategy::Exponential(base) => {
                let multiplier = 2u64.saturating_pow(attempt);
                *base * multiplier as u32
            }
        };

        if self.jitter {
            let jitter_range = base_delay.as_millis() as u64 / 4; // ±25%
            if jitter_range > 0 {
                let jitter = rand::thread_rng().gen_range(0..jitter_range);
                base_delay + Duration::from_millis(jitter)
            } else {
                base_delay
            }
        } else {
            base_delay
        }
    }
}
