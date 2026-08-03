//! Deterministic reconnect delays for Hyperliquid sessions.
//!
//! Feed timing is not part of engine determinism, but a fixed jitter sequence
//! keeps tests repeatable and avoids reconnecting every desktop client in
//! lockstep after an API-server restart.

use std::time::Duration;

/// Exponential equal-jitter backoff with a fixed upper bound.
#[derive(Debug, Clone)]
pub struct Backoff {
    base_ms: u64,
    max_ms: u64,
    attempt: u32,
    rng: u64,
}

impl Backoff {
    /// Create a schedule from `base` to `max`, seeded reproducibly.
    #[must_use]
    pub fn new(base: Duration, max: Duration, seed: u64) -> Self {
        Self {
            base_ms: base.as_millis().max(1) as u64,
            max_ms: max.as_millis().max(1) as u64,
            attempt: 0,
            rng: seed | 1,
        }
    }

    /// Defaults suitable for a public real-time venue.
    #[must_use]
    pub fn for_feed(seed: u64) -> Self {
        Self::new(Duration::from_millis(500), Duration::from_secs(30), seed)
    }

    /// Return the next delay and advance the schedule.
    pub fn next_delay(&mut self) -> Duration {
        let factor = 1_u64.checked_shl(self.attempt.min(31)).unwrap_or(u64::MAX);
        let capped = self.base_ms.saturating_mul(factor).min(self.max_ms);
        let half = capped / 2;
        let jitter = (half as f64 * self.next_random_unit()) as u64;
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_millis(half + jitter)
    }

    /// Reset after a session delivered authoritative data.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// Delays issued since the last reset.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    fn next_random_unit(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 11) as f64 / (1_u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_grow_within_the_cap_and_reset() {
        let mut backoff = Backoff::new(Duration::from_millis(100), Duration::from_millis(800), 42);
        let first = backoff.next_delay();
        assert!((50..=100).contains(&(first.as_millis() as u64)));
        for _ in 0..8 {
            assert!(backoff.next_delay() <= Duration::from_millis(800));
        }
        assert_eq!(backoff.attempt(), 9);
        backoff.reset();
        assert_eq!(backoff.attempt(), 0);
    }

    #[test]
    fn equal_seeds_produce_equal_schedules() {
        let sequence = |seed| {
            let mut backoff = Backoff::for_feed(seed);
            (0..8).map(|_| backoff.next_delay()).collect::<Vec<_>>()
        };
        assert_eq!(sequence(7), sequence(7));
    }
}
