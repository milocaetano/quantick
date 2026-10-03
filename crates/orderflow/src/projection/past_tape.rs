//! The native tape at a past instant: the live tape's own pipeline, cut to
//! the blocks a past window touches, so panning back costs the window and
//! never the history after it.

use std::sync::Arc;

use quantick_engine::Bar;

use super::model::price_span;
use super::{
    DotHorizon, HeatmapProjection, PriceWindow, SettledProjection, TapeFacts, TierCut,
    TierGrouping, VolumeDots, cluster_tier, lane_grouping, refine_tier, tier_primitives,
};
use crate::history::LiquidityHistory;
use crate::timeline::{BarTimeline, LiveEdge};

/// Width of the grid past groups are frozen on: the window, rounded up to
/// whole native windows so no native window straddles two blocks.
#[must_use]
pub fn past_block_ms(window_ms: i64, dot_window_ms: i64) -> i64 {
    let dot = dot_window_ms.max(1);
    (window_ms.max(1) + dot - 1)
        .div_euclid(dot)
        .saturating_mul(dot)
}

/// The grid block and the half-open stretch of whole blocks a window ending
/// at `end_ms` touches: `(block_ms, from_ms, until_ms)`.
#[must_use]
pub fn past_span(end_ms: i64, window_ms: i64, dot_window_ms: i64) -> (i64, i64, i64) {
    let block_ms = past_block_ms(window_ms, dot_window_ms);
    let from_ms = end_ms.saturating_sub(window_ms).div_euclid(block_ms) * block_ms;
    (
        block_ms,
        from_ms,
        (end_ms.div_euclid(block_ms) + 1) * block_ms,
    )
}

/// One past tape window and the native facts of every block it touches.
#[derive(Debug, Clone, PartialEq)]
pub struct PastTape {
    /// The instant the tape's right edge stands for.
    pub end_ms: i64,
    /// Market time the tape shows, ending at [`end_ms`](Self::end_ms).
    pub window_ms: i64,
    /// The grid past groups are frozen on ([`past_block_ms`]).
    pub block_ms: i64,
    /// Half-open stretch of market time the facts cover: whole blocks.
    pub from_ms: i64,
    pub until_ms: i64,
    /// A block ending at or before this instant can no longer change: every
    /// print that could still belong to it was delivered a window ago.
    pub settled_through_ms: i64,
    /// First instant the retained tape is complete from.
    pub retained_from_ms: Option<i64>,
    /// The dot rungs the facts were folded on: tape window and level ticks.
    pub rungs: (i64, i64),
    /// The native facts (`live` marks) and their exact tape facts.
    pub projection: Arc<HeatmapProjection>,
}

impl PastTape {
    /// This stretch again for a window ending at `end_ms`, when nothing in it
    /// can change: the same blocks and rungs, every print that could join
    /// them delivered, none evicted. A held drag re-reads no history.
    #[must_use]
    pub fn reused_at(
        &self,
        end_ms: i64,
        window_ms: i64,
        dots: &VolumeDots,
        retained_from_ms: Option<i64>,
    ) -> Option<Self> {
        let (block_ms, from_ms, until_ms) = past_span(end_ms, window_ms, dots.tape_window_ms);
        let rungs = (dots.tape_window_ms, dots.tape_level_ticks);
        let same = (block_ms, from_ms, until_ms, window_ms, rungs)
            == (
                self.block_ms,
                self.from_ms,
                self.until_ms,
                self.window_ms,
                self.rungs,
            );
        let frozen = self.settled_through_ms >= until_ms
            && retained_from_ms.is_none_or(|retained| retained <= from_ms);
        (same && frozen).then(|| Self {
            end_ms,
            retained_from_ms,
            ..self.clone()
        })
    }

