//! The one clock port the host machinery reads time through.
//!
//! This crate is headless: it never reads the wall clock or a monotonic
//! clock itself. The application owns both and hands this crate a
//! [`HostClock`], so a test can drive a capture at a fixed instant and the
//! headless guard has nothing to find. The idempotency store takes the time
//! the other way — as a `now_unix_ms` argument on every call — because each
//! of its calls is already made by a caller that has just read the clock.

use std::time::Duration;

/// Time as the application tells it.
pub trait HostClock: Send + Sync {
    /// Milliseconds since the Unix epoch, the unit every wire timestamp uses.
    fn unix_ms(&self) -> i64;

    /// A reading of a monotonic clock since an arbitrary origin, used only to
    /// measure how long something took. Two readings are subtracted; the
    /// origin never reaches a result.
    fn monotonic(&self) -> Duration;
}
