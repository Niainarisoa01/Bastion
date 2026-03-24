use bytes::Bytes;
use std::collections::HashMap;
use std::time::Instant;

/// Array-based Doubly Linked List Node for Zero-Allocation LRU
#[derive(Debug)]
struct Node {
    key: String,
    value: Bytes,
    expires_at: Option<Instant>,
    prev: usize,
    next: usize,
}

pub struct LruShard {
    map: HashMap<String, usize>,
    nodes: Vec<Node>,
    head: usize,
    tail: usize,
    free_list: usize,
    current_bytes: usize,
    max_bytes: usize,
}

// Sentinel index for null pointers
const NULL: usize = usize::MAX;

impl LruShard {
    pub fn new(capacity: usize, max_bytes: usize) -> Self {
        let mut nodes = Vec::with_capacity(capacity);
        
        // Pre-allocate nodes and build free list
        for i in 0..capacity {
            nodes.push(Node {
                key: String::new(),
                value: Bytes::new(),
                expires_at: None,
                prev: NULL,
                next: if i + 1 < capacity { i + 1 } else { NULL },
            });
        }

        Self {
            map: HashMap::with_capacity(capacity),
            nodes,
            head: NULL,
            tail: NULL,
            free_list: 0,
            current_bytes: 0,
            max_bytes,
        }
    }

    fn remove_node(&mut self, idx: usize) {
        let prev = self.nodes[idx].prev;
        let next = self.nodes[idx].next;

        if prev != NULL {
            self.nodes[prev].next = next;
        } else {
            self.head = next;
        }

        if next != NULL {
            self.nodes[next].prev = prev;
        } else {
            self.tail = prev;
        }
    }

    fn push_front(&mut self, idx: usize) {
        self.nodes[idx].prev = NULL;
        self.nodes[idx].next = self.head;

        if self.head != NULL {
            self.nodes[self.head].prev = idx;
        }
        self.head = idx;

        if self.tail == NULL {
            self.tail = idx;
        }
    }

    fn move_to_front(&mut self, idx: usize) {
        if self.head == idx {
            return;
        }
        self.remove_node(idx);
        self.push_front(idx);
    }

    fn evict_lru(&mut self) -> Option<(String, usize)> {
        if self.tail == NULL {
            return None;
        }
        
        // Remove from tail
        let lru_idx = self.tail;
        self.remove_node(lru_idx);
        
        // Extract data to free
        let key = self.nodes[lru_idx].key.clone();
        let bytes_freed = self.nodes[lru_idx].value.len();
        self.map.remove(&key);
        self.current_bytes -= bytes_freed;
        
        // Add to free list
        self.nodes[lru_idx].value = Bytes::new();
        self.nodes[lru_idx].next = self.free_list;
        self.free_list = lru_idx;
        
        Some((key, bytes_freed))
    }

    pub fn get(&mut self, key: &str) -> Option<Bytes> {
        if let Some(&idx) = self.map.get(key) {
            // Check TTL
            if let Some(exp) = self.nodes[idx].expires_at {
                if Instant::now() > exp {
                    // Expired - lazy eviction
                    self.remove_node(idx);
                    let bytes_freed = self.nodes[idx].value.len();
                    self.map.remove(key);
                    self.current_bytes -= bytes_freed;
                    
                    self.nodes[idx].value = Bytes::new();
                    self.nodes[idx].next = self.free_list;
                    self.free_list = idx;
                    return None;
                }
            }
            
            self.move_to_front(idx);
            return Some(self.nodes[idx].value.clone());
        }
        None
    }

    pub fn put(&mut self, key: String, value: Bytes, expires_at: Option<Instant>) -> usize {
        let val_len = value.len();
        let mut evicted_count = 0;

        // Make space if over byte limit (if max_bytes > 0)
        if self.max_bytes > 0 {
            while self.current_bytes + val_len > self.max_bytes && self.tail != NULL {
                self.evict_lru();
                evicted_count += 1;
            }
        }

        if let Some(&idx) = self.map.get(&key) {
            // Update existing
            let old_len = self.nodes[idx].value.len();
            self.current_bytes = self.current_bytes + val_len - old_len;
            self.nodes[idx].value = value;
            self.nodes[idx].expires_at = expires_at;
            self.move_to_front(idx);
            return 0; // updated, not evicted
        }

        // If no free nodes, evict LRU
        if self.free_list == NULL {
            self.evict_lru();
            evicted_count += 1;
        }

        // Pop from free list
        let new_idx = self.free_list;
        self.free_list = self.nodes[new_idx].next;

        // Insert new entry
        self.nodes[new_idx].key = key.clone();
        self.nodes[new_idx].value = value;
        self.nodes[new_idx].expires_at = expires_at;
        self.map.insert(key, new_idx);
        self.current_bytes += val_len;

        self.push_front(new_idx);
        
        evicted_count
    }

    pub fn remove(&mut self, key: &str) -> bool {
        if let Some(&idx) = self.map.get(key) {
            self.remove_node(idx);
            let bytes_freed = self.nodes[idx].value.len();
            self.map.remove(key);
            self.current_bytes -= bytes_freed;
            
            self.nodes[idx].value = Bytes::new();
            self.nodes[idx].next = self.free_list;
            self.free_list = idx;
            return true;
        }
        false
    }
}
