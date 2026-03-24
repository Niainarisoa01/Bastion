use bastion_core::health::{BackendHealth, HealthConfig};
use bastion_core::loadbalancer::{Backend, UpstreamGroup};
use std::time::Duration;

#[test]
fn test_health_state_transitions() {
    let mut config = HealthConfig::default();
    config.unhealthy_threshold = 2;
    config.healthy_threshold = 2;

    let health = BackendHealth::new(config);
    assert!(health.is_healthy(), "Initial state should be healthy");

    // 1st failure, should still be healthy
    health.record_failure();
    assert!(health.is_healthy());

    // 2nd failure, crosses threshold, should become unhealthy
    health.record_failure();
    assert!(!health.is_healthy());

    // 1st success, should still be unhealthy
    health.record_success();
    assert!(!health.is_healthy());

    // 2nd success, crosses threshold, should become healthy
    health.record_success();
    assert!(health.is_healthy());
}

#[tokio::test]
async fn test_upstream_group_health_skipping() {
    let b1 = Backend::new("http://a", 1);
    let b2 = Backend::new("http://b", 1);
    
    let group = UpstreamGroup::new("test", vec![b1, b2]);
    let backends = group.backends();

    // Kill node B
    backends[1].health.record_failure();
    backends[1].health.record_failure();
    backends[1].health.record_failure();
    
    // Now any next() should return node A only
    for _ in 0..10 {
        let b = group.next().unwrap();
        assert_eq!(b.url, "http://a");
    }
}

#[tokio::test]
async fn test_graceful_draining() {
    let b1 = Backend::new("http://a", 1);
    let b2 = Backend::new("http://b", 1);
    
    let group = UpstreamGroup::new("test", vec![b1, b2]);
    
    group.drain_backend("http://b");
    
    // B should be skipped
    for _ in 0..10 {
        let b = group.next().unwrap();
        assert_eq!(b.url, "http://a");
    }
}
