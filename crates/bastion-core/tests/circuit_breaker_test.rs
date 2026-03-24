use bastion_core::middleware::circuit_breaker::{
    CircuitBreaker, CircuitBreakerConfig, CbState, CircuitBreakerRegistry,
};
use bastion_core::middleware::retry::{RetryConfig, BackoffStrategy};
use std::time::Duration;
use hyper::Method;
use std::sync::Arc;

// ═════════════════════════════════════════
//  CIRCUIT BREAKER TESTS
// ═════════════════════════════════════════

#[test]
fn test_cb_closed_allows_requests() {
    let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
    assert_eq!(cb.state(), CbState::Closed);
    assert!(cb.allow_request().is_ok());
}

#[test]
fn test_cb_opens_after_failures() {
    let config = CircuitBreakerConfig {
        failure_threshold: 3,
        ..Default::default()
    };
    let cb = CircuitBreaker::new(config);

    // 2 failures -> Still Closed
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.state(), CbState::Closed);
    assert!(cb.allow_request().is_ok());

    // 3rd failure -> trips Open
    cb.record_failure();
    assert_eq!(cb.state(), CbState::Open);
    assert_eq!(cb.allow_request(), Err(CbState::Open));
}

#[test]
fn test_cb_rejects_when_open() {
    let config = CircuitBreakerConfig {
        failure_threshold: 1,
        ..Default::default()
    };
    let cb = CircuitBreaker::new(config);

    cb.record_failure();
    assert_eq!(cb.state(), CbState::Open);
    
    // Multiple requests should all be rejected immediately
    for _ in 0..10 {
        assert_eq!(cb.allow_request(), Err(CbState::Open));
    }
}

#[tokio::test]
async fn test_cb_half_open_after_timeout() {
    let config = CircuitBreakerConfig {
        failure_threshold: 1,
        open_timeout: Duration::from_millis(100),
        half_open_max_calls: 2,
        ..Default::default()
    };
    let cb = CircuitBreaker::new(config);

    cb.record_failure();
    assert_eq!(cb.state(), CbState::Open);

    // Sleep until timeout passes
    tokio::time::sleep(Duration::from_millis(150)).await;

    // First state() call triggers transition
    assert_eq!(cb.state(), CbState::HalfOpen);

    // Allows exactly 2 probes
    assert!(cb.allow_request().is_ok());
    assert!(cb.allow_request().is_ok());
    
    // Third probe is rejected
    assert_eq!(cb.allow_request(), Err(CbState::HalfOpen));
}

#[tokio::test]
async fn test_cb_closes_after_success() {
    let config = CircuitBreakerConfig {
        failure_threshold: 1,
        open_timeout: Duration::from_millis(10),
        half_open_max_calls: 3,
        success_threshold: 2,
    };
    let cb = CircuitBreaker::new(config);

    cb.record_failure(); // Trips to Open
    tokio::time::sleep(Duration::from_millis(20)).await;

    assert_eq!(cb.state(), CbState::HalfOpen);

    // Needs 2 successes to close
    cb.record_success();
    assert_eq!(cb.state(), CbState::HalfOpen);
    
    cb.record_success();
    assert_eq!(cb.state(), CbState::Closed);
    assert_eq!(cb.failure_count(), 0); // Failure count is reset
}

#[tokio::test]
async fn test_cb_reopens_on_half_open_failure() {
    let config = CircuitBreakerConfig {
        failure_threshold: 1,
        open_timeout: Duration::from_millis(10),
        half_open_max_calls: 3,
        success_threshold: 2,
    };
    let cb = CircuitBreaker::new(config);

    cb.record_failure(); // Open
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(cb.state(), CbState::HalfOpen);

    // 1 success
    cb.record_success();
    // 1 failure immediately ruins it -> back to Open
    cb.record_failure();
    
    assert_eq!(cb.state(), CbState::Open);
}

