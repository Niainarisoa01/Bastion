use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Backend {
    pub url: String,
    pub weight: u32,
    pub active_connections: Arc<AtomicUsize>,
}

impl Backend {
    pub fn new(url: &str, weight: u32) -> Self {
        Self {
            url: url.to_string(),
            weight,
            active_connections: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[derive(Clone, Debug)]
pub struct UpstreamGroup {
    pub name: String,
    pub backends: Vec<Backend>,
    current_idx: Arc<AtomicUsize>,
}

impl UpstreamGroup {
    pub fn new(name: &str, backends: Vec<Backend>) -> Self {
        Self {
            name: name.to_string(),
            backends,
            current_idx: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn add_backend(&mut self, backend: Backend) {
        self.backends.push(backend);
    }

    /// Distribution cyclique atomique (Round Robin)
    pub fn next(&self) -> Option<Backend> {
        if self.backends.is_empty() {
            return None;
        }
        let idx = self.current_idx.fetch_add(1, Ordering::Relaxed);
        Some(self.backends[idx % self.backends.len()].clone())
    }
}
