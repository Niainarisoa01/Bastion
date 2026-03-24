use bastion_core::loadbalancer::{Backend, LoadBalancerContext, WeightedRoundRobin, LeastConnections, ConsistentHash, LoadBalancer};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn test_smooth_weighted_distribution() {
    let b1 = Backend::new("http://a", 3);
    let b2 = Backend::new("http://b", 1);
    
    let balancer = WeightedRoundRobin::new(vec![b1, b2]);
    let ctx = LoadBalancerContext { hash_key: None };
    
    let mut count_a = 0;
    let mut count_b = 0;
    
    for _ in 0..4000 {
        let b = balancer.next(&ctx).unwrap();
        if b.url == "http://a" { count_a += 1 } else { count_b += 1 }
    }
    
    // Smooth weighted algorithm is exact across cycles.
    // Total weight = 4. 'a' gets 3/4, 'b' gets 1/4.
    assert_eq!(count_a, 3000);
    assert_eq!(count_b, 1000);
}

#[test]
fn test_least_connections() {
    let b1 = Backend::new("http://a", 1);
    let b2 = Backend::new("http://b", 1);
    
    // Simulate active connections
    b1.active_connections.store(10, Ordering::Relaxed);
    b2.active_connections.store(2, Ordering::Relaxed);
    
    let balancer = LeastConnections::new(vec![b1.clone(), b2.clone()]);
    let ctx = LoadBalancerContext { hash_key: None };
    
    let next = balancer.next(&ctx).unwrap();
    assert_eq!(next.url, "http://b"); // B has fewer connections
    
    // Now B spikes
    b2.active_connections.store(15, Ordering::Relaxed);
    let next2 = balancer.next(&ctx).unwrap();
    assert_eq!(next2.url, "http://a"); // Now A has fewer
}

#[test]
fn test_consistent_hashing() {
    let b1 = Backend::new("http://a", 1);
    let b2 = Backend::new("http://b", 1);
    let b3 = Backend::new("http://c", 1);
    
    let balancer = ConsistentHash::new(vec![b1, b2, b3], 100); // 100 vnodes
    
    let ctx1 = LoadBalancerContext { hash_key: Some("192.168.1.1".to_string()) };
    let ctx2 = LoadBalancerContext { hash_key: Some("10.0.0.5".to_string()) };
    
    let route1_a = balancer.next(&ctx1).unwrap().url;
    let route1_b = balancer.next(&ctx1).unwrap().url;
    assert_eq!(route1_a, route1_b, "Consistent hash should route same key to same backend");
    
    let route2_a = balancer.next(&ctx2).unwrap().url;
    let route2_b = balancer.next(&ctx2).unwrap().url;
    assert_eq!(route2_a, route2_b);
    
    // The keys are fundamentally different and usually hit different nodes (though not guaranteed 100% of time)
}
