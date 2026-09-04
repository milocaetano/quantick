//! Performance metrics: frame times, FPS and feed lag.
//!
//! Tick charts update at high frequency, so the app must stay smooth under live
//! bursts. This module holds the pure, unit-tested metric math (rolling frame
//! stats, lag, threshold checks); the app owns the clocks and the tracing that
//! surfaces it. The engine is never involved — it stays log-free and
//! deterministic.

use std::collections::VecDeque;

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

/// A rolling window of recent frame durations (milliseconds).
#[derive(Debug)]
pub struct FrameStats {
    samples: VecDeque<f32>,
    capacity: usize,
}

impl FrameStats {
    /// A window holding the last `capacity` frame durations.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Record one frame's duration in milliseconds, evicting the oldest.
    pub fn record(&mut self, frame_ms: f32) {
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(frame_ms);
    }

    /// Mean frame time over the window, or `None` if no frames yet.
    #[must_use]
    pub fn avg_ms(&self) -> Option<f32> {
        if self.samples.is_empty() {
            return None;
        }
        Some(self.samples.iter().sum::<f32>() / self.samples.len() as f32)
    }

    /// Rolling FPS derived from the mean frame time.
    #[must_use]
    pub fn fps(&self) -> Option<f32> {
        self.avg_ms()
            .map(|ms| if ms > 0.0 { 1000.0 / ms } else { f32::INFINITY })
    }

    /// Worst (longest) frame in the window.
    #[must_use]
    pub fn worst_ms(&self) -> Option<f32> {
        self.samples
            .iter()
            .copied()
            .fold(None, |acc, x| Some(acc.map_or(x, |a: f32| a.max(x))))
    }
}

// Lives with the feed host, the level of the graph that owns runtimes and so
// the level where real time enters quantick; re-exported so every caller here
// keeps one name for it. Nothing below that crate reads a clock at all.
pub use quantick_feed::clock::wall_clock_ms;

// Lives with the order-flow engine, which observes the same lag on depth
// events; re-exported so the tape's callers keep one name for it.
pub use quantick_orderflow::feed_lag_ms;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stats_report_none() {
        let s = FrameStats::new(8);
        assert!(s.avg_ms().is_none());
        assert!(s.fps().is_none());
        assert!(s.worst_ms().is_none());
    }

    #[test]
    fn avg_fps_and_worst() {
        let mut s = FrameStats::new(8);
        s.record(10.0);
        s.record(20.0);
        s.record(30.0);
        assert!((s.avg_ms().unwrap() - 20.0).abs() < 0.001);
        assert!((s.fps().unwrap() - 50.0).abs() < 0.001); // 1000 / 20
        assert!((s.worst_ms().unwrap() - 30.0).abs() < 0.001);
    }

    #[test]
    fn window_evicts_oldest() {
        let mut s = FrameStats::new(2);
        s.record(100.0);
        s.record(10.0);
        s.record(20.0); // evicts 100
        assert!((s.avg_ms().unwrap() - 15.0).abs() < 0.001);
        assert!((s.worst_ms().unwrap() - 20.0).abs() < 0.001);
    }
}
