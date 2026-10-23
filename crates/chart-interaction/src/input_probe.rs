//! A scripted-input probe: which gesture a stress run sends next, whether the
//! chart answered it, and how the frames between were paced.
//!
//! The host injects each gesture as real pointer events and reports, every
//! frame, a fingerprint of the view the gesture should move. Answering is a
//! changed fingerprint; a gesture still unanswered after [`STALL_FRAMES`]
//! frames is an input stall. Frames are paced from the first gesture on;
//! the ones before it — the window opening, the session loading — are kept
//! apart as the opening's worst. Time is supplied, never read: the same
//! frame times and fingerprints in give the same summary out.

/// Frames a gesture may take to show on the chart and still count as
/// answered on time.
pub const ANSWER_FRAMES: u64 = 2;
/// Frames after which an unanswered gesture is written off as a stall.
pub const STALL_FRAMES: u64 = 120;
/// A frame interval at or above this is a freeze.
pub const FREEZE_MS: f32 = 250.0;
/// Width of one bucket of the frame-interval histogram, in milliseconds.
const BUCKET_MS: f32 = 0.5;
/// Buckets in the histogram; the last one holds everything longer.
const BUCKETS: usize = 20_000;

/// One gesture of the cycle, in the order it is sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// A horizontal drag through history: takes the right edge off live.
    Pan,
    /// One wheel notch: changes the bar spacing.
    Zoom,
    /// A double click on the time axis: back to the live overview.
    DoubleClick,
}

impl Gesture {
    const CYCLE: [Self; 3] = [Self::Pan, Self::Zoom, Self::DoubleClick];

    /// The name a log line carries.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Pan => "pan",
            Self::Zoom => "zoom",
            Self::DoubleClick => "double_click",
        }
    }
}

/// What one sent gesture came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Outcome {
    pub gesture: Gesture,
    /// Frames from the frame that delivered it to the frame that showed it;
    /// `None` for a stall.
    pub frames: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    gesture: Gesture,
    sent_frame: u64,
    before: u64,
}

/// The probe's running totals, readable at any point of the run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Summary {
    pub sent: u64,
    pub answered: u64,
    pub on_time: u64,
    pub stalls: u64,
    pub slowest_answer_frames: u64,
    pub frames: u64,
    pub freezes: u64,
    pub worst_ms: f32,
    pub p99_ms: f32,
    /// The longest frame before the first gesture went out.
    pub opening_worst_ms: f32,
}

/// The scripted-input schedule and its ledger.
#[derive(Debug, Clone)]
pub struct InputProbe {
    period_ms: u64,
    next_due_ms: Option<u64>,
    cycle: usize,
    pending: Option<Pending>,
    sent: u64,
    answered: u64,
    on_time: u64,
    stalls: u64,
    slowest_answer_frames: u64,
    histogram: Vec<u32>,
    frames: u64,
    freezes: u64,
    worst_ms: f32,
    opening_worst_ms: f32,
}

impl InputProbe {
    /// A probe sending one gesture every `period_ms` once the run starts.
    #[must_use]
    pub fn new(period_ms: u64) -> Self {
        Self {
            period_ms: period_ms.max(1),
            next_due_ms: None,
            cycle: 0,
            pending: None,
            sent: 0,
            answered: 0,
            on_time: 0,
            stalls: 0,
            slowest_answer_frames: 0,
            histogram: vec![0; BUCKETS],
            frames: 0,
            freezes: 0,
            worst_ms: 0.0,
            opening_worst_ms: 0.0,
        }
    }

    /// The gesture to start this frame, if one is due and none is in flight.
    /// `before` is the view fingerprint the gesture is meant to change.
    pub fn due(&mut self, now_ms: u64, frame: u64, before: u64) -> Option<Gesture> {
        if self.pending.is_some() {
            return None;
        }
        let due = *self.next_due_ms.get_or_insert(now_ms + self.period_ms);
        if now_ms < due {
            return None;
        }
        let gesture = Gesture::CYCLE[self.cycle % Gesture::CYCLE.len()];
        self.cycle += 1;
        self.sent += 1;
        self.next_due_ms = Some(now_ms + self.period_ms);
        self.pending = Some(Pending {
            gesture,
            sent_frame: frame,
            before,
        });
        Some(gesture)
    }

    /// The gesture's last event went out on `frame`: answering is counted
    /// from here, so a drag spread over frames is not charged for its own
    /// length.
    pub fn delivered(&mut self, frame: u64) {
        if let Some(pending) = self.pending.as_mut() {
            pending.sent_frame = frame;
        }
    }

    /// The fingerprint after `frame` was drawn. Returns the gesture's outcome
    /// on the frame it is decided.
    pub fn observe(&mut self, frame: u64, after: u64) -> Option<Outcome> {
        let pending = self.pending?;
        let waited = frame.saturating_sub(pending.sent_frame);
        if after != pending.before {
            self.pending = None;
            self.answered += 1;
            if waited <= ANSWER_FRAMES {
                self.on_time += 1;
            }
            self.slowest_answer_frames = self.slowest_answer_frames.max(waited);
            return Some(Outcome {
                gesture: pending.gesture,
                frames: Some(waited),
            });
        }
        if waited >= STALL_FRAMES {
            self.pending = None;
            self.stalls += 1;
            return Some(Outcome {
                gesture: pending.gesture,
                frames: None,
            });
        }
        None
    }

