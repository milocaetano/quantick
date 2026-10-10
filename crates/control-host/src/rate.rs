//! One token bucket for every per-client budget: the request rate and the
//! notification budget. Time is passed in; the bucket never reads a clock.

use std::time::{Duration, Instant};

use quantick_control::limits::{
    CONTROL_CLIENT_BURST, CONTROL_CLIENT_RATE_PER_SECOND, CONTROL_NOTIFICATION_BURST,
    CONTROL_NOTIFICATION_RATE_PER_MINUTE,
};

/// The notification budget is stated per minute.
const SECONDS_PER_MINUTE: u32 = 60;

/// A budget of `burst` tokens refilled at `rate` tokens per `period_seconds`,
/// counted in nanotokens so a refill never rounds a token away.
pub struct TokenBucket {
    available_token_nanos: u128,
    last_refill: Instant,
    burst: u32,
    rate: u32,
    period_seconds: u128,
}

impl TokenBucket {
    pub const ONE_TOKEN_NANOS: u128 = 1_000_000_000;

    /// One client's request rate: the ordinary limit on every call.
    pub fn client_requests(now: Instant) -> Self {
        Self::new(CONTROL_CLIENT_BURST, CONTROL_CLIENT_RATE_PER_SECOND, 1, now)
    }

    /// One client's notification budget: stricter than the request rate,
    /// because exceeding it costs a trader who cannot work, not a queue
    /// that fills.
    pub fn notifications(now: Instant) -> Self {
        Self::new(
            CONTROL_NOTIFICATION_BURST,
            CONTROL_NOTIFICATION_RATE_PER_MINUTE,
            SECONDS_PER_MINUTE,
            now,
        )
    }

    /// A full bucket as of `now`.
    pub fn new(burst: u32, rate: u32, period_seconds: u32, now: Instant) -> Self {
        assert!(rate > 0, "a token bucket needs a refill rate above zero");
        assert!(
            period_seconds > 0,
            "a token bucket needs a refill period above zero"
        );
        Self {
            available_token_nanos: u128::from(burst) * Self::ONE_TOKEN_NANOS,
            last_refill: now,
            burst,
            rate,
            period_seconds: u128::from(period_seconds),
        }
    }

    /// Whether one more call fits at `now`, refilling by elapsed time first.
    pub fn allow(&mut self, now: Instant) -> bool {
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        let capacity = u128::from(self.burst) * Self::ONE_TOKEN_NANOS;
        let refill = elapsed.as_nanos().saturating_mul(u128::from(self.rate)) / self.period_seconds;
        self.available_token_nanos = self
            .available_token_nanos
            .saturating_add(refill)
            .min(capacity);
        if self.available_token_nanos < Self::ONE_TOKEN_NANOS {
            return false;
        }
        self.available_token_nanos -= Self::ONE_TOKEN_NANOS;
        true
    }

    /// How long until one more call would be allowed.
    pub fn retry_after(&self) -> Duration {
        if self.available_token_nanos >= Self::ONE_TOKEN_NANOS {
            return Duration::ZERO;
        }
        let missing = Self::ONE_TOKEN_NANOS - self.available_token_nanos;
        let nanos = missing.saturating_mul(self.period_seconds) / u128::from(self.rate);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "refill rate")]
    fn a_zero_rate_is_refused() {
        TokenBucket::new(1, 0, 1, Instant::now());
    }

    #[test]
    #[should_panic(expected = "refill period")]
    fn a_zero_period_is_refused() {
        TokenBucket::new(1, 1, 0, Instant::now());
    }

    #[test]
    fn the_request_rate_has_a_bounded_burst_and_refills() {
        let started = Instant::now();
        let mut limiter = TokenBucket::client_requests(started);
        for _ in 0..CONTROL_CLIENT_BURST {
            assert!(limiter.allow(started));
        }
        assert!(!limiter.allow(started));
        assert!(limiter.allow(started + Duration::from_secs(1)));
    }

    /// The budget's arithmetic on a clock the test owns: two at once, then
    /// one every ten seconds, and the wait it reports between.
    #[test]
    fn the_notification_budget_refills_one_every_ten_seconds() {
        let started = Instant::now();
        let mut limiter = TokenBucket::notifications(started);
        assert!(limiter.allow(started));
        assert!(limiter.allow(started));
        assert!(!limiter.allow(started));
        assert_eq!(limiter.retry_after(), Duration::from_secs(10));
        assert!(!limiter.allow(started + Duration::from_secs(5)));
        assert_eq!(limiter.retry_after(), Duration::from_secs(5));
        assert!(limiter.allow(started + Duration::from_secs(10)));
        assert_eq!(limiter.retry_after(), Duration::from_secs(10));
    }
}