    /// Where the window's prints traded, `(low, high)`: the axis a held tape
    /// fits, from the same facts it draws.
    #[must_use]
    pub fn price_range(&self) -> Option<(f64, f64)> {
        let from = self.end_ms.saturating_sub(self.window_ms);
        price_span(
            self.projection
                .aggressions
                .iter()
                .filter(|mark| {
                    mark.last_timestamp_ms >= from && mark.first_timestamp_ms <= self.end_ms
                })
                .map(|mark| mark.price),
        )
    }
}

/// Where the visible candles are, for the timeline the facts are placed on.
pub struct PastBars<'a> {
    pub first_bar_index: usize,
    pub closed: &'a [Bar],
    pub partial: Option<&'a Bar>,
}

/// Project the native tape for a window ending at `end_ms`, through the same
/// clustering, folding and flooring the live tape runs. Only prints: depth
/// reductions are matched on the live tape alone.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn project_past_tape(
    history: &LiquidityHistory,
    bars: PastBars<'_>,
    end_ms: i64,
    window_ms: i64,
    reference_ms: i64,
    prices: PriceWindow,
    settled: &SettledProjection,
    dots: &VolumeDots,
) -> Option<PastTape> {
    let latest = history.latest_ms()?;
    let end_ms = end_ms.min(latest);
    let window_ms = window_ms.max(1);
    let (block_ms, from_ms, until_ms) = past_span(end_ms, window_ms, dots.tape_window_ms);
    let edge = LiveEdge {
        now_ms: until_ms,
        window_ms: until_ms - from_ms,
        reference_ms,
        on_newest_bar: false,
    };
    let timeline =
        BarTimeline::from_bars(bars.first_bar_index, bars.closed, bars.partial, Some(edge))
            .with_full_lane_coverage();
    let config = history.config();
    let coverage: Vec<_> = if config.depth_visible_anywhere() {
        history.coverage_segments().cloned().collect()
    } else {
        Vec::new()
    };
    let tier = cluster_tier(
        history,
        &timeline,
        prices,
        &coverage,
        TierGrouping {
            slots: settled.effective_grouping,
            lane: lane_grouping(config),
        },
        TierCut {
            range: (Some(from_ms), Some(until_ms)),
            tape_from_ms: Some(from_ms),
            reach_ms: Some(until_ms.saturating_add(window_ms)),
            dots: Some(dots),
        },
        false,
    );
    let (mut marks, floored_quantity) = refine_tier(
        tier,
        config,
        settled.aggression_reference,
        &timeline,
        settled.effective_grouping,
        false,
        Some((dots, DotHorizon::of(history))),
    );
    let tape_facts = marks.tape_facts.take().map(|clusters| {
        let floor = config.bubbles.min_quantity_decimal().unwrap_or_default();
        Arc::new(TapeFacts {
            floored_quantity: clusters
                .iter()
                .filter(|cluster| cluster.quantity < floor)
                .map(|cluster| cluster.quantity)
                .sum(),
            clusters,
            evicted_through_ms: history.evicted_through_ms(),
            opening_bursts: history.opening_bursts().to_vec(),
            floor,
            seal: None,
        })
    });
    let mut projection = HeatmapProjection::empty(true, settled.effective_grouping);
    projection.aggressions = tier_primitives(
        marks,
        &timeline,
        prices,
        settled.aggression_reference,
        settled.summary_reference,
        Some(dots),
    )
    .into_iter()
    .filter(|mark| mark.live)
    .collect();
    projection.tape_facts = tape_facts;
    projection.volume_dots = true;
    projection.floored_quantity = floored_quantity;
    projection.aggression_reference = settled.aggression_reference;
    projection.summary_reference = settled.summary_reference;
    Some(PastTape {
        end_ms,
        window_ms,
        block_ms,
        from_ms,
        until_ms,
        settled_through_ms: latest.saturating_sub(window_ms),
        retained_from_ms: history.tape_retained_from_ms(),
        rungs: (dots.tape_window_ms, dots.tape_level_ticks),
        projection: Arc::new(projection),
    })
}
