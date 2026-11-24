use quantick_control_host::rate::TokenBucket;
use std::time::{Duration, Instant};

#[test]
fn partial_refills_accumulate_across_denied_calls() {
    let start = Instant::now();
    let mut bucket = TokenBucket::new(1, 4, 1, start);
    assert!(bucket.allow(start));
    for step in 1..=4 {
        assert!(!bucket.allow(start + Duration::from_millis(step * 50)));
        assert_eq!(bucket.retry_after(), Duration::from_millis(250 - step * 50));
    }
    assert!(bucket.allow(start + Duration::from_millis(250)));
    assert_eq!(bucket.retry_after(), Duration::from_millis(250));
}

#[test]
fn a_long_idle_period_refills_only_to_the_burst_limit() {
    let start = Instant::now();
    let mut bucket = TokenBucket::new(2, 1, 1, start);
    assert!(bucket.allow(start));
    assert!(bucket.allow(start));
    let later = start + Duration::from_secs(1000);
    assert!(bucket.allow(later));
    assert!(bucket.allow(later));
    assert!(!bucket.allow(later));
    assert_eq!(bucket.retry_after(), Duration::from_secs(1));
}

#[test]
fn retry_after_is_observational_and_does_not_spend_tokens() {
    let start = Instant::now();
    let mut bucket = TokenBucket::new(2, 1, 1, start);
    assert_eq!(bucket.retry_after(), Duration::ZERO);
    assert_eq!(bucket.retry_after(), Duration::ZERO);
    assert!(bucket.allow(start));
    assert_eq!(bucket.retry_after(), Duration::ZERO);
    assert!(bucket.allow(start));
    assert_eq!(bucket.retry_after(), Duration::from_secs(1));
    assert_eq!(bucket.retry_after(), Duration::from_secs(1));
    assert!(!bucket.allow(start));
}

#[test]
fn zero_burst_never_accumulates_capacity_even_after_a_refill() {
    let start = Instant::now();
    let mut bucket = TokenBucket::new(0, 1, 1, start);
    assert!(!bucket.allow(start));
    assert!(!bucket.allow(start + Duration::from_secs(60)));
    assert!(!bucket.allow(start + Duration::from_secs(3600)));
    assert_eq!(bucket.retry_after(), Duration::from_secs(1));
}

#[test]
fn subsecond_boundary_refuses_the_last_nanosecond_before_a_whole_token() {
    let start = Instant::now();
    let mut bucket = TokenBucket::new(1, 2, 1, start);
    assert!(bucket.allow(start));
    assert!(!bucket.allow(start + Duration::from_nanos(499_999_999)));
    assert_eq!(bucket.retry_after(), Duration::from_nanos(1));
    assert!(bucket.allow(start + Duration::from_nanos(500_000_000)));
    assert!(!bucket.allow(start + Duration::from_nanos(500_000_000)));
}

#[test]
fn independent_clients_cannot_consume_each_others_burst() {
    let start = Instant::now();
    let mut first = TokenBucket::new(1, 1, 1, start);
    let mut second = TokenBucket::new(1, 1, 1, start);
    assert!(first.allow(start));
    assert!(!first.allow(start));
    assert!(second.allow(start));
    assert!(!second.allow(start));
    assert!(first.allow(start + Duration::from_secs(1)));
    assert_eq!(second.retry_after(), Duration::from_secs(1));
    assert!(second.allow(start + Duration::from_secs(1)));
}
