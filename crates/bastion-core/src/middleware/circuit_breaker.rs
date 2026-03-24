use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::time::{Duration, Instant};
use std::sync::Mutex;
use dashmap::DashMap;

/// Circuit Breaker states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CbState {
    Closed = 0,   // Normal: requests pass through
    Open = 1,     // Tripped: all requests fail fast
    HalfOpen = 2, // Probe: limited requests to test recovery
}

impl From<u8> for CbState {
    fn from(v: u8) -> Self {
        match v {
            0 => CbState::Closed,
            1 => CbState::Open,
            2 => CbState::HalfOpen,
            _ => CbState::Closed,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of consecutive failures to trip (Closed → Open)
    pub failure_threshold: u64,
    /// Duration to stay Open before transitioning to HalfOpen
    pub open_timeout: Duration,
    /// Number of probe requests allowed in HalfOpen state
    pub half_open_max_calls: u64,
    /// Number of consecutive successes in HalfOpen to close
    pub success_threshold: u64,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            open_timeout: Duration::from_secs(30),
            half_open_max_calls: 3,
            success_threshold: 2,
        }
    }
}

/// A single Circuit Breaker instance.
pub struct CircuitBreaker {
    state: AtomicU8,
    failure_count: AtomicU64,
    success_count: AtomicU64,
    half_open_calls: AtomicU64,
    last_failure_time: Mutex<Option<Instant>>,
    config: CircuitBreakerConfig,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: AtomicU8::new(CbState::Closed as u8),
            failure_count: AtomicU64::new(0),
            success_count: AtomicU64::new(0),
            half_open_calls: AtomicU64::new(0),
            last_failure_time: Mutex::new(None),
            config,
        }
    }

    pub fn state(&self) -> CbState {
        let raw = self.state.load(Ordering::SeqCst);
        // Check if Open → should transition to HalfOpen
        if raw == CbState::Open as u8 {
            if let Ok(last) = self.last_failure_time.lock() {
                if let Some(t) = *last {
                    if t.elapsed() >= self.config.open_timeout {
                        // Attempt transition Open → HalfOpen
                        let _ = self.state.compare_exchange(
                            CbState::Open as u8,
                            CbState::HalfOpen as u8,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        );
                        self.half_open_calls.store(0, Ordering::SeqCst);
                        self.success_count.store(0, Ordering::SeqCst);
                        return CbState::HalfOpen;
                    }
                }
            }
        }
        CbState::from(raw)
    }

    /// Check if a request is allowed. Returns `Ok(())` if allowed, `Err(CbState::Open)` if rejected.
    pub fn allow_request(&self) -> Result<(), CbState> {
        match self.state() {
            CbState::Closed => Ok(()),
            CbState::Open => Err(CbState::Open),
            CbState::HalfOpen => {
                let calls = self.half_open_calls.fetch_add(1, Ordering::SeqCst);
                if calls < self.config.half_open_max_calls {
                    Ok(())
                } else {
                    Err(CbState::HalfOpen)
                }
            }
        }
    }

    /// Record a successful request.
    pub fn record_success(&self) {
        match self.state() {
            CbState::Closed => {
                self.failure_count.store(0, Ordering::SeqCst);
            }
            CbState::HalfOpen => {
                let successes = self.success_count.fetch_add(1, Ordering::SeqCst) + 1;
                if successes >= self.config.success_threshold {
                    // HalfOpen → Closed
                    self.state.store(CbState::Closed as u8, Ordering::SeqCst);
                    self.failure_count.store(0, Ordering::SeqCst);
                    self.success_count.store(0, Ordering::SeqCst);
                    tracing::info!("Circuit breaker closed (recovered)");
                }
            }
            _ => {}
        }
    }

    /// Record a failed request.
    pub fn record_failure(&self) {
        match self.state() {
            CbState::Closed => {
                let failures = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
                if failures >= self.config.failure_threshold {
                    // Closed → Open
                    self.state.store(CbState::Open as u8, Ordering::SeqCst);
                    *self.last_failure_time.lock().unwrap() = Some(Instant::now());
                    tracing::warn!("Circuit breaker opened after {} failures", failures);
                }
            }
            CbState::HalfOpen => {
                // Any failure in HalfOpen → back to Open
                self.state.store(CbState::Open as u8, Ordering::SeqCst);
                *self.last_failure_time.lock().unwrap() = Some(Instant::now());
                self.half_open_calls.store(0, Ordering::SeqCst);
                self.success_count.store(0, Ordering::SeqCst);
                tracing::warn!("Circuit breaker re-opened (half-open probe failed)");
            }
            _ => {}
        }
    }

    pub fn failure_count(&self) -> u64 {
        self.failure_count.load(Ordering::SeqCst)
    }
}

/// Registry of Circuit Breakers keyed by upstream group name.
pub struct CircuitBreakerRegistry {
    breakers: DashMap<String, CircuitBreaker>,
    default_config: CircuitBreakerConfig,
}

impl CircuitBreakerRegistry {
    pub fn new(default_config: CircuitBreakerConfig) -> Self {
        Self {
            breakers: DashMap::new(),
            default_config,
        }
    }

    pub fn get_or_create(&self, name: &str) -> dashmap::mapref::one::Ref<'_, String, CircuitBreaker> {
        if !self.breakers.contains_key(name) {
            self.breakers.insert(
                name.to_string(),
                CircuitBreaker::new(self.default_config.clone()),
            );
        }
        self.breakers.get(name).unwrap()
    }
}
