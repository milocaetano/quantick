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

    /// Observe the next seq. Returns the anomaly, if any, after tracing it.
    pub fn observe(&mut self, seq: u64) -> Option<SeqAnomaly> {
        let anomaly = match self.last {
            None => None,
            Some(last) if seq == last + 1 => None,
            Some(last) if seq > last + 1 => Some(SeqAnomaly::Gap {
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
