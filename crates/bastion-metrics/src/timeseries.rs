use parking_lot::Mutex;
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy)]
pub struct MinuteStats {
    pub timestamp: u64,
    pub requests: u64,
    pub errors: u64,
    pub p95_latency: u64,
}

pub struct TimeSeriesStore {
    buffer: Mutex<VecDeque<MinuteStats>>,
    capacity: usize,
}

impl TimeSeriesStore {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }
    
    pub fn push(&self, stat: MinuteStats) {
        let mut b = self.buffer.lock();
        if b.len() >= self.capacity {
            b.pop_front();
        }
        b.push_back(stat);
    }
    
    pub fn get_all(&self) -> Vec<MinuteStats> {
        let b = self.buffer.lock();
        b.iter().copied().collect()
    }
}
