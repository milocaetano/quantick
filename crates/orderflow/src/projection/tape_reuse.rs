//! The worker's native tape, folded again only after its seal.
//!
//! A native cell is one window of one price, and prints arrive in time order,
//! so the cells before the previous publication's seal are final: the live
//! pass copies them and folds only the prints after the seal. Everything that
//! could make an old cell differ sends the pass back over the whole tape:
//! another configuration or window, depth evidence to correlate, a tape that
//! reaches further back than before, a horizon that moved back, or a print
//! that arrived behind the seal.

use rust_decimal::Decimal;

use super::dots::window_start;
use super::tiers::TierClusters;
use super::{SettledProjection, TapeFacts, VolumeDots};
use crate::HeatmapConfig;
use crate::history::{CoverageSegment, LiquidityHistory};
use crate::interaction::AggressionCluster;

/// The publication a live pass may continue from.
#[derive(Debug, Clone, Copy)]
pub struct TapeReuse<'a> {
    /// The tape facts of the last publication.
    pub previous: &'a TapeFacts,
    /// The worker's configuration revision now.
    pub revision: u64,
}

/// Where the live pass must start folding prints again, when every native
/// cell before it can be copied from `reuse`; `None` folds the whole tape.
pub(crate) fn reusable_through(
    reuse: TapeReuse<'_>,
    history: &LiquidityHistory,
    lane_from_ms: Option<i64>,
    settled: &SettledProjection,
    dots: Option<&VolumeDots>,
    coverage: &[CoverageSegment],
    live_from_ms: i64,
) -> Option<i64> {
    let seal = reuse.previous.seal.as_ref()?;
    let dots = dots?;
    let lane_from_ms = lane_from_ms?;
    let recorded = history.counters().aggressions_recorded;
    let fresh = usize::try_from(recorded.checked_sub(seal.recorded)?).ok()?;
    let retained = history.aggression_count();
    (seal.revision == reuse.revision
        && seal.window_ms == dots.tape_window_ms
        && dots.native_tape
        && dots.tape_level_ticks == 1
        && coverage.is_empty()
        && settled.live_events.is_empty()
        && seal
            .lane_from_ms
            .is_some_and(|previous| lane_from_ms >= previous)
        && live_from_ms <= lane_from_ms
        && history.evicted_through_ms() >= reuse.previous.evicted_through_ms
        && fresh <= retained
        && history
            .aggressions()
            .skip(retained - fresh)
            .all(|print| window_start(print.timestamp_ms, dots.tape_window_ms) >= seal.through_ms))
    .then_some(seal.through_ms)
}

/// Put the cells copied from `previous` in front of the freshly folded ones,
/// on the same rules the fold applies: on the tape from `lane_from_ms`, after
/// the eviction horizon, and cut at the display floor. Returns the quantity
/// the floor keeps off the canvas among the copied cells.
pub(super) fn continue_tape(
    marks: &mut TierClusters,
    previous: &TapeFacts,
    through_ms: i64,
    lane_from_ms: Option<i64>,
    evicted_through_ms: Option<i64>,
    window_ms: i64,
    config: &HeatmapConfig,
) -> Decimal {
    let window = |cell: &AggressionCluster| window_start(cell.first_timestamp_ms, window_ms);
    let low = lane_from_ms.map_or(0, |from| {
        previous
            .clusters
            .partition_point(|cell| window(cell) < from)
    });
    let high = previous
        .clusters
        .partition_point(|cell| window(cell) < through_ms);
    let kept: Vec<AggressionCluster> = previous.clusters[low..high.max(low)]
        .iter()
        .filter(|cell| evicted_through_ms.is_none_or(|horizon| window(cell) > horizon))
        .cloned()
        .collect();
    let floor = config.bubbles.min_quantity_decimal();
    let floored = floor.map_or(Decimal::ZERO, |floor| {
        kept.iter()
            .filter(|cell| cell.quantity < floor)
            .map(|cell| cell.quantity)
            .sum()
    });
    let shown = kept
        .iter()
        .filter(|cell| floor.is_none_or(|floor| cell.quantity >= floor))
        .cloned();
    marks.tape.splice(0..0, shown);
    if let Some(facts) = marks.tape_facts.as_mut() {
        facts.splice(0..0, kept);
    }
    floored
}
