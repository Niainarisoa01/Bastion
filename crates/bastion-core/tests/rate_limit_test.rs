use std::time::Duration;
use std::sync::Arc;
use bastion_core::middleware::rate_limit::store::{TokenBucket, SlidingWindow};

// ═══════════════════════════════════════════
//  TOKEN BUCKET TESTS
// ═══════════════════════════════════════════

#[test]
fn test_token_bucket_allows_under_limit() {
    let bucket = TokenBucket::new(5, 1.0);
    for _ in 0..5 {
        let (allowed, _) = bucket.try_acquire("user1");
        assert!(allowed);
    }
}

#[test]
fn test_token_bucket_blocks_over_limit() {
    let bucket = TokenBucket::new(3, 1.0);
    // Drain the bucket
    for _ in 0..3 {
        bucket.try_acquire("user1");
    }
    // Now it should block
    let (allowed, remaining) = bucket.try_acquire("user1");
    assert!(!allowed);
    assert_eq!(remaining, 0);
}

#[test]
fn test_token_bucket_separate_keys() {
    let bucket = TokenBucket::new(2, 1.0);
    // Drain user1
    bucket.try_acquire("user1");
    bucket.try_acquire("user1");
    let (blocked, _) = bucket.try_acquire("user1");
    assert!(!blocked);

    // user2 should still be allowed
    let (allowed, _) = bucket.try_acquire("user2");
    assert!(allowed);
}

#[test]
fn test_token_bucket_remaining_count() {
    let bucket = TokenBucket::new(5, 1.0);
    let (_, remaining) = bucket.try_acquire("user1");
    assert_eq!(remaining, 4);
    let (_, remaining) = bucket.try_acquire("user1");
    assert_eq!(remaining, 3);
    let (_, remaining) = bucket.try_acquire("user1");
    assert_eq!(remaining, 2);
}

#[test]
fn test_token_bucket_burst_capacity() {
    // burst = 10, so 10 requests should pass immediately
    let bucket = TokenBucket::new(10, 1.0);
    for i in 0..10 {
        let (allowed, _) = bucket.try_acquire("burst");
        assert!(allowed, "Request {} should be allowed", i);
    }
    let (allowed, _) = bucket.try_acquire("burst");
    assert!(!allowed, "11th request should be blocked");
}

#[tokio::test]
async fn test_token_bucket_refill() {
    // 2 tokens, refill 10/sec → should refill in ~200ms
    let bucket = TokenBucket::new(2, 10.0);
    bucket.try_acquire("refill");
    bucket.try_acquire("refill");
    let (blocked, _) = bucket.try_acquire("refill");
    assert!(!blocked);

    // Wait for refill
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (allowed, _) = bucket.try_acquire("refill");
    assert!(allowed, "Should have refilled after waiting");
}

#[tokio::test]
async fn test_token_bucket_concurrent() {
    let bucket = Arc::new(TokenBucket::new(50, 0.0)); // no refill
    let mut handles = vec![];

    for _ in 0..100 {
        let b = Arc::clone(&bucket);
        handles.push(tokio::spawn(async move {
            b.try_acquire("concurrent")
        }));
    }

    let mut allowed_count = 0;
    let mut blocked_count = 0;
    for h in handles {
        let (allowed, _) = h.await.unwrap();
        if allowed { allowed_count += 1; } else { blocked_count += 1; }
    }

    assert_eq!(allowed_count, 50, "Exactly 50 should be allowed");
    assert_eq!(blocked_count, 50, "Exactly 50 should be blocked");
}

// ═══════════════════════════════════════════
//  SLIDING WINDOW TESTS
// ═══════════════════════════════════════════

#[test]
fn test_sliding_window_allows_under_limit() {
    let window = SlidingWindow::new(5, Duration::from_secs(60));
    for _ in 0..5 {
        let (allowed, _, _) = window.try_acquire("user1");
        assert!(allowed);
    }
}

#[test]
fn test_sliding_window_blocks_over_limit() {
    let window = SlidingWindow::new(3, Duration::from_secs(60));
    for _ in 0..3 {
        window.try_acquire("user1");
    }
    let (allowed, remaining, retry_after) = window.try_acquire("user1");
    assert!(!allowed);
    assert_eq!(remaining, 0);
    assert!(retry_after.is_some());
}

#[test]
fn test_sliding_window_remaining_count() {
    let window = SlidingWindow::new(5, Duration::from_secs(60));
    let (_, remaining, _) = window.try_acquire("user1");
    assert_eq!(remaining, 4);
    let (_, remaining, _) = window.try_acquire("user1");
    assert_eq!(remaining, 3);
}

#[test]
fn test_sliding_window_separate_keys() {
    let window = SlidingWindow::new(2, Duration::from_secs(60));
    window.try_acquire("user1");
    window.try_acquire("user1");
    let (blocked, _, _) = window.try_acquire("user1");
    assert!(!blocked);

    let (allowed, _, _) = window.try_acquire("user2");
    assert!(allowed);
}

#[tokio::test]
async fn test_sliding_window_reset() {
    let window = SlidingWindow::new(2, Duration::from_millis(200));
    window.try_acquire("reset");
    window.try_acquire("reset");
    let (blocked, _, _) = window.try_acquire("reset");
    assert!(!blocked);

    // Wait for window to expire
    tokio::time::sleep(Duration::from_millis(300)).await;
    let (allowed, _, _) = window.try_acquire("reset");
    assert!(allowed, "Should be allowed after window reset");
}

#[test]
fn test_sliding_window_retry_after_present() {
    let window = SlidingWindow::new(1, Duration::from_secs(60));
    window.try_acquire("retry");
    let (_, _, retry_after) = window.try_acquire("retry");
    assert!(retry_after.is_some());
    // Should be close to 60000 ms
    let ms = retry_after.unwrap();
    assert!(ms > 50000 && ms <= 60000, "Retry-After should be ~60s, got {}ms", ms);
}

#[tokio::test]
async fn test_sliding_window_concurrent() {
    let window = Arc::new(SlidingWindow::new(50, Duration::from_secs(60)));
    let mut handles = vec![];

    for _ in 0..100 {
        let w = Arc::clone(&window);
        handles.push(tokio::spawn(async move {
            w.try_acquire("concurrent")
        }));
    }

    let mut allowed_count = 0;
    let mut blocked_count = 0;
    for h in handles {
        let (allowed, _, _) = h.await.unwrap();
        if allowed { allowed_count += 1; } else { blocked_count += 1; }
    }

    assert_eq!(allowed_count, 50, "Exactly 50 should be allowed");
    assert_eq!(blocked_count, 50, "Exactly 50 should be blocked");
}

#[test]
fn test_sliding_window_exact_boundary() {
    let window = SlidingWindow::new(1, Duration::from_secs(60));
    let (allowed, remaining, _) = window.try_acquire("boundary");
    assert!(allowed);
    assert_eq!(remaining, 0);

    let (blocked, _, _) = window.try_acquire("boundary");
    assert!(!blocked);
}
