use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct HealthConfig {
    pub path: String,
    pub interval: std::time::Duration,
    pub timeout: std::time::Duration,
    pub healthy_threshold: u8,
    pub unhealthy_threshold: u8,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            path: "/health".to_string(),
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(2),
            healthy_threshold: 2,
            unhealthy_threshold: 3,
        }
    }
}

// 0 = Healthy, 1 = Unhealthy, 2 = Draining
#[derive(Debug)]
pub struct BackendHealth {
    state: AtomicU8,
    consecutive_successes: AtomicU8,
    consecutive_failures: AtomicU8,
    pub config: HealthConfig,
}

impl BackendHealth {
    pub fn new(config: HealthConfig) -> Self {
        Self {
            state: AtomicU8::new(0), // Default Healthy
            consecutive_successes: AtomicU8::new(0),
            consecutive_failures: AtomicU8::new(0),
            config,
        }
    }

    pub fn is_healthy(&self) -> bool {
        self.state.load(Ordering::Acquire) == 0
    }

    pub fn is_draining(&self) -> bool {
        self.state.load(Ordering::Acquire) == 2
    }

    pub fn set_draining(&self) {
        self.state.store(2, Ordering::Release);
    }

    pub fn record_success(&self) {
        if self.is_draining() { return; }
        
        let successes = self.consecutive_successes.fetch_add(1, Ordering::SeqCst) + 1;
        self.consecutive_failures.store(0, Ordering::Release);

        let state = self.state.load(Ordering::Acquire);
        if state == 1 && successes >= self.config.healthy_threshold {
            // Unhealthy -> Healthy
            self.state.store(0, Ordering::Release);
            self.consecutive_successes.store(0, Ordering::Release);
            tracing::info!("Backend health: recovered to Healthy");
        }
    }

    pub fn record_failure(&self) {
        if self.is_draining() { return; }

        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        self.consecutive_successes.store(0, Ordering::Release);

        let state = self.state.load(Ordering::Acquire);
        if state == 0 && failures >= self.config.unhealthy_threshold {
            // Healthy -> Unhealthy
            self.state.store(1, Ordering::Release);
            self.consecutive_failures.store(0, Ordering::Release);
            tracing::warn!("Backend health: degraded to Unhealthy");
        }
    }
}
