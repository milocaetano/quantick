//! Performance metrics: frame times, FPS and feed lag.
//!
//! Tick charts update at high frequency, so the app must stay smooth under live
//! bursts. This module holds the pure, unit-tested metric math (rolling frame
//! stats, lag, threshold checks); the app owns the clocks and the tracing that
//! surfaces it. The engine is never involved — it stays log-free and
//! deterministic.

/// A frame slower than this (≈ 50 FPS) is flagged as a hitch.
pub const SLOW_FRAME_MS: f32 = 20.0;

/// Feed lag beyond this many milliseconds is flagged (exchange → screen).
pub const HIGH_LAG_MS: i64 = 5_000;

/// A tape whose newest event is older than this reads as stale.
///
/// Not the same measurement as [`HIGH_LAG_MS`]: that one is how late a print
/// was when it arrived, an observation that stops ageing once prints stop.
/// This one is wall clock minus the newest event's timestamp, so it is what
/// catches a socket that stays open and delivers nothing — no error, no
/// disconnect, and a healthy-looking arrival figure frozen on screen. Ten
/// seconds is long enough that a quiet minute on a thin instrument does not
/// cry wolf, short enough that a wedged connection is visible.
pub const STALE_TAPE_MS: i64 = 10_000;

// The rolling frame window is pure math, so it lives headless beside the
// input probe that paces a stress run; re-exported so callers keep its name.
pub use quantick_chart_interaction::frame_stats::FrameStats;

// Lives with the feed host, the level of the graph that owns runtimes and so
// the level where real time enters quantick; re-exported so every caller here
// keeps one name for it. Nothing below that crate reads a clock at all.
pub use quantick_feed::clock::wall_clock_ms;

// Lives with the order-flow engine, which observes the same lag on depth
// events; re-exported so the tape's callers keep one name for it.
pub use quantick_orderflow::feed_lag_ms;
