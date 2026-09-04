//! Where real time enters quantick.
//!
//! This crate is the feed *host* — the level of the graph that owns runtimes,
//! sockets and threads — so it is also the level that owns the clock. Nothing
//! below it reads one: the engine is *told* what time it is by the trades it
//! receives, which is what keeps one fixture producing one set of bars.
//!
//! `quantick-app`'s `metrics` module re-exports [`wall_clock_ms`], so every
//! caller above still reaches it by one name.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current wall-clock time in epoch milliseconds. Never the engine's.
///
/// Everything that genuinely needs "now" — a latency observation, the span a
/// candle request covers — comes through here.
///
/// Saturates rather than wrapping. `as i64` on a `u128` past the i64 range
/// would quietly produce a *negative* timestamp, and a clock that wrong should
/// read as the end of time, not as 1969.
#[must_use]
pub fn wall_clock_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}
