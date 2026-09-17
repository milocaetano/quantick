//! Provider-neutral feed integrity snapshot contracts.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::wire::WireU64;

/// Independently versioned observations of received-row delivery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedDeliverySnapshot {
    /// One cumulative record per open market tab.
    pub tabs: Vec<TabFeedDeliverySnapshot>,
}

/// A zero observation never certifies a complete source stream.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TabFeedDeliverySnapshot {
    /// Stable market-tab identity.
    pub tab_id: WireU64,
    /// Original source-ID and unknown-interruption semantics.
    pub source_integrity: FeedIntegritySnapshot,
    /// True when any source counter has reached its saturating bound.
    pub source_counts_are_lower_bounds: bool,
    /// Absent when this attachment cannot report received-row exclusions.
    pub received_exclusions: Option<ReceivedExclusionsSnapshot>,
    /// Always false: these observations cannot prove completeness.
    pub completeness_proven: bool,
}

/// Received rows excluded before they could enter the usable tape.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ReceivedExclusionsSnapshot {
    /// Rows that could not be mapped into a valid trade.
    pub malformed_rows: WireU64,
    /// Non-overlap rows behind the accepted market-time watermark.
    pub stale_rows: WireU64,
    /// True if either count is saturated and therefore only a lower bound.
    pub counts_are_lower_bounds: bool,
}

/// Counts source messages, never estimates how many executed trades were lost.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FeedIntegritySnapshot {
    /// Source-ID anomalies and interruptions with unknown loss.
    pub anomalies: WireU64,
    /// Source IDs skipped on forward arrivals, not lost executions.
    pub missing_messages: WireU64,
    /// Interruptions whose missing-message count is unknown, never known zero.
    pub unknown_loss: WireU64,
    /// Repeated or backwards source IDs.
    pub non_monotonic: WireU64,
}

/// Neutral source counters in their original public units.
#[derive(Clone, Copy, Debug, Default)]
pub struct SourceCounts {
    /// Source-ID anomalies and unknown interruptions.
    pub anomalies: u64,
    /// Skipped source IDs.
    pub missing_messages: u64,
    /// Interruptions with unknown loss.
    pub unknown_loss: u64,
    /// Repeated or backwards source IDs.
    pub non_monotonic: u64,
}

/// Neutral received-row counters, independent of source-ID continuity.
#[derive(Clone, Copy, Debug, Default)]
pub struct ReceivedRowCounts {
    /// Received rows that could not become valid trades.
    pub malformed_rows: u64,
    /// Non-overlap rows behind the accepted timestamp watermark.
    pub stale_rows: u64,
}

impl From<SourceCounts> for FeedIntegritySnapshot {
    fn from(counts: SourceCounts) -> Self {
        Self {
            anomalies: WireU64::new(counts.anomalies),
            missing_messages: WireU64::new(counts.missing_messages),
            unknown_loss: WireU64::new(counts.unknown_loss),
            non_monotonic: WireU64::new(counts.non_monotonic),
        }
    }
}

impl TabFeedDeliverySnapshot {
    /// Construct wire observations without any dependency on a feed runtime.
    /// `None` is unavailable; available zeros never certify completeness.
    #[must_use]
    pub fn from_counts(
        tab_id: u64,
        source: SourceCounts,
        received: Option<ReceivedRowCounts>,
    ) -> Self {
        Self {
            tab_id: WireU64::new(tab_id),
            source_integrity: source.into(),
            source_counts_are_lower_bounds: [
                source.anomalies,
                source.missing_messages,
                source.unknown_loss,
                source.non_monotonic,
            ]
            .contains(&u64::MAX),
            received_exclusions: received.map(|counts| ReceivedExclusionsSnapshot {
                malformed_rows: WireU64::new(counts.malformed_rows),
                stale_rows: WireU64::new(counts.stale_rows),
                counts_are_lower_bounds: [counts.malformed_rows, counts.stale_rows]
                    .contains(&u64::MAX),
            }),
            completeness_proven: false,
        }
    }
}

/// Where a tab's tape delay is being spent.
///
/// The whole point of the breakdown is that "the chart is eighteen seconds
/// behind" is not actionable on its own: it reads the same whether the venue's
/// adapter was late, the wire was late, or this process drained late, and those
/// have different fixes. An investigation that starts here can name the hop
/// without a screenshot and without a person watching the corner of a window.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TapeHealthSnapshot {
    /// Newest print: venue stamp to this chart drawing it. The end-to-end
    /// figure the status bar shows.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub arrival_latency_ms: Option<i64>,
    /// The same measurement taken where the feed read the print off the wire,
    /// one hop earlier.
    ///
    /// The gap between this and `arrival_latency_ms` is what quantick's own
    /// queue and frame drain cost. It is a *derived* reading, not a measured
    /// one — the two are sampled at different instants — but a gap of seconds
    /// between them is unambiguous, and it is the only way to see that hop at
    /// all. `None` on a provider that cannot cut its own chain.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub feed_arrival_latency_ms: Option<i64>,
    /// Venue stamp to the source handing the print over: everything upstream
    /// of quantick.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub source_latency_ms: Option<i64>,
    /// The worst `source_latency_ms` over the sampled prints.
    ///
    /// The only peak reported, and deliberately: it is two source-side stamps
    /// subtracted per print, so every print contributes with no clock involved.
    /// A peak on the arrival or wire figures would need the reader's clock
    /// applied to a print that arrived earlier, which measures that print's age
    /// rather than its delay — on a quiet tape, the sampling interval itself.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub source_latency_peak_ms: Option<i64>,
    /// The source handing it over to quantick reading it: the wire.
    #[schemars(extend("x-unit" = "milliseconds"))]
    pub transport_latency_ms: Option<i64>,
    /// The provider's own name for the hop that owns most of the delay.
    pub dominant_hop: Option<String>,
    /// How many live prints the split covers.
    pub sampled_prints: WireU64,
}

