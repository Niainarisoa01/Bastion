use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct CacheMetrics {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub evictions: AtomicU64,
    pub bytes_used: AtomicU64,
}

impl CacheMetrics {
    #[inline]
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn add_bytes(&self, bytes: u64) {
        self.bytes_used.fetch_add(bytes, Ordering::Relaxed);
    }

    #[inline]
    pub fn sub_bytes(&self, bytes: u64) {
        self.bytes_used.fetch_sub(bytes, Ordering::Relaxed);
    }
}
