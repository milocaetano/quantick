//! When a venue websocket reconnects: the backoff between attempts and the
//! read silence that counts as a dead connection.
//!
//! Shared by every websocket venue so `quantick-feed-binance` and
//! `quantick-feed-hyperliquid` keep one schedule without depending on each
//! other. Pure arithmetic over [`Duration`]: the venue's runtime does the
//! waiting.

use std::time::Duration;

/// Seconds without a single inbound frame after which a trade websocket is
/// treated as dead and reconnected: a `[[feeds]]` entry's default
/// `idle_timeout_secs`.
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 30;

/// [`DEFAULT_IDLE_TIMEOUT_SECS`] as a duration.
pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(DEFAULT_IDLE_TIMEOUT_SECS);

/// Exponential backoff with equal jitter, deterministic given a seed.
///
/// Delay for attempt `n` is `min(base * 2^n, max)`, then jittered into
/// `[capped/2, capped]`. The jitter comes from a seeded xorshift, so the delay
/// sequence is reproducible in tests (no rng dependency; feed timing is not
/// engine-deterministic, the seed only makes tests stable). Jitter avoids
/// many clients reconnecting in lockstep after a venue restart.
#[derive(Debug, Clone)]
pub struct Backoff {
    base_ms: u64,
    max_ms: u64,
    attempt: u32,
    rng: u64,
}

impl Backoff {
    /// A backoff between `base` and `max`, seeded for reproducible jitter.
    #[must_use]
    pub fn new(base: Duration, max: Duration, seed: u64) -> Self {
        Self {
            base_ms: base.as_millis().max(1) as u64,
            max_ms: max.as_millis().max(1) as u64,
            attempt: 0,
            rng: seed | 1, // xorshift must not be seeded with 0
        }
    }

    /// Defaults for a live market-data feed: 500 ms base, 30 s ceiling.
    #[must_use]
    pub fn for_feed(seed: u64) -> Self {
        Self::new(Duration::from_millis(500), Duration::from_secs(30), seed)
    }

    fn next_rand(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 11) as f64 / (1_u64 << 53) as f64
    }

    /// The delay before the next reconnect attempt, advancing the counter.
    pub fn next_delay(&mut self) -> Duration {
        let factor = 1_u64.checked_shl(self.attempt.min(31)).unwrap_or(u64::MAX);
        let capped = self.base_ms.saturating_mul(factor).min(self.max_ms);
        let half = capped / 2;
        let jitter = (half as f64 * self.next_rand()) as u64;
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_millis(half + jitter)
    }

    /// Reset the attempt counter after a connection that delivered data.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    /// The number of delays handed out since the last reset.
    #[must_use]
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_is_capped() {
        let max = Duration::from_millis(3200);
        let mut b = Backoff::new(Duration::from_millis(100), max, 12345);

        // Attempt 0: capped = 100, delay in [50, 100].
        let d0 = b.next_delay().as_millis() as u64;
        assert!((50..=100).contains(&d0), "d0 = {d0}");

        let mut delays = vec![d0];
        for _ in 0..9 {
            delays.push(b.next_delay().as_millis() as u64);
        }
        for d in &delays {
            assert!(*d <= max.as_millis() as u64, "delay {d} exceeds max");
        }
        // By later attempts the cap (3200) is hit, so delay is in [1600, 3200].
        let late = *delays.last().unwrap();
        assert!((1600..=3200).contains(&late), "late = {late}");
    }

    #[test]
    fn reset_returns_to_the_base_delay() {
        let mut b = Backoff::new(Duration::from_millis(100), Duration::from_secs(30), 7);
        for _ in 0..5 {
            let _ = b.next_delay();
        }
        assert_eq!(b.attempt(), 5);
        b.reset();
        assert_eq!(b.attempt(), 0);
        let d = b.next_delay().as_millis() as u64;
        assert!((50..=100).contains(&d), "post-reset delay = {d}");
    }

    #[test]
    fn jitter_is_deterministic_for_a_given_seed() {
        let seq = |seed| {
            let mut b = Backoff::for_feed(seed);
            (0..8).map(|_| b.next_delay()).collect::<Vec<_>>()
        };
        assert_eq!(seq(42), seq(42), "same seed => same delays");
    }
}
