use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use dashmap::DashMap;

/// Token Bucket — lock-free burst control.
/// Allows `capacity` requests, refills at `refill_rate` tokens/sec.
pub struct TokenBucket {
    capacity: u64,
    refill_rate: f64,  // tokens per second
    buckets: DashMap<String, BucketState>,
}

struct BucketState {
    tokens: AtomicU64,
    last_refill: std::sync::Mutex<Instant>,
    capacity: u64,
    refill_rate: f64,
}

impl TokenBucket {
    pub fn new(capacity: u64, refill_rate: f64) -> Self {
        Self {
            capacity,
            refill_rate,
            buckets: DashMap::new(),
        }
    }

    /// Try to consume one token for the given key. Returns (allowed, remaining).
    pub fn try_acquire(&self, key: &str) -> (bool, u64) {
        let entry = self.buckets.entry(key.to_string()).or_insert_with(|| {
            BucketState {
                tokens: AtomicU64::new(self.capacity),
                last_refill: std::sync::Mutex::new(Instant::now()),
                capacity: self.capacity,
                refill_rate: self.refill_rate,
            }
        });

        let state = entry.value();
        
        // Refill tokens based on elapsed time
        {
            let mut last = state.last_refill.lock().unwrap();
            let elapsed = last.elapsed().as_secs_f64();
            let new_tokens = (elapsed * state.refill_rate) as u64;
            if new_tokens > 0 {
                let current = state.tokens.load(Ordering::Relaxed);
                let refilled = std::cmp::min(current + new_tokens, state.capacity);
                state.tokens.store(refilled, Ordering::Relaxed);
                *last = Instant::now();
            }
        }

        // Try to consume
        loop {
            let current = state.tokens.load(Ordering::Relaxed);
            if current == 0 {
                return (false, 0);
            }
            match state.tokens.compare_exchange_weak(
                current,
                current - 1,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ) {
                Ok(_) => return (true, current - 1),
                Err(_) => continue, // retry CAS loop
            }
        }
    }
}

/// Sliding Window — time-based counter per key.
/// Counts requests in the last `window` duration.
pub struct SlidingWindow {
    limit: u64,
    window: Duration,
    /// Maps key → Vec of request timestamps (as epoch millis)
    entries: DashMap<String, Vec<u64>>,
}

impl SlidingWindow {
    pub fn new(limit: u64, window: Duration) -> Self {
        Self {
            limit,
            window,
            entries: DashMap::new(),
        }
    }

    /// Check if a request for this key is allowed. Returns (allowed, remaining, retry_after_ms).
    pub fn try_acquire(&self, key: &str) -> (bool, u64, Option<u64>) {
        let now = Self::now_millis();
        let window_ms = self.window.as_millis() as u64;
        let cutoff = now.saturating_sub(window_ms);

        let mut entry = self.entries.entry(key.to_string()).or_default();
        let timestamps = entry.value_mut();

        // Evict expired entries
        timestamps.retain(|&ts| ts > cutoff);

        let count = timestamps.len() as u64;

        if count >= self.limit {
            // Calculate retry_after: time until the oldest entry expires
            let oldest = timestamps.first().copied().unwrap_or(now);
            let retry_after = (oldest + window_ms).saturating_sub(now);
            return (false, 0, Some(retry_after));
        }

        timestamps.push(now);
        let remaining = self.limit - count - 1;
        (true, remaining, None)
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}