/// Coarse tape change, independent of per-print latency fluctuations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TapeRevisionKey {
    /// The reported dominant source/wire hop.
    pub dominant_hop: Option<String>,
    /// Whether measured arrival latency strictly exceeds the caller's threshold.
    pub late: bool,
}

impl TapeHealthSnapshot {
    /// Preserve the caller's lateness policy without bringing its clock or runtime here.
    #[must_use]
    pub fn revision_key(&self, high_lag_ms: i64) -> TapeRevisionKey {
        TapeRevisionKey {
            dominant_hop: self.dominant_hop.clone(),
            late: self.arrival_latency_ms.is_some_and(|ms| ms > high_lag_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_wire_distinguishes_unavailable_zero_and_saturated_observations() {
        use serde_json::{json, to_value};
        let zero = TabFeedDeliverySnapshot::from_counts(7, SourceCounts::default(), None);
        assert_eq!(
            to_value(zero).unwrap(),
            json!({
                "tab_id":"7", "source_integrity":{"anomalies":"0","missing_messages":"0","unknown_loss":"0","non_monotonic":"0"},
                "source_counts_are_lower_bounds":false,"received_exclusions":null,"completeness_proven":false
            })
        );
        let available = TabFeedDeliverySnapshot::from_counts(
            7,
            SourceCounts::default(),
            Some(ReceivedRowCounts::default()),
        );
        assert_eq!(
            to_value(available).unwrap()["received_exclusions"],
            json!({"malformed_rows":"0","stale_rows":"0","counts_are_lower_bounds":false})
        );
        let mixed = TabFeedDeliverySnapshot::from_counts(
            9,
            SourceCounts {
                anomalies: 3,
                missing_messages: 0,
                unknown_loss: 3,
                non_monotonic: 0,
            },
            Some(ReceivedRowCounts {
                malformed_rows: 1,
                stale_rows: 1,
            }),
        );
        assert_eq!(
            to_value(mixed).unwrap(),
            json!({
                "tab_id":"9", "source_integrity":{"anomalies":"3","missing_messages":"0","unknown_loss":"3","non_monotonic":"0"},
                "source_counts_are_lower_bounds":false,"received_exclusions":{"malformed_rows":"1","stale_rows":"1","counts_are_lower_bounds":false},"completeness_proven":false
            })
        );
        let max = TabFeedDeliverySnapshot::from_counts(
            1,
            SourceCounts {
                missing_messages: u64::MAX,
                ..Default::default()
            },
            Some(ReceivedRowCounts {
                stale_rows: u64::MAX,
                ..Default::default()
            }),
        );
        assert!(max.source_counts_are_lower_bounds);
        assert!(max.received_exclusions.unwrap().counts_are_lower_bounds);
        assert!(!max.completeness_proven);
    }

    #[test]
    fn tape_revision_preserves_absent_equal_above_and_hop_only_changes() {
        let mut tape = TapeHealthSnapshot {
            arrival_latency_ms: None,
            feed_arrival_latency_ms: None,
            source_latency_ms: None,
            source_latency_peak_ms: None,
            transport_latency_ms: None,
            dominant_hop: None,
            sampled_prints: WireU64::new(0),
        };
        let absent = tape.revision_key(100);
        assert!(!absent.late);
        tape.arrival_latency_ms = Some(100);
        assert_eq!(tape.revision_key(100), absent);
        tape.arrival_latency_ms = Some(101);
        assert!(tape.revision_key(100).late);
        let previous = tape.revision_key(100);
        tape.dominant_hop = Some("wire".into());
        assert_ne!(previous, tape.revision_key(100));
        let same = tape.revision_key(100);
        tape.arrival_latency_ms = Some(999);
        tape.sampled_prints = WireU64::new(42);
        assert_eq!(tape.revision_key(100), same);
    }

    #[test]
    fn integrity_contract_matches_the_legacy_health_definition() {
        let mut generated = crate::schema::generated_schema::<FeedIntegritySnapshot>();
        let object = generated.as_object_mut().unwrap();
        object.remove("$schema");
        object.remove("title");
        object.remove("$defs");
        let source =
            include_str!("../../../schemas/control/observer-health-summary-v1.schema.json");
        let existing: serde_json::Value = serde_json::from_str(source).unwrap();
        assert_eq!(generated, existing["$defs"]["FeedIntegritySnapshot"]);
    }
}
