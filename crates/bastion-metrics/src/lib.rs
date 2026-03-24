use dashmap::DashMap;
use hdrhistogram::Histogram;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Global metrics for the entire Gateway
#[derive(Debug)]
pub struct GatewayMetrics {
    pub total_requests: AtomicU64,
    pub active_requests: AtomicU64,
    pub total_errors: AtomicU64,
    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub routes: DashMap<String, Arc<RouteMetrics>>,
    pub backends: DashMap<String, Arc<BackendMetrics>>,
    pub global_latency: Mutex<Histogram<u64>>,
}

impl Default for GatewayMetrics {
    fn default() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            active_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            routes: DashMap::new(),
            backends: DashMap::new(),
            // Histogram from 1us to 1min (60_000_000_000us) with 3 significant figures
            global_latency: Mutex::new(Histogram::<u64>::new_with_bounds(1, 60_000_000_000, 3).unwrap()),
        }
    }
}

/// Metrics per route (e.g. /api/users)
#[derive(Debug)]
pub struct RouteMetrics {
    pub total_requests: AtomicU64,
    pub total_errors: AtomicU64,
    pub latency: Mutex<Histogram<u64>>,
}

impl Default for RouteMetrics {
    fn default() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            latency: Mutex::new(Histogram::<u64>::new_with_bounds(1, 60_000_000_000, 3).unwrap()),
        }
    }
}

/// Metrics per backend (e.g. http://10.0.0.5:8000)
#[derive(Debug)]
pub struct BackendMetrics {
    pub active_connections: AtomicU64,
    pub total_requests: AtomicU64,
    pub total_errors: AtomicU64,
    pub latency: Mutex<Histogram<u64>>,
}

impl Default for BackendMetrics {
    fn default() -> Self {
        Self {
            active_connections: AtomicU64::new(0),
            total_requests: AtomicU64::new(0),
            total_errors: AtomicU64::new(0),
            latency: Mutex::new(Histogram::<u64>::new_with_bounds(1, 60_000_000_000, 3).unwrap()),
        }
    }
}

impl GatewayMetrics {
    pub fn get_or_create_route(&self, path: &str) -> Arc<RouteMetrics> {
        self.routes.entry(path.to_string())
            .or_insert_with(|| Arc::new(RouteMetrics::default()))
            .clone()
    }

    pub fn get_or_create_backend(&self, url: &str) -> Arc<BackendMetrics> {
        self.backends.entry(url.to_string())
            .or_insert_with(|| Arc::new(BackendMetrics::default()))
            .clone()
    }

    pub fn record_request(&self, route_path: &str, backend_url: Option<&str>, latency_us: u64, is_error: bool) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
        
        let route = self.get_or_create_route(route_path);
        route.total_requests.fetch_add(1, Ordering::Relaxed);
        
        // Lock to record latency; these histograms are very fast but a mutex could induce latency under extreme load.
        // We use parking_lot::Mutex for performance.
        let _ = self.global_latency.lock().record(latency_us);
        let _ = route.latency.lock().record(latency_us);

        if is_error {
            self.total_errors.fetch_add(1, Ordering::Relaxed);
            route.total_errors.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(url) = backend_url {
            let backend = self.get_or_create_backend(url);
            backend.total_requests.fetch_add(1, Ordering::Relaxed);
            let _ = backend.latency.lock().record(latency_us);
            if is_error {
                backend.total_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

pub mod prometheus;
pub mod timeseries;
