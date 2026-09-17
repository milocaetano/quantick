//! Ordered, provider-neutral evidence of incomplete feed delivery.

use crate::FeedGap;
use quantick_engine::Trade;
use quantick_feed_mt5::SeqAnomaly;

#[cfg(test)]
mod tests;

/// A source-message anomaly, never a fabricated trade or a fill estimate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeedContinuity {
    /// Market-time bounds when both sides are known and ordered. Equal
    /// timestamps can still bracket missing messages.
    pub gap: Option<FeedGap>,
    /// Known source messages absent from the usable tape (Binance aggregates,
    /// MT5 wire ticks, or individually rejected source rows). MT5 ticks can be
    /// quotes, so this is not a count of lost trades.
    /// `None` means an interruption with unknown loss.
    pub missing_messages: Option<u64>,
    /// A repeated/backwards source ID, separate from a missing interval.
    pub non_monotonic: bool,
}

impl FeedContinuity {
    pub(crate) fn malformed_rows(count: u64) -> Self {
        Self {
            gap: None,
            missing_messages: Some(count),
            non_monotonic: false,
        }
    }

    pub(crate) fn stale_rows(count: u64) -> Self {
        Self {
            gap: None,
            missing_messages: Some(count),
            non_monotonic: true,
        }
    }

    pub(crate) fn mt5(anomaly: SeqAnomaly, from_ms: i64, to_ms: i64) -> Self {
        match anomaly {
            SeqAnomaly::Gap { missing, .. } => Self {
                gap: (to_ms >= from_ms).then_some(FeedGap { from_ms, to_ms }),
                missing_messages: Some(missing),
                non_monotonic: false,
            },
            SeqAnomaly::NotMonotonic { .. } => Self {
                gap: None,
                missing_messages: Some(0),
                non_monotonic: true,
            },
        }
    }
}

/// Cumulative diagnostics survive eviction from the bounded on-chart gap list.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FeedIntegrity {
    /// Anomalies, including unknown interruptions and non-monotonic IDs.
    pub anomalies: u64,
    /// Source messages absent from the usable tape: either source IDs skipped
    /// on forward arrivals or individually rejected source rows. This is not
    /// an estimate of lost executions. A later out-of-order arrival does not
    /// erase the historical diagnostic.
    pub missing_messages: u64,
    /// Interruptions whose missing-message count cannot be established.
    pub unknown_loss: u64,
    /// Repeated/backwards source IDs or a stale-row exclusion category.
    pub non_monotonic: u64,
}

impl FeedIntegrity {
    /// Incorporate exactly one ordered source diagnostic using bounded state.
    pub fn observe(&mut self, event: FeedContinuity) {
        self.anomalies = self.anomalies.saturating_add(1);
        if let Some(missing) = event.missing_messages {
            self.missing_messages = self.missing_messages.saturating_add(missing);
        } else {
            self.unknown_loss = self.unknown_loss.saturating_add(1);
        }
        self.non_monotonic = self
            .non_monotonic
            .saturating_add(u64::from(event.non_monotonic));
    }
}

/// The receiver outlives Binance's individual socket connections. Tracking
/// here retains the public trade-only adapter API while publishing anomalies
/// on the same ordered channel as the trades they qualify.
#[derive(Default)]
pub(crate) struct BinanceContinuity {
    highest_id: Option<u64>,
    highest_ms: Option<i64>,
    unknown_handoff: bool,
}

impl BinanceContinuity {
    pub(crate) fn after_backfill(last: Option<&Trade>) -> Self {
        Self {
            highest_id: last.map(|trade| trade.agg_id),
            highest_ms: last.map(|trade| trade.timestamp_ms),
            unknown_handoff: last.is_none(),
        }
    }

    #[inline]
    pub(crate) fn observe(&mut self, trade: &Trade) -> Option<FeedContinuity> {
        let event = if std::mem::take(&mut self.unknown_handoff) {
            Some(FeedContinuity {
                gap: None,
                missing_messages: None,
                non_monotonic: false,
            })
        } else {
            self.highest_id.and_then(|highest| {
                if trade.agg_id > highest && trade.agg_id - highest > 1 {
                    Some(FeedContinuity {
                        gap: self
                            .highest_ms
                            .filter(|from| *from <= trade.timestamp_ms)
                            .map(|from_ms| FeedGap {
                                from_ms,
                                to_ms: trade.timestamp_ms,
                            }),
                        missing_messages: Some(trade.agg_id - highest - 1),
                        non_monotonic: false,
                    })
                } else if trade.agg_id <= highest {
                    Some(FeedContinuity {
                        gap: None,
                        missing_messages: Some(0),
                        non_monotonic: true,
                    })
                } else {
                    None
                }
            })
        };
        if self.highest_id.is_none_or(|highest| trade.agg_id > highest) {
            self.highest_id = Some(trade.agg_id);
            self.highest_ms = Some(trade.timestamp_ms);
        }
        event
    }
}

/// Session-scoped MT5 IDs cannot establish continuity across a new connection.
/// Label the first resumed print with unknown loss, never a guessed count.
#[inline]
pub(crate) fn mt5_reconnect_gap(
    trade: &Trade,
    pending_gap: &mut Option<i64>,
) -> Option<FeedContinuity> {
    pending_gap.take().map(|from_ms| FeedContinuity {
        gap: (trade.timestamp_ms >= from_ms).then_some(FeedGap {
            from_ms,
            to_ms: trade.timestamp_ms,
        }),
        missing_messages: None,
        non_monotonic: false,
    })
}