#[test]
fn test_cb_concurrent_failures() {
    let config = CircuitBreakerConfig {
        failure_threshold: 50,
        ..Default::default()
    };
    let cb = Arc::new(CircuitBreaker::new(config));
    
    let mut handles = vec![];
    for _ in 0..100 {
        let cb_clone = Arc::clone(&cb);
        handles.push(std::thread::spawn(move || {
            cb_clone.record_failure();
        }));
    }

    for h in handles {
        h.join().unwrap();
    }

    // Should be Open since 100 > 50 failures
    assert_eq!(cb.state(), CbState::Open);
    assert!(cb.failure_count() >= 50); // Exact counts don't matter much past threshold
}

#[test]
fn test_cb_registry() {
    let registry = CircuitBreakerRegistry::new(CircuitBreakerConfig::default());
    
    let cb1 = registry.get_or_create("upstreamA");
    cb1.record_failure();
    
    let cb2 = registry.get_or_create("upstreamB");
    assert_eq!(cb2.state(), CbState::Closed);
    assert_eq!(cb2.failure_count(), 0);
}

// ═════════════════════════════════════════
//  RETRY TESTS
// ═════════════════════════════════════════

#[test]
fn test_retry_idempotent_methods() {
    assert!(RetryConfig::is_idempotent(&Method::GET));
    assert!(RetryConfig::is_idempotent(&Method::PUT));
    assert!(RetryConfig::is_idempotent(&Method::DELETE));
    assert!(RetryConfig::is_idempotent(&Method::HEAD));
    assert!(RetryConfig::is_idempotent(&Method::OPTIONS));
    
    assert!(!RetryConfig::is_idempotent(&Method::POST));
    assert!(!RetryConfig::is_idempotent(&Method::PATCH));
}

#[test]
fn test_retry_status_codes() {
    let config = RetryConfig::default();
    assert!(config.should_retry_status(502));
    assert!(config.should_retry_status(503));
    assert!(config.should_retry_status(504));
    
    assert!(!config.should_retry_status(500));
    assert!(!config.should_retry_status(404));
    assert!(!config.should_retry_status(400));
}

#[test]
fn test_retry_constant_backoff() {
    let config = RetryConfig {
        backoff: BackoffStrategy::Constant(Duration::from_millis(100)),
        jitter: false,
        ..Default::default()
    };
    
    assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
    assert_eq!(config.delay_for_attempt(1), Duration::from_millis(100));
    assert_eq!(config.delay_for_attempt(5), Duration::from_millis(100));
}

#[test]
fn test_retry_linear_backoff() {
    let config = RetryConfig {
        backoff: BackoffStrategy::Linear(Duration::from_millis(100)),
        jitter: false,
        ..Default::default()
    };
    
    assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
    assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
    assert_eq!(config.delay_for_attempt(2), Duration::from_millis(300));
}

#[test]
fn test_retry_exponential_backoff() {
    let config = RetryConfig {
        backoff: BackoffStrategy::Exponential(Duration::from_millis(100)),
        jitter: false,
        ..Default::default()
    };
    
    assert_eq!(config.delay_for_attempt(0), Duration::from_millis(100));
    assert_eq!(config.delay_for_attempt(1), Duration::from_millis(200));
    assert_eq!(config.delay_for_attempt(2), Duration::from_millis(400));
    assert_eq!(config.delay_for_attempt(3), Duration::from_millis(800));
}

#[test]
fn test_retry_jitter() {
    let config = RetryConfig {
        backoff: BackoffStrategy::Constant(Duration::from_millis(100)),
        jitter: true,  // Should introduce 0-25ms random jitter
        ..Default::default()
    };
    
    // Test 10 times to ensure it falls within range and varies
    let mut varies = false;
    let first = config.delay_for_attempt(0);
    
    for _ in 0..10 {
        let delay = config.delay_for_attempt(0);
        assert!(delay >= Duration::from_millis(100), "Delay too small: {:?}", delay);
        assert!(delay < Duration::from_millis(125), "Delay too large: {:?}", delay);
        
        if delay != first {
            varies = true;
        }
    }
    
    // Incredibly improbable that 10 random runs yield exactly the same number
    assert!(varies, "Jitter is not giving random variations");
}
