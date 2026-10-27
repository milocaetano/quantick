//! A rolling window of frame durations: the mean, the rate and the worst.
//!
//! The app records one interval per frame and reads these back for its
//! health summary; the math reads no clock, so it lives below `app`.

use std::collections::VecDeque;

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

#[cfg(test)]
mod frame_stats_tests {
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
