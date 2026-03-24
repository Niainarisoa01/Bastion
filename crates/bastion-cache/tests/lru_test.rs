use bastion_cache::{ShardedLruCache, LruShard};
use bytes::Bytes;
use std::time::{Duration, Instant};

#[test]
fn test_lru_shard_put_get() {
    let mut shard = LruShard::new(5, 0);
    shard.put("k1".to_string(), Bytes::from("v1"), None);
    assert_eq!(shard.get("k1"), Some(Bytes::from("v1")));
    assert_eq!(shard.get("k2"), None);
}

#[test]
fn test_lru_eviction() {
    // Capacity 3
    let mut shard = LruShard::new(3, 0);
    shard.put("k1".to_string(), Bytes::from("v1"), None);
    shard.put("k2".to_string(), Bytes::from("v2"), None);
    shard.put("k3".to_string(), Bytes::from("v3"), None);
    
    // Cache is full. k1 is LRU.
    // Insert k4, should evict k1.
    shard.put("k4".to_string(), Bytes::from("v4"), None);
    assert_eq!(shard.get("k1"), None);
    assert_eq!(shard.get("k2"), Some(Bytes::from("v2")));
    
    // Access k2 moves it to MRU. LRU is now k3.
    // Insert k5, should evict k3.
    shard.put("k5".to_string(), Bytes::from("v5"), None);
    assert_eq!(shard.get("k3"), None);
    assert_eq!(shard.get("k2"), Some(Bytes::from("v2")));
    assert_eq!(shard.get("k4"), Some(Bytes::from("v4")));
    assert_eq!(shard.get("k5"), Some(Bytes::from("v5")));
}

#[test]
fn test_ttl_eviction() {
    let mut shard = LruShard::new(5, 0);
    shard.put("k1".to_string(), Bytes::from("v1"), Some(Instant::now() - Duration::from_secs(1)));
    
    // Should return None because it is expired
    assert_eq!(shard.get("k1"), None);
}

#[test]
fn test_sharded_cache() {
    let cache = ShardedLruCache::new(8, 10, 0);
    cache.put("user:1".to_string(), Bytes::from("data1"), None);
    cache.put("user:2".to_string(), Bytes::from("data2"), None);
    
    assert_eq!(cache.get("user:1"), Some(Bytes::from("data1")));
    assert_eq!(cache.get("user:2"), Some(Bytes::from("data2")));
    assert_eq!(cache.get("user:3"), None);
    
    assert!(cache.remove("user:1"));
    assert_eq!(cache.get("user:1"), None);
}

#[test]
fn test_max_bytes_eviction() {
    // Max 10 bytes
    let mut shard = LruShard::new(10, 10);
    
    // 4 bytes
    shard.put("k1".to_string(), Bytes::from("abcd"), None);
    // 4 bytes (total 8)
    shard.put("k2".to_string(), Bytes::from("efgh"), None);
    // 4 bytes (total 12 -> evicts k1)
    shard.put("k3".to_string(), Bytes::from("ijkl"), None);
    
    assert_eq!(shard.get("k1"), None);
    assert_eq!(shard.get("k3"), Some(Bytes::from("ijkl")));
}
