//! Gap, out-of-order, duplicate and backwards-timestamp detection.
//!
//! A long-running live feed can drop trades, deliver them out of order, or
//! repeat them across a reconnect. The data-honesty rule says such holes must be
//! detected and labelled, never silently patched. [`ContinuityTracker`] watches
//! the `agg_id` and timestamp sequence and reports every anomaly — both as a
//! returned value (so a consumer can label the data) and as a structured,
//! labelled `tracing` event (so a log excerpt alone explains what happened).
//!
//! This is deterministic and does not run inside the engine: the engine still
//! only ever sees ordered `Trade`s and emits nothing.

use tracing::warn;

use quantick_engine::Trade;

/// A break in the expected trade sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anomaly {
    /// One or more aggregate trades between `expected_agg_id` and `got_agg_id`
    /// were never delivered.
    Gap {
        /// The agg_id we expected next (last seen + 1).
        expected_agg_id: u64,
        /// The agg_id we actually got.
        got_agg_id: u64,
        /// How many ids were skipped.
        missing: u64,
    },
    /// A trade arrived with an agg_id lower than one already seen.
    OutOfOrder {
        /// The highest agg_id seen so far.
        last_agg_id: u64,
        /// The (lower) agg_id that just arrived.
        got_agg_id: u64,
    },
    /// The same agg_id arrived twice.
    Duplicate {
        /// The repeated agg_id.
        agg_id: u64,
    },
    /// A trade's timestamp went backwards relative to the previous trade.
    BackwardsTimestamp {
        /// The previous trade's timestamp (epoch ms).
        last_ms: i64,
        /// The (earlier) timestamp that just arrived.
        got_ms: i64,
    },
}

impl Anomaly {
    /// Emit a structured, labelled `tracing` warning for this anomaly.
    fn trace(self) {
        match self {
            Anomaly::Gap {
                expected_agg_id,
                got_agg_id,
                missing,
            } => warn!(
                target: "quantick::feed",
                kind = "gap",
                expected_agg_id,
                got_agg_id,
                missing,
                "aggTrade gap: {missing} trade(s) missing between {expected_agg_id} and {got_agg_id}"
            ),
            Anomaly::OutOfOrder {
                last_agg_id,
                got_agg_id,
            } => warn!(
                target: "quantick::feed",
                kind = "out_of_order",
                last_agg_id,
                got_agg_id,
                "aggTrade out of order: {got_agg_id} arrived after {last_agg_id}"
            ),
            Anomaly::Duplicate { agg_id } => warn!(
                target: "quantick::feed",
                kind = "duplicate",
                agg_id,
                "aggTrade duplicate: {agg_id} seen again"
            ),
            Anomaly::BackwardsTimestamp { last_ms, got_ms } => warn!(
                target: "quantick::feed",
                kind = "backwards_timestamp",
                last_ms,
                got_ms,
                "aggTrade timestamp went backwards: {got_ms} after {last_ms}"
            ),
        }
    }
}

/// Tracks trade-sequence continuity across a (possibly reconnecting) stream.
///
/// State is intentionally minimal — the highest agg_id and the last timestamp —
/// so it can be owned by a reconnect loop and detect gaps that span a dropped
/// connection.
#[derive(Debug, Default, Clone)]
pub struct ContinuityTracker {
    highest_agg_id: Option<u64>,
    last_timestamp_ms: Option<i64>,
}

impl ContinuityTracker {
    /// A fresh tracker with no history.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe one trade, returning any anomalies it introduced.
    ///
    /// Each returned anomaly is also emitted as a structured `tracing` warning.
    /// The trade is *not* dropped — detection labels the hole, it doesn't patch
    /// it; the caller still forwards the trade.
    pub fn observe(&mut self, trade: &Trade) -> Vec<Anomaly> {
        let mut anomalies = Vec::new();

        if let Some(highest) = self.highest_agg_id {
            if trade.agg_id == highest {
                anomalies.push(Anomaly::Duplicate {
                    agg_id: trade.agg_id,
                });
            } else if trade.agg_id < highest {
                anomalies.push(Anomaly::OutOfOrder {
                    last_agg_id: highest,
                    got_agg_id: trade.agg_id,
                });
            } else if trade.agg_id > highest + 1 {
                anomalies.push(Anomaly::Gap {
                    expected_agg_id: highest + 1,
                    got_agg_id: trade.agg_id,
                    missing: trade.agg_id - highest - 1,
                });
            }
        }

        if let Some(last_ms) = self.last_timestamp_ms
            && trade.timestamp_ms < last_ms
        {
            anomalies.push(Anomaly::BackwardsTimestamp {
                last_ms,
                got_ms: trade.timestamp_ms,
            });
        }

        for anomaly in &anomalies {
            anomaly.trace();
        }

        // Track the highest agg_id (so a later reorder doesn't fabricate gaps)
        // and the actual last timestamp.
        self.highest_agg_id = Some(
            self.highest_agg_id
                .map_or(trade.agg_id, |h| h.max(trade.agg_id)),
        );
        self.last_timestamp_ms = Some(trade.timestamp_ms);

        anomalies
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use rust_decimal::Decimal;

    fn trade(agg_id: u64, ts: i64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: ts,
            price: Decimal::ONE,
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    #[test]
    fn contiguous_ids_have_no_anomaly() {
        let mut t = ContinuityTracker::new();
        assert!(t.observe(&trade(1, 10)).is_empty());
        assert!(t.observe(&trade(2, 20)).is_empty());
        assert!(t.observe(&trade(3, 30)).is_empty());
    }

    #[test]
    fn detects_a_gap() {
        let mut t = ContinuityTracker::new();
        t.observe(&trade(1, 10));
        let a = t.observe(&trade(5, 20));
        assert_eq!(
            a,
            vec![Anomaly::Gap {
                expected_agg_id: 2,
                got_agg_id: 5,
                missing: 3
            }]
        );
    }

    #[test]
    fn detects_duplicate_and_out_of_order() {
        let mut t = ContinuityTracker::new();
        t.observe(&trade(10, 100));
        assert_eq!(
            t.observe(&trade(10, 100)),
            vec![Anomaly::Duplicate { agg_id: 10 }]
        );
        // A lower id after 10 is out of order (and its earlier timestamp is
        // flagged too).
        let a = t.observe(&trade(7, 90));
        assert!(a.contains(&Anomaly::OutOfOrder {
            last_agg_id: 10,
            got_agg_id: 7
        }));
        assert!(a.contains(&Anomaly::BackwardsTimestamp {
            last_ms: 100,
            got_ms: 90
        }));
    }

    #[test]
    fn a_reorder_does_not_fabricate_a_later_gap() {
        // 1, 3 (gap of 2), then 2 (out of order). The next contiguous id after
        // the highest (3) is 4 — arriving 4 must be clean, not a phantom gap.
        let mut t = ContinuityTracker::new();
        t.observe(&trade(1, 10));
        t.observe(&trade(3, 30)); // gap
        t.observe(&trade(2, 20)); // out of order + backwards ts
        assert!(
            t.observe(&trade(4, 40)).is_empty(),
            "4 follows the highest (3) contiguously"
        );
    }
}
