use criterion::{black_box, criterion_group, criterion_main, Criterion};
use bastion_cache::ShardedLruCache;
use bytes::Bytes;
use std::sync::Arc;
use std::thread;

fn bench_cache_lookup_hit(c: &mut Criterion) {
    let cache = ShardedLruCache::new(1024, 100, 0);
    cache.put("test_key".to_string(), Bytes::from("bench_data"), None);

    c.bench_function("cache_lookup/hit", |b| {
        b.iter(|| {
            black_box(cache.get("test_key"));
        });
    });
}

fn bench_cache_lookup_miss(c: &mut Criterion) {
    let cache = ShardedLruCache::new(1024, 100, 0);
    
    c.bench_function("cache_lookup/miss", |b| {
        b.iter(|| {
            black_box(cache.get("missing_key"));
        });
    });
}

fn bench_cache_insert(c: &mut Criterion) {
    let cache = ShardedLruCache::new(1024, 100, 0);
    let val = Bytes::from("bench_data");
    
    c.bench_function("cache_insert", |b| {
        b.iter(|| {
            // Note: modifying state in bench will trigger LRU evictions continuously
            cache.put(black_box("test_key".to_string()), black_box(val.clone()), None);
        });
    });
}

fn bench_cache_concurrent_rw(c: &mut Criterion) {
    let cache = Arc::new(ShardedLruCache::new(1024, 1000, 0));
    
    c.bench_function("cache_concurrent_rw", |b| {
        b.iter(|| {
            let mut threads = vec![];
            for i in 0..16 {
                let cache_clone = Arc::clone(&cache);
                threads.push(thread::spawn(move || {
                    let key = format!("key_{}", i % 100);
                    if i % 2 == 0 {
                        cache_clone.put(key, Bytes::from("data"), None);
                    } else {
                        black_box(cache_clone.get(&key));
                    }
                }));
            }
            for t in threads {
                t.join().unwrap();
            }
        });
    });
}

criterion_group!(benches, bench_cache_lookup_hit, bench_cache_lookup_miss, bench_cache_insert, bench_cache_concurrent_rw);
criterion_main!(benches);
