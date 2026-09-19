//! Sequence-continuity tracking for the bridge's synthetic `seq` ids.
//!
//! The bridge numbers ticks 1, 2, 3… per session. A hole in that sequence
//! means the transport (socket, bridge buffer) lost ticks — which, per the
//! data-honesty rule, must be detected and labelled, never papered over. This
//! mirrors the vocabulary of the Binance `ContinuityTracker`, but for
//! session-scoped synthetic ids (there is nothing to dedupe across sessions).
//!
//! Pure and deterministic: the caller decides what to do with each anomaly
//! (they are also traced here, so a log excerpt alone tells the story).

use tracing::warn;

/// A break in the bridge's tick sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeqAnomaly {
    /// One or more ticks between `expected` and `got` never arrived.
    Gap {
        /// The seq we expected next (last seen + 1).
        expected: u64,
        /// The seq that actually arrived.
        got: u64,
        /// How many ticks are missing.
        missing: u64,
    },
    /// A tick arrived with a seq at or below one already seen (bridge restart
    /// mid-connection, or a transport reorder — both worth flagging).
    NotMonotonic {
        /// The highest seq seen so far.
        last: u64,
        /// The (not higher) seq that just arrived.
        got: u64,
    },
}

/// Watches the `seq` stream of one bridge session.
#[derive(Debug, Default)]
pub struct SeqTracker {
    last: Option<u64>,
}

impl SeqTracker {
    /// A fresh tracker (one per bridge connection).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Highest observed ID in this connection, not the most recent arrival.
    #[must_use]
    pub fn highest(&self) -> Option<u64> {
        self.last
    }

    /// Observe the next seq. Returns the anomaly, if any, after tracing it.
    pub fn observe(&mut self, seq: u64) -> Option<SeqAnomaly> {
        let anomaly = match self.last {
            None => None,
            Some(last) if last.checked_add(1) == Some(seq) => None,
            Some(last) if seq > last => Some(SeqAnomaly::Gap {
                expected: last + 1,
                got: seq,
                missing: seq - last - 1,
            }),
            Some(last) => Some(SeqAnomaly::NotMonotonic { last, got: seq }),
        };
        // Track the max seen so a late lower seq doesn't shrink expectations.
        self.last = Some(self.last.map_or(seq, |l| l.max(seq)));

        match anomaly {
            Some(SeqAnomaly::Gap {
                expected,
                got,
                missing,
            }) => warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_SEQ_GAP",
                expected,
                got,
                missing,
                "bridge tick gap: {missing} tick(s) lost between seq {expected} and {got}"
            ),
            Some(SeqAnomaly::NotMonotonic { last, got }) => warn!(
                target: "quantick::feed",
                schema_version = 1_u8,
                event_code = "MT5_SEQ_NOT_MONOTONIC",
                last,
                got,
                "bridge seq went backwards: {got} after {last} (bridge restarted?)"
            ),
            None => {}
        }
        anomaly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_seqs_are_clean() {
        let mut t = SeqTracker::new();
        assert_eq!(t.observe(1), None);
        assert_eq!(t.observe(2), None);
        assert_eq!(t.observe(3), None);
    }

    #[test]
    fn maximum_sequence_does_not_wrap_or_invent_missing_messages() {
        let mut tracker = SeqTracker::new();
        assert_eq!(tracker.observe(u64::MAX), None);
        assert_eq!(
            tracker.observe(0),
            Some(SeqAnomaly::NotMonotonic {
                last: u64::MAX,
                got: 0
            })
        );
    }

    #[test]
    #[ignore = "manual hot-path benchmark; prints baseline and candidate timings"]
    fn benchmark_contiguous_mt5_delivery() {
        use crate::map::{SideMode, TickMapper};
        use crate::protocol::{BridgeMsg, parse_line};
        use std::hint::black_box;
        use std::time::Instant;
        const PRINTS: u64 = 200_000;
        const FRAME: &str = r#"{"type":"tick","seq":1,"time_ms":1784824300000,"bid":"0","ask":"0","last":"100","volume":1,"flags":1080}"#;
        let mut timings = [Vec::new(), Vec::new()];
        for round in 0..10 {
            for candidate in [round % 2 == 0, round % 2 != 0] {
                let mut tracker = SeqTracker::new();
                let mut mapper = TickMapper::new(SideMode::TickRule, -10_800);
                let mut highest_tick_ms = None;
                let start = Instant::now();
                for id in 0..PRINTS {
                    let BridgeMsg::Tick(mut tick) = parse_line(black_box(FRAME)).unwrap() else {
                        panic!("expected tick");
                    };
                    tick.seq = id;
                    if candidate {
                        let tick_ms = mapper.to_utc_ms(tick.time_ms);
                        let advances = tracker.highest().is_none_or(|highest| tick.seq > highest);
                        black_box(tracker.observe(tick.seq));
                        if advances {
                            highest_tick_ms = Some(tick_ms);
                        }
                    } else {
                        black_box(tracker.observe(tick.seq));
                    }
                    black_box(mapper.map(&tick));
                }
                black_box(highest_tick_ms);
                timings[usize::from(candidate)]
                    .push(start.elapsed().as_nanos() as f64 / PRINTS as f64);
            }
        }
        for values in &mut timings {
            values.sort_by(f64::total_cmp);
        }
        println!(
            "contiguous MT5 decode/tracker/map, {PRINTS} ticks x10: baseline_median_ns={:.2} candidate_median_ns={:.2}",
            timings[0][5], timings[1][5]
        );
        println!(
            "baseline_ns={:?}\ncandidate_ns={:?}",
            timings[0], timings[1]
        );
    }

    #[test]
    fn first_seq_needs_not_be_one() {
        // A feed attached mid-session starts wherever the bridge is.
        let mut t = SeqTracker::new();
        assert_eq!(t.observe(500), None);
        assert_eq!(t.observe(501), None);
    }

    #[test]
    fn a_gap_is_reported_with_its_size() {
        let mut t = SeqTracker::new();
        t.observe(10);
        assert_eq!(
            t.observe(14),
            Some(SeqAnomaly::Gap {
                expected: 11,
                got: 14,
                missing: 3
            })
        );
        // And the stream continues from the new position.
        assert_eq!(t.observe(15), None);
    }

    #[test]
    fn backwards_and_duplicate_seqs_are_not_monotonic() {
        let mut t = SeqTracker::new();
        t.observe(10);
        assert_eq!(
            t.observe(10),
            Some(SeqAnomaly::NotMonotonic { last: 10, got: 10 })
        );
        assert_eq!(
            t.observe(7),
            Some(SeqAnomaly::NotMonotonic { last: 10, got: 7 })
        );
        // Expectations did not shrink: 11 is still the clean successor.
        assert_eq!(t.observe(11), None);
    }
}
