use std::sync::atomic::{AtomicUsize, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::collections::BTreeMap;
use xxhash_rust::xxh64::xxh64;

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

pub struct LoadBalancerContext {
    pub hash_key: Option<String>,
}

pub trait LoadBalancer: Send + Sync + std::fmt::Debug {
    fn next(&self, ctx: &LoadBalancerContext) -> Option<Backend>;
    fn add_backend(&mut self, backend: Backend);
    fn backends(&self) -> Vec<Backend>;
}

// ----------------------------------------------------
// 1. Basic / Smooth Weighted Round Robin (Nginx style)
// ----------------------------------------------------
#[derive(Debug)]
pub struct WeightedRoundRobin {
    backends: Vec<Backend>,
    states: Vec<AtomicI64>, // current_weight per backend, now truly lock-free
}

impl WeightedRoundRobin {
    pub fn new(backends: Vec<Backend>) -> Self {
        let mut states = Vec::with_capacity(backends.len());
        for _ in 0..backends.len() {
            states.push(AtomicI64::new(0));
        }
        Self {
            backends,
            states,
        }
    }
}

impl LoadBalancer for WeightedRoundRobin {
    fn next(&self, _ctx: &LoadBalancerContext) -> Option<Backend> {
        if self.backends.is_empty() { return None; }
        if self.backends.len() == 1 { return Some(self.backends[0].clone()); }
        
        let mut total_weight = 0;
        let mut best_idx = 0;
        let mut best_weight = i64::MIN;

        for (i, backend) in self.backends.iter().enumerate() {
            let weight = backend.weight as i64;
            
            // Advance state (atomic cross-thread progress)
            let current = self.states[i].fetch_add(weight, Ordering::SeqCst) + weight;
            total_weight += weight;
            
            if current > best_weight {
                best_idx = i;
                best_weight = current;
            }
        }
        
        // Decrease best by total_weight
        self.states[best_idx].fetch_sub(total_weight, Ordering::SeqCst);
        
        Some(self.backends[best_idx].clone())
    }

    fn add_backend(&mut self, backend: Backend) {
        self.backends.push(backend);
        self.states.push(AtomicI64::new(0));
    }

    fn backends(&self) -> Vec<Backend> {
        self.backends.clone()
    }
}

// ----------------------------------------------------
// 2. Least Connections
// ----------------------------------------------------
#[derive(Debug, Default)]
pub struct LeastConnections {
    backends: Vec<Backend>,
}

impl LeastConnections {
    pub fn new(backends: Vec<Backend>) -> Self {
        Self { backends }
    }
}

impl LoadBalancer for LeastConnections {
    fn next(&self, _ctx: &LoadBalancerContext) -> Option<Backend> {
        self.backends.iter()
            .min_by_key(|b| b.active_connections.load(Ordering::Relaxed))
            .cloned()
    }

    fn add_backend(&mut self, backend: Backend) {
        self.backends.push(backend);
    }

    fn backends(&self) -> Vec<Backend> {
        self.backends.clone()
    }
}

// ----------------------------------------------------
// 3. Consistent Hash (Ketama Ring)
// ----------------------------------------------------
#[derive(Debug)]
pub struct ConsistentHash {
    backends: Vec<Backend>,
    ring: BTreeMap<u64, usize>, // hash -> backend index
    virtual_nodes: usize,
}

impl ConsistentHash {
    pub fn new(backends: Vec<Backend>, virtual_nodes: usize) -> Self {
        let mut slf = Self {
            backends: Vec::new(),
            ring: BTreeMap::new(),
            virtual_nodes,
        };
        for backend in backends {
            slf.add_backend(backend);
        }
        slf
    }

    fn rebuild_ring(&mut self) {
        self.ring.clear();
        for (i, backend) in self.backends.iter().enumerate() {
            // Apply weight to replica count
            let replicas = self.virtual_nodes * backend.weight as usize;
            for r in 0..replicas {
                let key = format!("{}:{}", backend.url, r);
                let hash = xxh64(key.as_bytes(), 0);
                self.ring.insert(hash, i);
            }
        }
    }
}

impl LoadBalancer for ConsistentHash {
    fn next(&self, ctx: &LoadBalancerContext) -> Option<Backend> {
        if self.backends.is_empty() || self.ring.is_empty() { return None; }
        
        let hash = match &ctx.hash_key {
            Some(key) => xxh64(key.as_bytes(), 0),
            None => return Some(self.backends[0].clone()), // Fallback
        };

        // Find the first virtual node >= hash
        let mut iter = self.ring.range(hash..);
        let idx = match iter.next() {
            Some((_, &idx)) => idx,
            None => {
                // Wrap around to the start of the ring
                let (_, &idx) = self.ring.iter().next().unwrap();
                idx
            }
        };

        Some(self.backends[idx].clone())
    }

    fn add_backend(&mut self, backend: Backend) {
        self.backends.push(backend);
        self.rebuild_ring();
    }

    fn backends(&self) -> Vec<Backend> {
        self.backends.clone()
    }
}

// ----------------------------------------------------
// Retro-compat: UpstreamGroup
// ----------------------------------------------------
#[derive(Debug)]
pub struct UpstreamGroup {
    pub name: String,
    pub strategy: Arc<Mutex<Box<dyn LoadBalancer>>>,
}

// Implement Clone to cleanly pass around `UpstreamGroup`
impl Clone for UpstreamGroup {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            strategy: self.strategy.clone(),
        }
    }
}

impl UpstreamGroup {
    pub fn new(name: &str, backends: Vec<Backend>) -> Self {
        // Default to Weighted Round Robin
        let strategy = Box::new(WeightedRoundRobin::new(backends));
        Self {
            name: name.to_string(),
            strategy: Arc::new(Mutex::new(strategy)),
        }
    }

    pub fn with_strategy(name: &str, strategy: Box<dyn LoadBalancer>) -> Self {
        Self {
            name: name.to_string(),
            strategy: Arc::new(Mutex::new(strategy)),
        }
    }

    pub fn add_backend(&self, backend: Backend) {
        self.strategy.lock().unwrap().add_backend(backend);
    }

    pub fn next(&self) -> Option<Backend> {
        self.next_with_ctx(&LoadBalancerContext { hash_key: None })
    }

    pub fn next_with_ctx(&self, ctx: &LoadBalancerContext) -> Option<Backend> {
        self.strategy.lock().unwrap().next(ctx)
    }
    
    pub fn backends(&self) -> Vec<Backend> {
        self.strategy.lock().unwrap().backends()
    }
}
