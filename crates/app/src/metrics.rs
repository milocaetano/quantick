//! Performance metrics: frame times, FPS and feed lag.
//!
//! Tick charts update at high frequency, so the app must stay smooth under live
//! bursts. This module holds the pure, unit-tested metric math (rolling frame
//! stats, lag, threshold checks); the app owns the clocks and the tracing that
//! surfaces it. The engine is never involved — it stays log-free and
//! deterministic.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

/// A frame slower than this (≈ 50 FPS) is flagged as a hitch.
pub const SLOW_FRAME_MS: f32 = 20.0;

/// Feed lag beyond this many milliseconds is flagged (exchange → screen).
pub const HIGH_LAG_MS: i64 = 5_000;

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

/// Current wall-clock time in epoch milliseconds (app-only; never the engine).
#[must_use]
pub fn wall_clock_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Feed lag: how far behind the latest trade's timestamp we are right now.
///
/// `None` until a trade has been seen. Can be slightly negative if the local
/// clock is behind the exchange's — reported as-is (honest), not clamped.
#[must_use]
pub fn feed_lag_ms(now_ms: i64, latest_trade_ms: Option<i64>) -> Option<i64> {
    latest_trade_ms.map(|t| now_ms - t)
}

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

    #[test]
    fn lag_is_now_minus_trade_time() {
        assert_eq!(feed_lag_ms(1_000, Some(600)), Some(400));
        assert_eq!(feed_lag_ms(1_000, None), None);
        // Local clock behind the exchange: negative lag, reported honestly.
        assert_eq!(feed_lag_ms(500, Some(600)), Some(-100));
    }
}