    /// Whether a gesture is still waiting on the chart.
    #[must_use]
    pub fn in_flight(&self) -> Option<Gesture> {
        self.pending.map(|pending| pending.gesture)
    }

    /// One frame interval, in milliseconds.
    pub fn record_frame(&mut self, interval_ms: f32) {
        let interval_ms = interval_ms.max(0.0);
        if self.sent == 0 {
            self.opening_worst_ms = self.opening_worst_ms.max(interval_ms);
            return;
        }
        self.frames += 1;
        if interval_ms >= FREEZE_MS {
            self.freezes += 1;
        }
        self.worst_ms = self.worst_ms.max(interval_ms);
        let bucket = ((interval_ms / BUCKET_MS) as usize).min(BUCKETS - 1);
        self.histogram[bucket] += 1;
    }

    /// The totals so far.
    #[must_use]
    pub fn summary(&self) -> Summary {
        Summary {
            sent: self.sent,
            answered: self.answered,
            on_time: self.on_time,
            stalls: self.stalls,
            slowest_answer_frames: self.slowest_answer_frames,
            frames: self.frames,
            freezes: self.freezes,
            worst_ms: self.worst_ms,
            p99_ms: self.percentile_ms(0.99),
            opening_worst_ms: self.opening_worst_ms,
        }
    }

    /// The upper edge of the bucket holding the `q` quantile of intervals.
    fn percentile_ms(&self, q: f64) -> f32 {
        if self.frames == 0 {
            return 0.0;
        }
        let rank = ((self.frames as f64) * q).ceil().max(1.0) as u64;
        let mut seen = 0_u64;
        for (index, count) in self.histogram.iter().enumerate() {
            seen += u64::from(*count);
            if seen >= rank {
                return (index + 1) as f32 * BUCKET_MS;
            }
        }
        self.worst_ms
    }
}

#[cfg(test)]
mod input_probe_tests {
    use super::*;

    #[test]
    fn gestures_cycle_on_the_period_and_wait_for_the_one_in_flight() {
        let mut probe = InputProbe::new(1_000);
        assert_eq!(probe.due(0, 0, 7), None, "the first is a period away");
        assert_eq!(probe.due(999, 1, 7), None);
        assert_eq!(probe.due(1_000, 2, 7), Some(Gesture::Pan));
        assert_eq!(probe.due(5_000, 3, 7), None, "one gesture at a time");
        assert_eq!(
            probe.observe(4, 8),
            Some(Outcome {
                gesture: Gesture::Pan,
                frames: Some(2)
            })
        );
        assert_eq!(probe.due(5_000, 5, 8), Some(Gesture::Zoom));
        probe.observe(5, 9);
        assert_eq!(probe.due(6_000, 6, 9), Some(Gesture::DoubleClick));
        probe.observe(7, 10);
        assert_eq!(probe.due(7_000, 8, 10), Some(Gesture::Pan));
    }

    #[test]
    fn a_late_answer_is_answered_but_not_on_time_and_silence_is_a_stall() {
        let mut probe = InputProbe::new(10);
        assert_eq!(probe.due(0, 0, 1), None);
        assert_eq!(probe.due(10, 0, 1), Some(Gesture::Pan));
        probe.delivered(2);
        assert_eq!(probe.observe(4, 1), None);
        assert!(probe.observe(5, 2).is_some(), "three frames after delivery");
        probe.due(100, 6, 2);
        assert_eq!(probe.observe(6 + STALL_FRAMES - 1, 2), None);
        assert_eq!(
            probe.observe(6 + STALL_FRAMES, 2),
            Some(Outcome {
                gesture: Gesture::Zoom,
                frames: None
            })
        );
        let summary = probe.summary();
        assert_eq!(
            (
                summary.sent,
                summary.answered,
                summary.on_time,
                summary.stalls
            ),
            (2, 1, 0, 1)
        );
        assert_eq!(summary.slowest_answer_frames, 3);
    }

    #[test]
    fn frame_percentiles_count_freezes_and_the_tail() {
        let mut probe = InputProbe::new(1_000);
        probe.record_frame(900.0);
        assert_eq!(probe.due(0, 0, 0), None);
        assert_eq!(probe.due(1_000, 1, 0), Some(Gesture::Pan));
        for _ in 0..990 {
            probe.record_frame(16.6);
        }
        for _ in 0..9 {
            probe.record_frame(40.2);
        }
        probe.record_frame(300.0);
        let summary = probe.summary();
        assert_eq!(summary.frames, 1_000);
        assert_eq!(summary.freezes, 1);
        assert_eq!(summary.worst_ms, 300.0);
        assert_eq!(summary.opening_worst_ms, 900.0, "kept apart from the run");
        assert_eq!(
            summary.p99_ms, 17.0,
            "990 of 1000 sit in the 16.5-17 bucket"
        );
        probe.record_frame(40.2);
        assert_eq!(probe.summary().p99_ms, 40.5);
    }
}
