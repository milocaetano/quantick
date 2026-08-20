//! Renderer-independent projection of RLE history into normalized primitives.

use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::sync::Arc;

use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};

use super::config::{
    DEFAULT_LIVE_LANE_SHARE, DisplayGrouping, HeatmapConfig, IntensityMode, LiveLaneStyle,
    MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE,
};
use super::grouping::{EffectiveGrouping, GroupedLiquidity, GroupingWindow, sweep_grouped_runs};
use super::history::{AggressorSide, CoverageSegment, LiquidityHistory, RestingSide};
pub use super::interaction::LiquidityEvidence;
use super::interaction::{
    AggressionCluster, LiquidityEvent, cluster_aggressions, correlate_liquidity, liquidity_events,
    merge_dust_clusters, regionalize_clusters, summarize_clusters,
};
use super::timeline::BarTimeline;

/// Exact visible price interval. `high` maps to y=0 and `low` to y=1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PriceWindow {
    /// Lowest visible price.
    pub low: Decimal,
    /// Highest visible price.
    pub high: Decimal,
}

impl PriceWindow {
    /// Construct a non-degenerate price window.
    #[must_use]
    pub fn new(low: Decimal, high: Decimal) -> Option<Self> {
        (high > low).then_some(Self { low, high })
    }

    /// Map a visible price to normalized screen y.
    #[must_use]
    pub fn y(&self, price: Decimal) -> Option<f64> {
        if price < self.low || price > self.high {
            return None;
        }
        ((self.high - price) / (self.high - self.low)).to_f64()
    }
}

/// One clipped liquidity rectangle ready for a backend to colour.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapCell {
    /// Synchronization generation.
    pub generation: u64,
    /// Resting side.
    pub side: RestingSide,
    /// Exact lower bucket edge before clipping.
    pub price_bucket: Decimal,
    /// Aggregated displayed quantity.
    pub quantity: Decimal,
    /// Normalized left and right positions.
    pub x0: f64,
    /// Normalized right position.
    pub x1: f64,
    /// Normalized top and bottom positions.
    pub y0: f64,
    /// Normalized bottom position.
    pub y1: f64,
    /// Gamma-adjusted colour-ramp position.
    pub intensity: f32,
    /// Final alpha after applying configured opacity.
    pub alpha: f32,
}

/// One aggressive execution ready for circles, footprint cells or tooltips.
#[derive(Debug, Clone, PartialEq)]
pub struct AggressionPrimitive {
    /// Representative aggregate-trade id.
    pub agg_id: u64,
    /// Every aggregate-trade id represented by this bubble.
    pub agg_ids: Vec<u64>,
    /// Coverage generation derived from exchange timestamp.
    pub generation: Option<u64>,
    /// Taker side.
    pub side: AggressorSide,
    /// Passive side this trade attempted to consume.
    pub consumed_side: RestingSide,
    /// Exact execution quantity.
    pub quantity: Decimal,
    /// `[0,1]` share of [`quantity`](Self::quantity) taken by buyers.
    ///
    /// `1.0` or `0.0` on the single-sided bubbles that make up the tape.
    /// Anything between is a closed-bar summary carrying both sides, which the
    /// renderer draws as a pie. Computed here rather than at draw time: the
    /// projection runs on its own thread every few hundred milliseconds, the
    /// renderer runs every frame and must not divide `Decimal`s per bubble.
    pub buy_share: f32,
    /// Whether this bubble is in the live lane — the reserved band right of
    /// the forming bar, where the lane's own radius range applies.
    pub live: bool,
    /// Inclusive lower edge of the visual price range.
    pub price_bucket: Decimal,
    /// Exact price height the range covers, starting at
    /// [`price_bucket`](Self::price_bucket): one visual row for a plain
    /// bubble, the whole region for a regional fold. Range-drawing consumers
    /// (the live strip's histogram) read this instead of assuming one row.
    pub price_span: Decimal,
    /// Number of aggregate trades represented by this bubble.
    pub trade_count: usize,
    /// Earliest exchange timestamp represented by this bubble.
    pub first_timestamp_ms: i64,
    /// Latest exchange timestamp represented by this bubble.
    pub last_timestamp_ms: i64,
    /// Exact bubble quantity aligned with compatible liquidity reductions.
    pub matched_quantity: Decimal,
    /// `[0,1]` fraction of bubble quantity aligned with reductions.
    pub matched_fraction: f32,
    /// Factual liquidity-event ids receiving matched bubble quantity.
    pub liquidity_event_ids: Vec<u64>,
    /// Normalized chart coordinates.
    pub x: f64,
    /// Normalized y coordinate.
    pub y: f64,
    /// `[0,1]` size factor whose square is proportional to quantity.
    pub size: f32,
    /// How many separate marks the frame's budget folded into this one.
    ///
    /// Zero on a bubble the budget never touched — what it draws is what one
    /// cluster of prints did. Above one it is a fold, and the renderer says so:
    /// reading a fold as a single execution is reading a size that never
    /// traded at once, and a trader sizing a position off that is being lied
    /// to. Nothing is lost either way — the quantity is exact — but the two
    /// must not look the same.
    pub folded_marks: u32,
}

/// One factual displayed-liquidity reduction ready for an overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct LiquidityEventPrimitive {
    /// Deterministic frame-local id.
    pub event_id: u64,
    /// Synchronization generation.
    pub generation: u64,
    /// Resting side.
    pub side: RestingSide,
    /// Inclusive lower edge of the visual price range.
    pub price_bucket: Decimal,
    /// Exchange timestamp of the before/after observation.
    pub timestamp_ms: i64,
    /// Displayed quantity immediately before the reduction.
    pub before: Decimal,
    /// Displayed quantity immediately after the reduction.
    pub after: Decimal,
    /// Exact factual reduction.
    pub removed: Decimal,
    /// `[0,1]` reduction fraction relative to `before`.
    pub fraction: f32,
    /// Whether the displayed visual range became empty.
    pub full_removal: bool,
    /// Exact compatible aggression quantity allocated to this event.
    pub matched_quantity: Decimal,
    /// `[0,1]` matched fraction relative to `removed`.
    pub matched_fraction: f32,
    /// Available factual evidence without a causal label.
    pub evidence: LiquidityEvidence,
    /// Normalized horizontal observation coordinate.
    pub x: f64,
    /// Normalized top of the affected visual price range.
    pub y0: f64,
    /// Normalized bottom of the affected visual price range.
    pub y1: f64,
}

/// Reason recorded for the stretch of chart older than the first snapshot this
/// session captured. It is the only gap that can span most of the viewport, so
/// renderers mark it differently from an interior discontinuity.
///
/// Exported so the renderer's label table matches on this constant instead of
/// repeating the literal: a reason renamed here would otherwise fall through
/// to the generic label without a single test noticing.
pub const BEFORE_CAPTURE: &str = "book_unavailable_before_capture";

/// A visible interval that must not be filled or connected.
#[derive(Debug, Clone, PartialEq)]
pub struct GapPrimitive {
    /// Previous synchronized generation.
    pub from_generation: Option<u64>,
    /// Replacement generation.
    pub to_generation: Option<u64>,
    /// Normalized horizontal interval.
    pub x0: f64,
    /// Normalized horizontal interval end.
    pub x1: f64,
    /// Diagnostic reason copied from history.
    pub reason: String,
}

impl GapPrimitive {
    /// Whether this is the leading stretch that predates local capture, as
    /// opposed to a discontinuity inside covered time.
    #[must_use]
    pub fn precedes_capture(&self) -> bool {
        self.reason == BEFORE_CAPTURE
    }
}

/// Complete pure output for one chart frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HeatmapProjection {
    /// Whether the feature was enabled in sanitized configuration.
    pub enabled: bool,
    /// Exact quantity the trader's own display floor
    /// ([`BubbleStyle::min_quantity`]) kept off the canvas.
    ///
    /// The only contracts a frame still leaves undrawn, and reported in
    /// contracts rather than in marks so the reading is the size of what is
    /// missing, not the number of dots. Zero unless the floor is set.
    pub floored_quantity: Decimal,
    /// Whether this frame's candle marks are bar summaries rather than raw
    /// clusters.
    ///
    /// A summary counts a print in its bar *and* leaves it on the tape, on
    /// purpose — the pie is an aggregate, the tape mark is the detail. Any
    /// consumer that sums across both panes has to know, or it counts the same
    /// contract twice.
    pub summarized: bool,
    /// Visible heatmap rectangles.
    ///
    /// Shared rather than owned: this layer is rebuilt on the projection
    /// cadence while the frame around it is rebuilt per frame, so copying it
    /// every time would cost more than building the live half does.
    pub cells: Arc<Vec<HeatmapCell>>,
    /// Visible aggressive executions.
    pub aggressions: Vec<AggressionPrimitive>,
    /// Visible factual displayed-liquidity reductions.
    pub liquidity_events: Vec<LiquidityEventPrimitive>,
    /// Visible continuity gaps. Shared for the reason [`cells`](Self::cells) is.
    pub gaps: Arc<Vec<GapPrimitive>>,
    /// Normalized x the live edge has reached inside the lane, and the signal
    /// that this frame has a lane at all. `None` when it follows no live edge.
    ///
    /// The lane's left boundary is not carried here: it is the forming slot's
    /// own edge, which the layout already knows from the slot count.
    pub live_now_x: Option<f64>,
    /// Exact visual grouping resolved for this frame.
    pub effective_grouping: EffectiveGrouping,
    /// Quantity that maps to full cell intensity.
    pub liquidity_reference: Decimal,
    /// Quantity that maps to full aggression size for a single-print bubble.
    pub aggression_reference: Decimal,
    /// Quantity that maps to full size for a closed-bar summary. Equal to
    /// [`aggression_reference`](Self::aggression_reference) whenever nothing
    /// is summarized, which is when both regions share one size scale.
    pub summary_reference: Decimal,
    /// Cells omitted by the configured primitive cap.
    pub dropped_cells: usize,
    /// Aggressions omitted by the configured primitive cap.
    pub folded_aggressions: usize,
    /// Liquidity events omitted by the visible-cell safety cap.
    pub dropped_liquidity_events: usize,
}

impl HeatmapProjection {
    /// A frame with nothing to draw: the seed the render tests build a
    /// projection from, so they exercise the same struct the pipeline emits.
    ///
    /// The pipeline itself starts from [`SettledProjection::empty`] — it always
    /// has a live half to attach, even when that half is empty too.
    #[cfg(test)]
    pub(crate) fn empty(enabled: bool, effective_grouping: EffectiveGrouping) -> Self {
        Self {
            enabled,
            summarized: false,
            floored_quantity: Decimal::ZERO,
            cells: Arc::new(Vec::new()),
            aggressions: Vec::new(),
            liquidity_events: Vec::new(),
            gaps: Arc::new(Vec::new()),
            live_now_x: None,
            effective_grouping,
            liquidity_reference: Decimal::ZERO,
            aggression_reference: Decimal::ZERO,
            summary_reference: Decimal::ZERO,
            dropped_cells: 0,
            folded_aggressions: 0,
            dropped_liquidity_events: 0,
        }
    }
}

/// The half of a frame that is finished, and can therefore be kept.
///
/// Its bars are closed and their prints are all in, so nothing in here changes
/// until the layout does. The other half — [`LiveMarks`] — is whatever is still
/// moving, and is rebuilt as often as the chart draws.
#[derive(Debug, Clone, PartialEq)]
pub struct SettledProjection {
    /// Exact quantity the trader's own display floor
    /// ([`BubbleStyle::min_quantity`]) kept off the canvas.
    ///
    /// The only contracts a frame still leaves undrawn, and reported in
    /// contracts rather than in marks so the reading is the size of what is
    /// missing, not the number of dots. Zero unless the floor is set.
    pub floored_quantity: Decimal,
    /// Whether this half's marks are bar summaries. See
    /// [`HeatmapProjection::summarized`].
    pub summarized: bool,
    /// Whether the feature was enabled in sanitized configuration.
    pub enabled: bool,
    /// Visible heatmap rectangles.
    pub cells: Arc<Vec<HeatmapCell>>,
    /// Bubbles of the bars that are done.
    pub aggressions: Vec<AggressionPrimitive>,
    /// Visible factual displayed-liquidity reductions.
    pub liquidity_events: Vec<LiquidityEventPrimitive>,
    /// Visible continuity gaps.
    pub gaps: Arc<Vec<GapPrimitive>>,
    /// Exact visual grouping resolved for this frame.
    pub effective_grouping: EffectiveGrouping,
    /// Quantity that maps to full cell intensity.
    pub liquidity_reference: Decimal,
    /// Quantity that maps to full aggression size for a single-print bubble.
    pub aggression_reference: Decimal,
    /// Quantity that maps to full size for a closed-bar summary.
    pub summary_reference: Decimal,
    /// Cells omitted by the configured primitive cap.
    pub dropped_cells: usize,
    /// Bubbles this half was already over the primitive cap by.
    ///
    /// Capping here as well as over the whole frame keeps the per-frame merge
    /// proportional to what can be drawn rather than to the visible tape. It
    /// costs nothing in what is shown: a mark the frame would keep is by
    /// definition among the strongest of this half too.
    pub folded_aggressions: usize,
    /// Liquidity events omitted by the visible-cell safety cap.
    pub dropped_liquidity_events: usize,
    /// Exchange time this half stops at, and the live half takes over from.
    ///
    /// Snapped to a bar's open time, so no bar is summarized twice — once per
    /// half — and drawn as two partial marks where one whole one is owed.
    pub live_from_ms: Option<i64>,
    /// Reductions timestamped inside the live half, still unallocated.
    ///
    /// They are swept here, where the book is read, and handed over rather than
    /// matched: the prints that could account for them are the ones the live
    /// half rebuilds, so it is the half that must do the matching.
    pub live_events: Vec<LiquidityEvent>,
}

/// The marks of the part of the chart that is still moving.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LiveMarks {
    /// Bubbles for the prints after [`SettledProjection::live_from_ms`].
    pub aggressions: Vec<AggressionPrimitive>,
    /// Markers for the reductions those same prints were matched against.
    pub liquidity_events: Vec<LiquidityEventPrimitive>,
    /// Reductions the safety cap left out of this half.
    pub dropped_liquidity_events: usize,
    /// Marks this half folded into a neighbour to fit its pane's budget.
    pub folded_aggressions: usize,
    /// Exact quantity this half's display floor kept off the canvas.
    pub floored_quantity: Decimal,
    /// Normalized x the live edge has reached inside the lane.
    pub live_now_x: Option<f64>,
}

impl SettledProjection {
    /// A settled half with nothing in it.
    pub(crate) fn empty(enabled: bool, effective_grouping: EffectiveGrouping) -> Self {
        Self {
            enabled,
            summarized: false,
            floored_quantity: Decimal::ZERO,
            cells: Arc::new(Vec::new()),
            aggressions: Vec::new(),
            liquidity_events: Vec::new(),
            gaps: Arc::new(Vec::new()),
            effective_grouping,
            liquidity_reference: Decimal::ZERO,
            aggression_reference: Decimal::ZERO,
            summary_reference: Decimal::ZERO,
            dropped_cells: 0,
            folded_aggressions: 0,
            dropped_liquidity_events: 0,
            live_from_ms: None,
            live_events: Vec::new(),
        }
    }

    /// Put the two halves together into the frame a renderer draws.
    ///
    /// Each pane arrives already inside *its own* share of the bubble budget —
    /// both halves fold where they are built, against the pane each mark
    /// belongs to — so joining them is a concatenation and never a
    /// competition. One shared budget was the bug: the candles' marks each
    /// carry a bar and the tape's each carry a print, so ranking them together
    /// made zooming the candles out empty the tape.
    #[must_use]
    pub fn with_live(&self, live: LiveMarks, config: &HeatmapConfig) -> HeatmapProjection {
        let _ = config;
        let folded_aggressions = self.folded_aggressions + live.folded_aggressions;
        let mut aggressions = Vec::with_capacity(self.aggressions.len() + live.aggressions.len());
        aggressions.extend(self.aggressions.iter().cloned());
        aggressions.extend(live.aggressions);
        // Folded first, then ordered: a fold picks by size or by age, but what
        // a frame draws is ordered by time, so a chart that is over the budget
        // stacks its bubbles the same way as one that is under it.
        aggressions.sort_by(|a, b| {
            a.first_timestamp_ms
                .cmp(&b.first_timestamp_ms)
                .then_with(|| a.last_timestamp_ms.cmp(&b.last_timestamp_ms))
                .then_with(|| a.live.cmp(&b.live))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        });

        // The display switches — the aggression layer's master switch and the
        // per-side ones — are *not* applied here. A projection is the fact the
        // frame observed, and more than one surface reads it: the bubbles, the
        // consumption carve, and the live strip's histogram beside the price
        // axis. Filtering here made every one of them a hostage of the bubble
        // switch (the strip went blank when the bubbles were hidden). Each
        // renderer now decides what it draws — see `RenderContext::bubbles`.

        // Both halves capped themselves where they were built; the join is
        // capped again for the same reason the bubbles are, so the markers a
        // frame draws stay inside one budget rather than one per half.
        let mut liquidity_events = self.liquidity_events.clone();
        liquidity_events.extend(live.liquidity_events);
        let dropped_liquidity_events = self.dropped_liquidity_events
            + live.dropped_liquidity_events
            + liquidity_events
                .len()
                .saturating_sub(config.max_visible_cells);
        cap_events(&mut liquidity_events, config.max_visible_cells);

        HeatmapProjection {
            enabled: self.enabled,
            summarized: self.summarized,
            floored_quantity: self.floored_quantity + live.floored_quantity,
            cells: Arc::clone(&self.cells),
            aggressions,
            liquidity_events,
            gaps: Arc::clone(&self.gaps),
            live_now_x: live.live_now_x,
            effective_grouping: self.effective_grouping,
            liquidity_reference: self.liquidity_reference,
            aggression_reference: self.aggression_reference,
            summary_reference: self.summary_reference,
            dropped_cells: self.dropped_cells,
            folded_aggressions,
            dropped_liquidity_events,
        }
    }
}

#[derive(Debug)]
struct DraftCell {
    generation: u64,
    side: RestingSide,
    price_bucket: Decimal,
    quantity: Decimal,
    x0: f64,
    x1: f64,
    y0: f64,
    y1: f64,
}

/// One level's resting liquidity, summed over one bar and weighted by time.
///
/// The accumulator behind a bar slot's summary band: `weighted / span_ms` is
/// the quantity that was typically resting at that price while the bar ran.
struct SlotHeat {
    generation: u64,
    /// Σ quantity × milliseconds it was displayed for, inside this bar.
    weighted: Decimal,
    /// The bar's own duration, which the sum is averaged over — so a level
    /// present for part of the bar reads proportionally fainter.
    span_ms: i64,
    y0: f64,
    y1: f64,
}

/// Project retained order flow into `[0,1] × [0,1]` chart primitives.
///
/// Capture buckets are swept into visual ranges only for this frame. Retained
/// base history remains untouched when grouping or zoom changes.
///
/// The whole frame in one call, which is how the tests read a projection.
///
/// The app itself never takes this path: it redraws the tape far faster than it
/// redraws the map, so it holds the two halves apart — [`project_settled`] and
/// [`project_live`] — and puts them together per frame with
/// [`SettledProjection::with_live`]. Composing them here, in one call, is what
/// keeps that split honest: every test that asserts on a whole frame is
/// asserting on both halves joined exactly as the app joins them.
#[cfg(test)]
#[must_use]
pub fn project(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
) -> HeatmapProjection {
    let settled = project_settled(history, timeline, prices);
    let live = project_live(history, timeline, prices, &settled);
    settled.with_live(live, history.config())
}

/// Build the half of the frame that is finished.
///
/// Everything here is a statement about time that has stopped moving: resting
/// liquidity as it was, the reductions it went through, and the bubbles of bars
/// whose prints are all in. It is what a caller may keep and redraw unchanged
/// until the layout moves under it.
#[must_use]
pub fn project_settled(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
) -> SettledProjection {
    let config = history.config();
    let effective_grouping = EffectiveGrouping::resolve(
        config.display_grouping,
        config.price_grouping,
        prices.high - prices.low,
    );
    if !config.any_layer_enabled() {
        return SettledProjection::empty(false, effective_grouping);
    }
    let Some((time_start, time_end)) = timeline.timestamp_range() else {
        return SettledProjection::empty(true, effective_grouping);
    };

    // The depth layer is projected only while the map is both recording and on
    // screen — on *either* pane. Retained runs survive hiding it untouched:
    // they simply stop being drawn and keep accumulating, so the aggression
    // layer can render without the map behind it and reopening repaints the
    // whole retained past.
    //
    // "Either pane" is the whole point and was the bug: these cells span the
    // normalized x axis, tape included, and the renderer clips them per pane
    // (`layer_clip`). Gating production on the *candles'* switch therefore
    // deleted the tape's map along with the chart's — the projection decided
    // there was nothing to draw before the renderer ever got to decide where.
    let depth_enabled = config.depth_visible_anywhere();
    let retained_start = history
        .retention_start_ms()
        .map_or(time_start, |start| start.max(time_start));
    let open_run_end_ms = history.latest_book_ms().unwrap_or(time_end);
    let coverage: Vec<_> = if depth_enabled {
        history.coverage_segments().cloned().collect()
    } else {
        Vec::new()
    };
    let grouped = if depth_enabled {
        sweep_grouped_runs(
            history.runs_intersecting(retained_start, time_end),
            coverage.iter(),
            effective_grouping,
            GroupingWindow {
                start_ms: retained_start,
                end_ms: time_end,
                open_run_end_ms,
                price_low: prices.low,
                price_high: prices.high,
            },
        )
    } else {
        GroupedLiquidity::default()
    };

    // Where the book is drawn, and in what form.
    //
    // A bar is timeless: its slot is a fixed width whatever market time it took,
    // so drawing sub-bar runs inside it invents a clock the bar does not have —
    // and once the tape covers the present, asking only for the newest place a
    // run belongs leaves the candles' half of the chart frozen a window in the
    // past. With a tape on screen the two halves split the job the way the
    // prints already do: the tape keeps the runs themselves, second by second,
    // and each bar slot carries *one summary band per level* — the quantity
    // that was typically resting there while the bar ran, each run weighted by
    // how much of the bar it covered. A wall that stood the whole bar reads at
    // its own size; one that flickered for a tenth of it reads a tenth as
    // bright. Both panes read on the same scale, so a stable wall looks the
    // same on either side of the divider.
    //
    // Without a tape there is nothing to carry the detail, so the chart keeps
    // drawing every run where it happened, exactly as it always has.
    let mut drafts = Vec::new();
    // One entry per run, so the intensity reference measures the book itself
    // rather than how many views a level happens to be drawn in.
    let mut run_quantities = Vec::with_capacity(grouped.runs.len());
    let mut summary: BTreeMap<(usize, Decimal, RestingSide), SlotHeat> = BTreeMap::new();
    let lane_view = timeline.lane_start_ms().is_some();
    for run in &grouped.runs {
        let bucket_low = run.price_bucket;
        let bucket_high = bucket_low + effective_grouping.bucket_width;
        let clipped_low = bucket_low.max(prices.low);
        let clipped_high = bucket_high.min(prices.high);
        let Some(y0) = prices.y(clipped_high) else {
            continue;
        };
        let Some(y1) = prices.y(clipped_low) else {
            continue;
        };
        if y1 <= y0 {
            continue;
        }
        let draft = |x0: f64, x1: f64, quantity: Decimal| DraftCell {
            generation: run.generation,
            side: run.side,
            price_bucket: run.price_bucket,
            quantity,
            x0,
            x1,
            y0,
            y1,
        };

        let mut drawn = false;
        if lane_view {
            for span in timeline.slots_between(run.start_ms, run.end_ms) {
                let overlap = run
                    .end_ms
                    .min(span.end_ms)
                    .saturating_sub(run.start_ms.max(span.start_ms));
                if overlap <= 0 {
                    continue;
                }
                let entry = summary
                    .entry((span.index, run.price_bucket, run.side))
                    .or_insert(SlotHeat {
                        generation: run.generation,
                        weighted: Decimal::ZERO,
                        span_ms: (span.end_ms - span.start_ms).max(1),
                        y0,
                        y1,
                    });
                entry.weighted = entry
                    .weighted
                    .saturating_add(run.quantity.saturating_mul(Decimal::from(overlap)));
                entry.generation = entry.generation.max(run.generation);
                drawn = true;
            }
            if let (Some(x0), Some(x1)) = (
                timeline.locate_in_lane_clamped(run.start_ms),
                timeline.locate_in_lane_clamped(run.end_ms),
            ) && x1.normalized > x0.normalized
            {
                drafts.push(draft(x0.normalized, x1.normalized, run.quantity));
                drawn = true;
            }
        } else if let (Some(x0), Some(x1)) = (
            timeline.locate_clamped(run.start_ms),
            timeline.locate_clamped(run.end_ms),
        ) && x1.normalized > x0.normalized
        {
            drafts.push(draft(x0.normalized, x1.normalized, run.quantity));
            drawn = true;
        }
        if drawn {
            run_quantities.push(run.quantity);
        }
    }
    for ((index, price_bucket, side), heat) in summary {
        let (x0, x1) = timeline.slot_bounds(index);
        drafts.push(DraftCell {
            generation: heat.generation,
            side,
            price_bucket,
            quantity: heat.weighted / Decimal::from(heat.span_ms),
            x0,
            x1,
            y0: heat.y0,
            y1: heat.y1,
        });
    }

    let liquidity_reference = match config.intensity_mode {
        IntensityMode::VisibleP99 => percentile_99(run_quantities.into_iter()),
        IntensityMode::Fixed(maximum) => maximum,
    };

    // Hidden heat is gated after the reference: the drafts fed the P99 above,
    // and the depletion floors keyed to it must not move just because the map
    // behind them is switched off. Before the drop accounting, so the health
    // counters never blame the cap for cells the user chose to hide.
    if !config.show_liquidity {
        drafts.clear();
    }
    let dropped_cells = drafts.len().saturating_sub(config.max_visible_cells);
    if dropped_cells > 0 {
        // Retain the strongest walls deterministically and surface the loss.
        drafts.sort_by(|a, b| {
            b.quantity
                .cmp(&a.quantity)
                .then_with(|| a.generation.cmp(&b.generation))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.x0.total_cmp(&b.x0))
        });
        drafts.truncate(config.max_visible_cells);
    }

    let cells = drafts
        .into_iter()
        .map(|draft| {
            let intensity =
                normalized_log_intensity(draft.quantity, liquidity_reference, config.gamma);
            HeatmapCell {
                generation: draft.generation,
                side: draft.side,
                price_bucket: draft.price_bucket,
                quantity: draft.quantity,
                x0: draft.x0,
                x1: draft.x1,
                y0: draft.y0,
                y1: draft.y1,
                intensity,
                alpha: intensity * config.opacity,
            }
        })
        .collect();

    // The chart draws the same flow in two views. The tape shows the last
    // stretch of market time print by print; a bar slot shows what its bar has
    // come to. A print is on the tape while it is inside the rolling window,
    // and it belongs to its bar's slot either once it has aged out of the
    // window or — while summarizing — immediately, because a summary is a
    // running statement about its bar and has to be complete at every instant:
    // the bar that just closed the moment it closes, and the forming bar as its
    // orders arrive, so the left side reads what is happening now instead of
    // only what already happened. Raw prints are still drawn exactly once; only
    // the aggregate is allowed to overlap the tape it was computed from.
    // Whether the bar is summarized is the trader's summary switch and nothing
    // else. It used to also demand both side switches — a two-sided mark would
    // lie about its size with one side hidden — but that put a *display*
    // choice back inside the projection, and the live strip reads these same
    // clusters: hiding one side of the bubbles reshaped the strip's histogram
    // (the hostage relationship this branch exists to end, and the shipped
    // presets turn the summary on). The honesty it protected is enforced where
    // the ink is now: `RenderContext::bubbles` refuses to draw a two-sided
    // mark while a side is hidden.
    let summarizing = config.bubble_candle_summary;

    // The chart is cut in two at the oldest bar still taking orders. What
    // follows that instant is redrawn from the tape every frame, so this half
    // deliberately stops short of it; what precedes it is finished and is what
    // this build is for.
    let live_from_ms = timeline.live_boundary_ms();
    let mut settled = cluster_tier(
        history,
        timeline,
        prices,
        &coverage,
        TierGrouping {
            slots: effective_grouping,
            lane: lane_grouping(config),
        },
        TierCut {
            range: (None, live_from_ms),
            tape_from_ms: None,
        },
        summarizing,
    );
    // The size scale is a statement about the session, never about the
    // screen: zoom decides what is visible, not what a quantity means, so the
    // same print keeps the same area through every window, and a cluster the
    // viewport merges past the scale saturates at full size — the honest
    // reading of "more than anything the scale measures". The history
    // accumulates it one print at a time (`SessionScale`), so reading it here
    // costs the same whether ten prints are retained or a million — and it is
    // independent of the display filter below, so hiding small prints never
    // silently rescales the ones left on screen.
    let aggression_reference = history.bubble_size_reference();

    // A reduction is allocated by the half that owns the prints around it, so
    // the same removed quantity is never claimed as evidence twice. Cut at the
    // same instant the prints were.
    let mut events = if config.liquidity_events_enabled() {
        liquidity_events(&grouped.transitions)
    } else {
        Vec::new()
    };
    let live_events = match live_from_ms {
        Some(from) => {
            let mut live = Vec::new();
            let mut kept = Vec::with_capacity(events.len());
            for event in std::mem::take(&mut events) {
                if event.timestamp_ms >= from {
                    live.push(event);
                } else {
                    kept.push(event);
                }
            }
            events = kept;
            live
        }
        None => Vec::new(),
    };
    correlate_tier(&mut events, &mut settled, config, summarizing);
    let dropped_liquidity_events = filter_events(&mut events, config, liquidity_reference);

    let (settled_marks, floored_quantity) = refine_tier(
        settled,
        config,
        aggression_reference,
        timeline,
        effective_grouping,
        summarizing,
    );
    // While every mark is a raw print they share the session print scale
    // above, so an area means the same thing everywhere. The summary breaks
    // that premise: a pie carries a whole bar and a tape mark carries one
    // print, quantities an order of magnitude apart, and one shared reference
    // would peg every pie at the largest radius while flattening the tape
    // into dots. Pies then get their own scale — as session-anchored as the
    // print scale, measured against the busiest minute a price level saw
    // (`SummaryScale`), never against whatever pies happen to be on screen:
    // pies dominate a summarized chart, so a viewport reference here would
    // hand the zoom the very rescale the print scale just took away. Under a
    // fixed reference both getters return the pinned quantity, because the
    // user chose it precisely so that nothing on screen may rescale a mark.
    let summary_reference = if summarizing {
        history.bubble_summary_reference()
    } else {
        aggression_reference
    };

    let mut aggressions = tier_primitives(
        settled_marks,
        timeline,
        prices,
        aggression_reference,
        summary_reference,
    );
    let (chart_budget, _) = pane_budgets(config.max_aggression_primitives, &config.live_lane);
    let before_fold = aggressions.len();
    fold_to_budget(
        &mut aggressions,
        chart_budget,
        FoldOrder::SmallestFirst,
        // The scale the marks were drawn on. With the summary on these are
        // pies carrying whole bars, sized against `summary_reference`; folding
        // them against the print scale pegs every one at the largest radius
        // (`normalized_area_size` clamps the ratio), so a summarized chart over
        // budget would fill with max-size pies beside honestly sized ones.
        summary_reference,
        Some(timeline),
    );
    let folded_aggressions = before_fold.saturating_sub(aggressions.len());

    let liquidity_events = event_primitives(events, timeline, prices, effective_grouping);

    // Coverage primitives describe the depth layer. With L2 capture off there
    // is no map whose absence needs explaining, so a bubbles-only frame emits
    // no gap marks at all.
    let mut gaps: Vec<GapPrimitive> = if depth_enabled && config.show_gaps {
        history
            .coverage_gaps()
            .filter_map(|gap| {
                let gap_end = gap.end_ms.unwrap_or(time_end);
                if gap_end <= time_start || gap.start_ms >= time_end {
                    return None;
                }
                let x0 = timeline.locate_clamped(gap.start_ms.max(time_start))?;
                let x1 = timeline.locate_clamped(gap_end.min(time_end))?;
                (x1.normalized > x0.normalized).then(|| GapPrimitive {
                    from_generation: gap.from_generation,
                    to_generation: gap.to_generation,
                    x0: x0.normalized,
                    x1: x1.normalized,
                    reason: gap.reason.clone(),
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    // Historical trades can precede the first locally captured L2 snapshot.
    // Make that absence an explicit primitive instead of a transparent region
    // that could be mistaken for zero resting liquidity. The gap switch covers
    // this leading span too: it is one legend entry, and half-hiding it would
    // leave the legend describing marks the viewer cannot see.
    if depth_enabled && config.show_gaps {
        match history.coverage_segments().next() {
            Some(first_coverage) if first_coverage.start_ms > time_start => {
                let unavailable_end = first_coverage.start_ms.min(time_end);
                if let (Some(x0), Some(x1)) = (
                    timeline.locate_clamped(time_start),
                    timeline.locate_clamped(unavailable_end),
                ) && x1.normalized > x0.normalized
                {
                    gaps.push(GapPrimitive {
                        from_generation: None,
                        to_generation: Some(first_coverage.generation),
                        x0: x0.normalized,
                        x1: x1.normalized,
                        reason: BEFORE_CAPTURE.to_owned(),
                    });
                }
            }
            None => gaps.push(GapPrimitive {
                from_generation: None,
                to_generation: None,
                x0: 0.0,
                x1: 1.0,
                reason: "book_unavailable_before_capture".to_owned(),
            }),
            Some(_) => {}
        }
    }
    gaps.sort_by(|a, b| a.x0.total_cmp(&b.x0).then_with(|| a.x1.total_cmp(&b.x1)));

    SettledProjection {
        enabled: true,
        summarized: summarizing,
        floored_quantity,
        cells: Arc::new(cells),
        aggressions,
        liquidity_events,
        gaps: Arc::new(gaps),
        effective_grouping,
        liquidity_reference,
        aggression_reference,
        summary_reference,
        dropped_cells,
        folded_aggressions,
        dropped_liquidity_events,
        live_from_ms,
        live_events,
    }
}

/// Build the marks of the part of the chart that is still moving: the prints
/// rolling through the lane, and the bar still taking orders.
///
/// Cheap by construction — it only ever touches the prints after
/// [`SettledProjection::live_from_ms`] — so a caller may run it on every frame
/// and have a print reach the screen in the frame after it arrived.
///
/// `settled` supplies the two size scales, so a bubble drawn here reads on
/// exactly the scale the settled bubbles beside it were drawn on. Those scales
/// are as old as the settled half; a print large enough to move them therefore
/// resizes the chart when that half is next rebuilt, not the instant it lands.
#[must_use]
pub fn project_live(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    settled: &SettledProjection,
) -> LiveMarks {
    let config = history.config();
    if !settled.enabled || !config.any_layer_enabled() {
        return LiveMarks::default();
    }
    let live_now_x = timeline
        .live_now_position()
        .map(|position| position.normalized);
    // No boundary means no bar is represented at all, so there is no live half
    // to draw — and no reason to walk the retained tape looking for one.
    //
    // Hidden bubbles are *not* a reason to skip this: a reduction is labelled
    // as aggression-aligned by the print that explains it, so the prints are
    // still clustered and matched, and only the marks are dropped later.
    let Some(live_from_ms) = settled.live_from_ms else {
        return LiveMarks {
            live_now_x,
            ..LiveMarks::default()
        };
    };
    // Literally the same rule as the settled half. It used to also require
    // both sides visible, so hiding one side moved the seam's marks between
    // pies and raw prints — two halves disagreeing about what a bar is. The
    // renderer is where a one-sided frame is handled (`RenderContext::bubbles`
    // refuses to draw a two-sided mark while a side is hidden).
    let summarizing = config.bubble_candle_summary;
    // Same rule as the settled half: this is the *tape's* projection, so a
    // switch on the candles may not empty it.
    let coverage: Vec<_> = if config.depth_visible_anywhere() {
        history.coverage_segments().cloned().collect()
    } else {
        Vec::new()
    };
    let mut tier = cluster_tier(
        history,
        timeline,
        prices,
        &coverage,
        TierGrouping {
            slots: settled.effective_grouping,
            lane: lane_grouping(config),
        },
        TierCut {
            range: (Some(live_from_ms), None),
            tape_from_ms: timeline.lane_start_ms(),
        },
        summarizing,
    );
    let mut events = settled.live_events.clone();
    correlate_tier(&mut events, &mut tier, config, summarizing);
    let dropped_liquidity_events = filter_events(&mut events, config, settled.liquidity_reference);
    let (marks, floored_quantity) = refine_tier(
        tier,
        config,
        settled.aggression_reference,
        timeline,
        settled.effective_grouping,
        summarizing,
    );
    // This half carries marks for *both* panes: the prints rolling through the
    // tape, and — while the summary is on — the forming bar's own slot marks.
    // They are folded apart, because a fold that mixed them would draw one
    // pane's volume inside the other: the renderer clips by `x`, sizes by the
    // pane's own radius range and gates on the pane's own switch, so a merged
    // mark would be clipped into one pane, sized for the other, and hidden by
    // the wrong control.
    let (mut tape_marks, mut slot_marks): (Vec<_>, Vec<_>) = tier_primitives(
        marks,
        timeline,
        prices,
        settled.aggression_reference,
        settled.summary_reference,
    )
    .into_iter()
    .partition(|mark| mark.live);
    let before = tape_marks.len() + slot_marks.len();
    let (chart_budget, lane_budget) =
        pane_budgets(config.max_aggression_primitives, &config.live_lane);
    fold_to_budget(
        &mut tape_marks,
        lane_budget,
        FoldOrder::OldestFirst,
        settled.aggression_reference,
        None,
    );
    // The forming bar's marks are candle marks and answer to the candles'
    // budget and the candles' ranking. Sized on the summary scale when there is
    // one, for the same reason `tier_primitives` drew them on it: a pie carries
    // a whole bar and a tape mark carries one print, so folding a pie against
    // the print scale would peg it at the largest radius.
    fold_to_budget(
        &mut slot_marks,
        chart_budget,
        FoldOrder::SmallestFirst,
        settled.summary_reference,
        Some(timeline),
    );
    tape_marks.append(&mut slot_marks);
    let folded_aggressions = before.saturating_sub(tape_marks.len());

    LiveMarks {
        aggressions: tape_marks,
        liquidity_events: event_primitives(events, timeline, prices, settled.effective_grouping),
        dropped_liquidity_events,
        folded_aggressions,
        floored_quantity,
        live_now_x,
    }
}

/// Allocate `events` to the prints of one half of the chart.
///
/// Each half is handed only the reductions timestamped inside it, so a removed
/// quantity is claimed as evidence exactly once however the chart is cut.
fn correlate_tier(
    events: &mut [LiquidityEvent],
    tier: &mut TierClusters,
    config: &HeatmapConfig,
    summarizing: bool,
) {
    if summarizing {
        // The views overlap, so they are correlated apart: neither may allocate
        // the same reduction twice within itself, and a print drawn in both
        // places carries the same fact in both. The markers themselves keep the
        // slot pass's numbers, which cover every print of every bar.
        correlate_liquidity(
            &mut events.to_vec(),
            &mut tier.tape,
            config.liquidity_correlation_ms,
        );
        correlate_liquidity(events, &mut tier.slot, config.liquidity_correlation_ms);
    } else {
        // Disjoint: one pass over both, so a reduction is allocated across the
        // whole visible tape exactly as it always was. `correlate_liquidity`
        // mutates in place without reordering, so the halves split back apart.
        let tape_len = tier.tape.len();
        let mut both = std::mem::take(&mut tier.tape);
        both.append(&mut tier.slot);
        correlate_liquidity(events, &mut both, config.liquidity_correlation_ms);
        tier.slot = both.split_off(tape_len);
        tier.tape = both;
    }
}

/// Drop the reductions not worth a marker, and report how many the safety cap
/// took with them.
fn filter_events(
    events: &mut Vec<LiquidityEvent>,
    config: &HeatmapConfig,
    liquidity_reference: Decimal,
) -> usize {
    // Display floors: a busy book shrinks buckets constantly, and a marker per
    // wiggle is violet drizzle. An unattributed pull must be deep (fraction of
    // its level, or a full pull) AND big (share of the visible full-intensity
    // reference) to draw. Aggression-aligned reductions are exempt from the
    // floors, and the safety cap below keeps them ahead of unattributed ones,
    // so a bubble can only point at a hidden event in the extreme case where
    // aligned events alone exceed the cap (reported via dropped counters).
    let pull_floor = Decimal::from_f32(config.min_unattributed_pull_share)
        .map(|share| liquidity_reference * share)
        .unwrap_or(Decimal::ZERO);
    events.retain(|event| {
        if matches!(event.evidence, LiquidityEvidence::AggressionAligned) {
            return true;
        }
        (event.full_removal || event.fraction >= config.min_unattributed_reduction)
            && event.removed >= pull_floor
    });

    // Per-layer display switches, applied after correlation so bubbles keep
    // their matched evidence (and their consumption marks) even when the
    // depletion markers themselves are hidden.
    events.retain(|event| match event.evidence {
        LiquidityEvidence::AggressionAligned => config.show_aligned_depletion,
        LiquidityEvidence::DepthOnly => config.show_unattributed_reductions,
    });

    let dropped = events.len().saturating_sub(config.max_visible_cells);
    if dropped > 0 {
        events.sort_by_key(|event| {
            event_cap_key(
                event.evidence,
                event.removed,
                event.timestamp_ms,
                event.event_id,
            )
        });
        events.truncate(config.max_visible_cells);
    }
    dropped
}

/// How the safety cap ranks reductions, wherever it is applied: aligned
/// evidence first, because it is what a bubble points at, then the biggest
/// reduction, then time and id so the same book always yields the same markers.
fn event_cap_key(
    evidence: LiquidityEvidence,
    removed: Decimal,
    timestamp_ms: i64,
    event_id: u64,
) -> (Reverse<bool>, Reverse<Decimal>, i64, u64) {
    (
        Reverse(matches!(evidence, LiquidityEvidence::AggressionAligned)),
        Reverse(removed),
        timestamp_ms,
        event_id,
    )
}

/// Keep the strongest `limit` reductions of a whole frame.
fn cap_events(events: &mut Vec<LiquidityEventPrimitive>, limit: usize) {
    if events.len() <= limit {
        return;
    }
    events.sort_by_key(|event| {
        event_cap_key(
            event.evidence,
            event.removed,
            event.timestamp_ms,
            event.event_id,
        )
    });
    events.truncate(limit);
}

/// A total order over sides, so a fold can sort and group by one.
/// `AggressorSide` is a fact about a print, not a rank, so it carries no `Ord`
/// of its own.
fn side_key(side: AggressorSide) -> u8 {
    match side {
        AggressorSide::Buy => 0,
        AggressorSide::Sell => 1,
    }
}

/// Split the frame's bubble budget between the two panes.
///
/// The two panes draw different things out of the same budget: the candles
/// draw a compressed history whose marks each carry a bar, the tape draws the
/// newest prints one by one. Ranked against each other by quantity — which is
/// what a single shared budget did — the tape lost every time, and zooming the
/// candles out emptied it. So the budget is split before anything is ranked,
/// and each pane spends its own.
///
/// The split is the tape's own width share ([`LiveLaneStyle::width_share`]),
/// not a number of its own: marks need room to be read, so the pane with a
/// third of the canvas gets a third of the marks, and a trader who drags the
/// divider moves both together.
///
/// With the tape switched off there is no second pane, and the candles get the
/// whole budget — reserving a share for a band nobody is drawing would fold the
/// candles harder to protect nothing.
///
/// Both shares are at least two whenever the tape is on, which is what lets a
/// pane always fit: two marks is one per side, and a fold never crosses sides.
fn pane_budgets(limit: usize, lane: &LiveLaneStyle) -> (usize, usize) {
    if !lane.enabled {
        return (limit, 0);
    }
    let share = if lane.width_share.is_finite() {
        lane.width_share
            .clamp(MIN_LIVE_LANE_SHARE, MAX_LIVE_LANE_SHARE)
    } else {
        DEFAULT_LIVE_LANE_SHARE
    };
    let lane_budget = ((limit as f32) * share).round().max(0.0) as usize;
    let lane_budget = lane_budget.clamp(2, limit.saturating_sub(2).max(2));
    (limit.saturating_sub(lane_budget).max(2), lane_budget)
}

/// Fold `other` into `mark`, conserving every exact quantity it carried.
///
/// `mark` is always the group's heaviest member — [`fold_chunk`] puts it there
/// — so the merged mark keeps the position of the print that actually carries
/// the volume. A quantity-weighted midpoint would land between the members: a
/// bubble at a price and an instant where nothing traded at all, and a
/// fabricated fact is worse than the omission this fold replaced. The regional
/// fold settled the same question the same way — `finish_regional` refuses the
/// fold-wide average and anchors at the point of control.
///
/// The declared price band, by contrast, covers *every* member: a consumer that
/// draws the range (the live strip's histogram) has to be told the whole zone
/// the folded quantity came from, not just the winner's row.
fn merge_marks(
    mark: &mut AggressionPrimitive,
    other: &mut AggressionPrimitive,
    reference: Decimal,
) {
    debug_assert_eq!(mark.live, other.live, "a fold may not cross the panes");
    debug_assert_eq!(mark.side, other.side, "a fold may not cross sides");
    let total = mark.quantity + other.quantity;
    let low = mark.price_bucket.min(other.price_bucket);
    let high = (mark.price_bucket + mark.price_span).max(other.price_bucket + other.price_span);
    let total_f64 = total.to_f64().unwrap_or(0.0);
    // A fold of folds counts the marks underneath it, not the folds: a virgin
    // mark stands for itself, so it enters the sum as one.
    mark.folded_marks = mark
        .folded_marks
        .max(1)
        .saturating_add(other.folded_marks.max(1));
    mark.buy_share = if total_f64 > 0.0 {
        ((f64::from(mark.buy_share) * mark.quantity.to_f64().unwrap_or(0.0)
            + f64::from(other.buy_share) * other.quantity.to_f64().unwrap_or(0.0))
            / total_f64) as f32
    } else {
        mark.buy_share
    };
    mark.quantity = total;
    mark.trade_count = mark.trade_count.saturating_add(other.trade_count);
    mark.first_timestamp_ms = mark.first_timestamp_ms.min(other.first_timestamp_ms);
    mark.last_timestamp_ms = mark.last_timestamp_ms.max(other.last_timestamp_ms);
    mark.matched_quantity += other.matched_quantity;
    mark.matched_fraction = if total > Decimal::ZERO {
        (mark.matched_quantity / total)
            .to_f64()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    // Moved, never cloned, and never sorted here: a fold absorbs many marks in
    // a row, and sorting the growing list once per absorption is quadratic — it
    // was worth 25x the frame cost on a dense tape. `settle_ids` tidies each
    // finished fold exactly once instead.
    mark.agg_ids.append(&mut other.agg_ids);
    mark.agg_id = mark.agg_id.min(other.agg_id);
    mark.liquidity_event_ids
        .append(&mut other.liquidity_event_ids);
    if mark.generation != other.generation {
        mark.generation = None;
    }
    mark.price_bucket = low;
    mark.price_span = (high - low).max(mark.price_span);
    // The fold carries more quantity, so it has to read bigger. Sized against
    // the reference the pane was already drawn on — merging is a drawing
    // decision and must not rescale the marks it left alone.
    mark.size = normalized_area_size(mark.quantity, reference);
}

/// Ids a single fold keeps, before it stops recording which prints it stands
/// for and lets [`AggressionPrimitive::trade_count`] speak for them.
///
/// A fold is unbounded in principle — a quiet budget on a busy session can put
/// a whole minute of prints under one mark — and the id lists are cloned into
/// every published frame. The exact count is never lost, only the roll of
/// individual ids past this point, and the truncation is declared by
/// `trade_count` exceeding `agg_ids.len()`.
const MAX_FOLD_IDS: usize = 256;

/// Put one finished fold's id lists back in order, once, and bound them.
fn settle_ids(mark: &mut AggressionPrimitive) {
    mark.agg_ids.sort_unstable();
    mark.agg_ids.dedup();
    mark.agg_ids.truncate(MAX_FOLD_IDS);
    mark.liquidity_event_ids.sort_unstable();
    mark.liquidity_event_ids.dedup();
    mark.liquidity_event_ids.truncate(MAX_FOLD_IDS);
}

/// Merge one group of compatible marks into a single mark anchored on the
/// heaviest of them — the group's point of control.
fn fold_chunk(mut chunk: Vec<AggressionPrimitive>, reference: Decimal) -> AggressionPrimitive {
    let heaviest = chunk
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            a.quantity
                .cmp(&b.quantity)
                .then_with(|| b.agg_id.cmp(&a.agg_id))
        })
        .map_or(0, |(index, _)| index);
    chunk.swap(0, heaviest);
    let mut rest = chunk.split_off(1);
    let mut merged = chunk.pop().expect("a chunk is never empty");
    for other in &mut rest {
        merge_marks(&mut merged, other, reference);
    }
    if merged.folded_marks > 0 {
        settle_ids(&mut merged);
    }
    merged
}

/// Which mark folds first, per pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FoldOrder {
    /// The candles fold their *smallest* marks together. A trader reads the
    /// big prints off the compressed history; the small ones are the ones a
    /// wider zoom was always going to blur, and folding them keeps the marks
    /// that carry the story untouched.
    SmallestFirst,
    /// The tape folds its *oldest* marks together. Its whole reason to exist
    /// is the newest prints, one by one, at the right edge — so pressure on
    /// the budget is paid at the left edge, where a print was leaving anyway.
    OldestFirst,
}

/// Bring a pane inside its budget without deleting a single contract.
///
/// This is the mission's invariant in code. The old behaviour ranked marks by
/// quantity and truncated, so a print that traded could leave the canvas with
/// nothing said — and which prints left changed with the zoom, because the
/// zoom changes how many marks there are to rank. Now the excess is *folded*:
/// compatible neighbours merge into one bigger mark carrying the exact summed
/// quantity and the union of their evidence, and the sum of the ink is the sum
/// of the tape whatever the budget.
///
/// The result can sit *above* the budget, deliberately. A fold may not cross a
/// side, a pane, or a bar, and with more of those groups than the budget has
/// marks the only way further down is to misattribute volume. The budget is a
/// performance target; correctness outranks it.
fn fold_to_budget(
    marks: &mut Vec<AggressionPrimitive>,
    limit: usize,
    order: FoldOrder,
    reference: Decimal,
    bars: Option<&BarTimeline>,
) {
    let limit = limit.max(2);
    if marks.len() <= limit {
        return;
    }
    // Ranked by what the pane is willing to lose resolution on first, across
    // every group: the candles rank by size, the tape by age.
    match order {
        FoldOrder::SmallestFirst => marks.sort_by(|a, b| {
            a.quantity
                .cmp(&b.quantity)
                .then_with(|| a.first_timestamp_ms.cmp(&b.first_timestamp_ms))
                .then_with(|| side_key(a.side).cmp(&side_key(b.side)))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        }),
        FoldOrder::OldestFirst => marks.sort_by(|a, b| {
            a.first_timestamp_ms
                .cmp(&b.first_timestamp_ms)
                .then_with(|| a.quantity.cmp(&b.quantity))
                .then_with(|| side_key(a.side).cmp(&side_key(b.side)))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.agg_id.cmp(&b.agg_id))
        }),
    }

    // Only the front of that ranking is touched, and the tail comes through
    // untouched — which is the whole point of ranking. On the candles the big
    // prints a trader is reading stay exactly as they were; on the tape the
    // newest prints at the right edge stay one mark per execution, and the
    // pressure is paid at the left edge where a print was leaving anyway.
    let excess = marks.len() - limit;
    let (pool_len, target) = if excess * 2 <= marks.len() {
        // Twice the excess folded two-at-a-time is the smallest pool that fits,
        // so the loss is spread thin and most of the pane is untouched.
        (excess * 2, excess)
    } else {
        // Far past budget: everything folds, in even groups.
        (marks.len(), limit)
    };

    let mut ranked = std::mem::take(marks);
    let tail = ranked.split_off(pool_len);
    // What a fold may never mix. Pane, because a tape mark and a candle mark
    // are clipped, sized and switched on and off separately — merging them
    // draws one pane's volume inside the other. Side, because a buy and a sell
    // are not one pressure. Bar, because a mark drawn inside a bar's slot is a
    // claim that *that* bar took the volume it carries, and merging a
    // neighbour's prints into it is a fabricated fact about who traded when —
    // the same reason `regionalize_clusters` keeps `bar_index` in its key. The
    // tape has no bars to confuse: it is one continuous band, so its marks key
    // on `None` and the whole pane is one group per side.
    let mut groups: BTreeMap<(bool, u8, Option<usize>), Vec<AggressionPrimitive>> = BTreeMap::new();
    for mark in ranked {
        let bar = if mark.live {
            None
        } else {
            bars.and_then(|timeline| {
                timeline
                    .locate(mark.first_timestamp_ms)
                    .map(|position| position.bar_index)
            })
        };
        groups
            .entry((mark.live, side_key(mark.side), bar))
            .or_default()
            .push(mark);
    }

    // Enough marks per fold that the pane fits, given how many groups there
    // are: two part-full groups are two marks, not one, and a budget that
    // ignored that would be quietly overspent. When the groups alone already
    // outnumber the target no group size can reach it, so the search stops
    // rather than walking `group` up to `pool_len` one step at a time.
    let mut group = pool_len.div_ceil(target.max(1)).max(2);
    if groups.len() <= target.max(1) {
        let folds_at = |size: usize| -> usize {
            groups
                .values()
                .map(|members| members.len().div_ceil(size))
                .sum()
        };
        while group < pool_len && folds_at(group) > target.max(1) {
            group += 1;
        }
    }

    let mut folded: Vec<AggressionPrimitive> = Vec::with_capacity(limit + 2);
    for mut members in groups.into_values() {
        while members.len() > group {
            let rest = members.split_off(group);
            folded.push(fold_chunk(members, reference));
            members = rest;
        }
        folded.push(fold_chunk(members, reference));
    }
    folded.extend(tail);
    *marks = folded;
}

/// Place reductions on the chart.
fn event_primitives(
    events: Vec<LiquidityEvent>,
    timeline: &BarTimeline,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
) -> Vec<LiquidityEventPrimitive> {
    events
        .into_iter()
        .filter_map(|event| {
            let x = timeline.locate(event.timestamp_ms)?.normalized;
            let clipped_low = event.price_bucket.max(prices.low);
            let clipped_high = (event.price_bucket + grouping.bucket_width).min(prices.high);
            let y0 = prices.y(clipped_high)?;
            let y1 = prices.y(clipped_low)?;
            (y1 > y0).then_some(LiquidityEventPrimitive {
                event_id: event.event_id,
                generation: event.generation,
                side: event.side,
                price_bucket: event.price_bucket,
                timestamp_ms: event.timestamp_ms,
                before: event.before,
                after: event.after,
                removed: event.removed,
                fraction: event.fraction,
                full_removal: event.full_removal,
                matched_quantity: event.matched_quantity,
                matched_fraction: event.matched_fraction,
                evidence: event.evidence,
                x,
                y0,
                y1,
            })
        })
        .collect()
}

/// The price resolution the tape clusters on.
///
/// Native, always: the adaptive display grouping is a *compression* device for
/// the candles, where a wide window has to fit many bars into few pixels, and
/// resolving it against the visible price span is what made the candles' zoom
/// decide which of the tape's prints fused together. The tape is not
/// compressed — it has the room to draw prints one by one, which is the whole
/// reason a scalper reads it — so it clusters at capture resolution and stays
/// the same picture through every zoom of the pane beside it.
fn lane_grouping(config: &HeatmapConfig) -> EffectiveGrouping {
    // Only the adaptive mode is overridden. `Adaptive` resolves against the
    // visible price span, which is how the candles' zoom came to decide which
    // of the tape's prints fused together — that is the leak. `Multiple(n)` is
    // the trader saying "give me rows this wide" and has nothing to do with
    // zoom, so it is obeyed on the tape as it is on the candles; overriding it
    // would leave an illiquid instrument readable on one pane and not the
    // other, with no control and no explanation.
    let display = match config.display_grouping {
        DisplayGrouping::Adaptive { .. } => DisplayGrouping::Native,
        chosen => chosen,
    };
    EffectiveGrouping::resolve(display, config.price_grouping, Decimal::ZERO)
}

/// Where one tier is cut out of the retained tape.
///
/// The two questions travel together because they are the same decision seen
/// from both ends: which prints this tier owns at all, and which of the ones it
/// owns belong on the tape rather than in a bar slot.
#[derive(Debug, Clone, Copy)]
struct TierCut {
    /// Half-open `[from, until)` in exchange milliseconds.
    range: (Option<i64>, Option<i64>),
    // Where the tape begins, for the half that owns the tape — `None` for the
    // settled half, which owns none of it. Carried here rather than read off
    // the timeline because "am I on the tape?" is only a question for the live
    // half: the settled half asking it made its *content* depend on the lane's
    // left edge, and that edge moves with every print. The cached half would
    // have had to be rebuilt every frame to stay truthful, and the one that was
    // not rebuilt drew a print in a bar slot while the live half drew the same
    // print on the tape.
    tape_from_ms: Option<i64>,
}

/// The price resolutions the two views cluster on.
///
/// One type rather than two loose arguments, because the pair *is* the rule:
/// the compressed slots fuse prints into whatever visual row the candles'
/// zoom resolved, and the tape never does.
#[derive(Debug, Clone, Copy)]
struct TierGrouping {
    /// What the bar slots fuse prints into: the adaptive display grouping.
    slots: EffectiveGrouping,
    /// What the tape fuses prints into: capture resolution, always.
    lane: EffectiveGrouping,
}

/// The clustered prints of one stretch of the chart, in the two views a print
/// can be drawn in.
struct TierClusters {
    /// Raw prints, placed by the live edge they are measured from.
    tape: Vec<AggressionCluster>,
    /// Prints read against the bar they belong to.
    slot: Vec<AggressionCluster>,
}

/// Cluster the retained prints timestamped inside `range`, as `[from, until)`.
///
/// The whole retained tape is walked, but the range is tested first and it is
/// two integer comparisons: the half that runs every frame pays the per-print
/// cost — locating it, placing it, clustering it — only for its own prints.
/// The walk is deliberately linear rather than a search inward from the newest
/// print: the retained tape is only *almost* ordered by timestamp, and one
/// print delivered out of order must not be able to hide every print behind
/// it.
fn cluster_tier(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    coverage: &[CoverageSegment],
    grouping: TierGrouping,
    cut: TierCut,
    summarizing: bool,
) -> TierClusters {
    let config = history.config();
    let TierCut {
        range: (from_ms, until_ms),
        tape_from_ms,
    } = cut;
    let mut tape_prints = Vec::new();
    let mut slot_prints = Vec::new();
    for trade in history.aggressions() {
        if from_ms.is_some_and(|from| trade.timestamp_ms < from)
            || until_ms.is_some_and(|until| trade.timestamp_ms >= until)
        {
            continue;
        }
        if timeline.locate(trade.timestamp_ms).is_none() || prices.y(trade.price).is_none() {
            continue;
        }
        let on_tape = tape_from_ms.is_some_and(|start| trade.timestamp_ms >= start);
        if on_tape {
            tape_prints.push(trade);
        }
        // Exactly one pane draws a print, and which one is the tape's window:
        // while a print is inside it the tape has it, and when it falls out of
        // the window it lands in the slot of the bar it happened in. Widening
        // the tape therefore *moves* marks from the candles to the tape and
        // never deletes one — the alternative, drawing the print in both, puts
        // one execution on the canvas twice, which reads as two trades and is
        // the dishonesty this whole change exists to remove. The summary is the
        // one exception: a pie is an aggregate of the bar, not a second copy of
        // a print, so the bar keeps counting prints the tape is still showing.
        if summarizing || !on_tape {
            slot_prints.push(trade);
        }
    }

    // Each view clusters on its own window: the tape has room the compressed
    // slots do not, and the split is also what keeps a cluster from straddling
    // the boundary between them.
    TierClusters {
        tape: cluster_aggressions(
            tape_prints,
            coverage,
            grouping.lane,
            config.live_lane.effective_cluster_ms(
                config.bubble_cluster_ms,
                // No lane means no tape prints to cluster, so the reference
                // only has to be a number the scale can divide by.
                timeline.lane_reference_ms().unwrap_or(1),
            ),
        ),
        slot: cluster_aggressions(
            slot_prints,
            coverage,
            grouping.slots,
            config.bubble_cluster_ms,
        ),
    }
}

/// Apply the display floors, fold what is too small to read, and summarize.
fn refine_tier(
    mut tier: TierClusters,
    config: &HeatmapConfig,
    print_reference: Decimal,
    timeline: &BarTimeline,
    grouping: EffectiveGrouping,
    summarizing: bool,
) -> (TierClusters, Decimal) {
    let regionalizing = config.bubble_region_rows > 1;
    // What the trader's own display floor takes off the canvas. Counted rather
    // than merely applied: it is the one discard left in this pipeline, it is a
    // setting the trader chose, and a setting that quietly removes contracts
    // without saying how many is the same dishonesty the budget just stopped
    // committing.
    let mut floored = Decimal::ZERO;

    // Display floor for bubbles. Applied after association so a hidden small
    // print still counts as the evidence behind an aligned reduction: the
    // marker keeps saying "a trade ate this", the tape just stays readable.
    // With the regional fold on, the slot's floor waits until after the fold:
    // a region's area claims the zone's summed volume, and a floor applied to
    // the members would silently thin the very sum the mark reports — small
    // prints fold into their region, and only still-small *regions* are
    // hidden.
    if let Some(floor) = config.bubbles.min_quantity_decimal() {
        tier.tape.retain(|cluster| {
            let kept = cluster.quantity >= floor;
            if !kept {
                floored += cluster.quantity;
            }
            kept
        });
        if !regionalizing {
            tier.slot.retain(|cluster| {
                let kept = cluster.quantity >= floor;
                if !kept {
                    floored += cluster.quantity;
                }
                kept
            });
        }
    }

    // Readability floor, then the closed-bar summary. Association already
    // happened, so folding moves no evidence: a merged bubble carries the
    // summed quantity and the union of the event ids its parts pointed at.
    // Sized against the reference computed above, which is deliberately *not*
    // recomputed — merging is a drawing decision and must not rescale the
    // bubbles that were already readable.
    if let Some(dust) = config.bubbles.dust_quantity(print_reference) {
        tier.tape = merge_dust_clusters(tier.tape, dust, config.bubble_dust_merge_ms);
        tier.slot = merge_dust_clusters(tier.slot, dust, config.bubble_dust_merge_ms);
    }

    // Which bar slot a cluster falls in — the key the regional fold and the
    // summary both group by, so a fold can never credit one bar with volume
    // its neighbour traded.
    let bar_of = |cluster: &AggressionCluster| {
        timeline
            .locate(cluster.timestamp_ms)
            .map(|position| position.bar_index)
    };

    // The regional fold, above one row: same-side clusters sharing a region
    // `bubble_region_rows` rows tall, inside one bar, become one bubble
    // anchored at the region's point of control. After association and after
    // the dust merge for the same reason those run where they do — folding is
    // a drawing decision, and by now it moves no evidence and rescales
    // nothing. The summary below then works at region granularity, because
    // the fold rewrote each cluster's bucket to its region's lower edge.
    //
    // Slots only, never the tape: the live lane draws prints one by one
    // because it has the room to, and a scalper reads the forming edge from
    // exactly that granularity — a region there would hold the newest print
    // hostage to a window that has not closed. The compressed history is
    // where per-row marks stack into bead necklaces, so the history is where
    // the fold pays.
    if regionalizing {
        let region_width = grouping.bucket_width * Decimal::from(config.bubble_region_rows);
        tier.slot = regionalize_clusters(tier.slot, region_width, config.bubble_region_ms, bar_of);
        if let Some(floor) = config.bubbles.min_quantity_decimal() {
            tier.slot.retain(|cluster| {
                let kept = cluster.quantity >= floor;
                if !kept {
                    floored += cluster.quantity;
                }
                kept
            });
        }
    }

    // Every bar with prints in its slot gets one mark per price range, the
    // forming one included: its pie is a running total that grows with each
    // order, which is how the compressed left side says what is happening now
    // rather than only what already happened. It is honest because it is
    // exactly what the bar has taken so far — and the tape beside it still
    // shows those same prints one by one.
    if summarizing {
        tier.slot = summarize_clusters(std::mem::take(&mut tier.slot), bar_of);
    }
    (tier, floored)
}

/// Place one tier's marks on the chart, each on the scale its view reads on.
fn tier_primitives(
    marks: TierClusters,
    timeline: &BarTimeline,
    prices: PriceWindow,
    print_reference: Decimal,
    summary_reference: Decimal,
) -> Vec<AggressionPrimitive> {
    marks
        .tape
        .into_iter()
        .map(|cluster| (cluster, true, print_reference))
        .chain(
            marks
                .slot
                .into_iter()
                .map(|cluster| (cluster, false, summary_reference)),
        )
        .filter_map(|(cluster, live, reference)| {
            // A tape mark is placed by the live edge it is measured from; a
            // slot mark by the bar it belongs to. `locate` answers the first,
            // so a settled mark has to ask for its bar's slot explicitly.
            let position = if live {
                timeline.locate(cluster.timestamp_ms)?
            } else {
                timeline.locate_in_slot(cluster.timestamp_ms)?
            };
            let y = prices.y(cluster.price)?;
            let size = normalized_area_size(cluster.quantity, reference);
            Some(aggression_primitive(
                cluster,
                position.normalized,
                y,
                size,
                live,
            ))
        })
        .collect()
}

fn aggression_primitive(
    cluster: AggressionCluster,
    x: f64,
    y: f64,
    size: f32,
    live: bool,
) -> AggressionPrimitive {
    let matched_fraction = cluster.matched_fraction();
    let buy_share = cluster.buy_share();
    AggressionPrimitive {
        agg_id: cluster.agg_id,
        agg_ids: cluster.agg_ids,
        generation: cluster.generation,
        side: cluster.side,
        consumed_side: cluster.consumed_side,
        quantity: cluster.quantity,
        buy_share,
        live,
        price_bucket: cluster.price_bucket,
        price_span: cluster.price_span,
        trade_count: cluster.trade_count,
        first_timestamp_ms: cluster.first_timestamp_ms,
        last_timestamp_ms: cluster.last_timestamp_ms,
        matched_quantity: cluster.matched_quantity,
        matched_fraction,
        liquidity_event_ids: cluster.liquidity_event_ids,
        x,
        y,
        size,
        folded_marks: 0,
    }
}

fn percentile_99(values: impl Iterator<Item = Decimal>) -> Decimal {
    let mut positive: Vec<Decimal> = values
        .filter(|quantity| *quantity > Decimal::ZERO)
        .collect();
    if positive.is_empty() {
        return Decimal::ZERO;
    }
    positive.sort_unstable();
    let rank = (99 * positive.len()).div_ceil(100);
    positive[rank.saturating_sub(1)]
}

/// Shared with the live strip (`crate::live_strip`), which normalizes its
/// depth rows against the same reference the heatmap cells used, so one wall
/// reads with one colour on both sides of the chart edge.
pub(crate) fn normalized_log_intensity(quantity: Decimal, reference: Decimal, gamma: f32) -> f32 {
    if quantity <= Decimal::ZERO || reference <= Decimal::ZERO {
        return 0.0;
    }
    let ratio = (quantity / reference).to_f64().unwrap_or(0.0).max(0.0);
    let logarithmic = ((1.0 + 9.0 * ratio).ln() / 10.0_f64.ln()).clamp(0.0, 1.0);
    logarithmic.powf(f64::from(gamma)) as f32
}

/// Shared with the live strip's aggression histogram, which sizes its bars by
/// the same square-root area rule the bubbles use — twice the quantity reads
/// as twice the ink, on both.
pub(crate) fn normalized_area_size(quantity: Decimal, reference: Decimal) -> f32 {
    if quantity <= Decimal::ZERO || reference <= Decimal::ZERO {
        return 0.0;
    }
    (quantity / reference)
        .to_f64()
        .unwrap_or(0.0)
        .clamp(0.0, 1.0)
        .sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderflow::config::{
        BubbleSizeReference, BubbleStyle, DisplayGrouping, LiveLaneStyle,
    };
    use crate::orderflow::history::LiquidityHistory;
    use quantick_engine::{Bar, Side, Trade};
    use quantick_orderbook::BookSide;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    /// The live edge as the chart supplies it while the newest bar is on
    /// screen: the lane shows the recent bars' typical duration, unzoomed.
    fn live(now_ms: i64, closed: &[Bar]) -> Option<crate::orderflow::LiveEdge> {
        Some(crate::orderflow::LiveEdge {
            now_ms,
            window_ms: crate::orderflow::reserved_span_ms(closed),
            reference_ms: crate::orderflow::reserved_span_ms(closed),
            on_newest_bar: true,
        })
    }

    fn level(price: &str, quantity: &str) -> BookLevel {
        BookLevel::new(dec(price), dec(quantity)).unwrap()
    }

    fn snapshot(update_id: u64) -> BookSnapshot {
        BookSnapshot::new(
            update_id,
            vec![level("99", "2"), level("100", "3")],
            vec![level("101", "4"), level("102", "5")],
            BookCoverage::Full,
        )
    }

    fn bar(open_ms: i64, close_ms: i64) -> Bar {
        Bar {
            open_time: open_ms,
            close_time: close_ms,
            open: dec("100"),
            high: dec("101"),
            low: dec("99"),
            close: dec("100"),
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    fn config() -> HeatmapConfig {
        HeatmapConfig {
            enabled: true,
            show_aggressions: true,
            price_grouping: Decimal::ONE,
            ..HeatmapConfig::default()
        }
    }

    /// The reduction cap is a budget for the whole frame too. Each half caps
    /// itself where it is built, so without a cap over the join a frame could
    /// draw twice the markers the budget allows — one full cap per half.
    #[test]
    fn the_reduction_cap_is_one_budget_for_both_halves() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            max_visible_cells: 2,
            min_unattributed_reduction: 0.0,
            min_unattributed_pull_share: 0.0,
            ..HeatmapConfig::default()
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        // Two pulls each side of the seam at 2 000 ms.
        for (update_id, timestamp_ms, bids, asks) in [
            (11_u64, 300_i64, vec![level("100", "1")], vec![]),
            (12, 900, vec![level("99", "0.5")], vec![]),
            (13, 2_300, vec![], vec![level("101", "1")]),
            (14, 2_900, vec![], vec![level("102", "1")]),
        ] {
            history
                .apply_delta(
                    timestamp_ms,
                    &BookDelta::new(update_id, update_id, bids, asks),
                )
                .unwrap();
        }
        history
            .apply_delta(3_900, &BookDelta::new(15, 15, vec![], vec![]))
            .unwrap();

        let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            None,
            Some(crate::orderflow::LiveEdge {
                now_ms: 3_900,
                window_ms: 1_500,
                reference_ms: 1_500,
                on_newest_bar: true,
            }),
        );
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );

        assert_eq!(
            projection.liquidity_events.len(),
            2,
            "a cap of two is two markers for the frame, not two per half"
        );
        assert_eq!(projection.dropped_liquidity_events, 2);
    }

    /// The primitive budget is *split* between the panes, not shared by them.
    ///
    /// One shared budget was the bug this mission exists to fix: the candles'
    /// marks each carry a bar and the tape's each carry a print, so ranking
    /// them against each other by quantity emptied the tape whenever the
    /// candles had more to draw. Each pane now folds against its own share,
    /// and — the part that matters to a trader — folding conserves, so the six
    /// contracts that traded are still six contracts of ink.
    #[test]
    fn the_budget_is_split_between_the_panes() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_cluster_ms: 0,
            bubble_dust_merge_ms: 0,
            max_aggression_primitives: 3,
            ..config()
        });
        history.install_snapshot(0, 1, snapshot(10)).unwrap();
        // Three prints each side of the seam at 2 000 ms, interleaved by size so
        // the winners cannot be picked by half.
        for (agg_id, timestamp_ms, quantity) in [
            (1_u64, 100_i64, "10"),
            (2, 300, "1"),
            (3, 1_100, "9"),
            (4, 2_100, "2"),
            (5, 3_100, "8"),
            (6, 3_300, "3"),
        ] {
            history.record_aggression(&Trade {
                agg_id,
                timestamp_ms,
                price: dec("101"),
                quantity: dec(quantity),
                side: Side::Buy,
            });
        }
        let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            None,
            Some(crate::orderflow::LiveEdge {
                now_ms: 3_900,
                window_ms: 1_500,
                reference_ms: 1_500,
                on_newest_bar: true,
            }),
        );
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );

        let drawn: Decimal = projection
            .aggressions
            .iter()
            .map(|mark| mark.quantity)
            .sum();
        assert_eq!(
            drawn,
            dec("33"),
            "10 + 1 + 9 + 2 + 8 + 3 traded, and all of it is still on the canvas"
        );
        assert!(
            projection.aggressions.iter().any(|mark| mark.live),
            "the tape keeps its own share however loud the candles are"
        );
        assert!(
            projection.aggressions.iter().any(|mark| !mark.live),
            "and so do the candles"
        );
        // No fold is even possible here, and that is the point: six prints
        // spread one per bar leave every (pane, side, bar) group holding a
        // single mark, and a fold may not cross any of those boundaries. The
        // frame carries more marks than the budget asked for rather than
        // misattribute volume — and it still carries every contract.
        assert_eq!(
            projection.folded_aggressions, 0,
            "nothing could be folded without crossing a bar, so nothing was"
        );
    }

    /// The regional fold compresses the settled history and never the tape:
    /// prints in the live lane keep their own marks — a scalper reads the
    /// forming edge print by print — while the same prices behind the seam
    /// become one bubble per region carrying the exact summed quantity.
    #[test]
    fn regions_fold_the_settled_half_and_leave_the_tape_alone() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_cluster_ms: 0,
            bubble_dust_merge_ms: 0,
            bubble_region_rows: 3,
            bubble_region_ms: 5_000,
            ..config()
        });
        history.install_snapshot(0, 1, snapshot(10)).unwrap();
        // Two prints in the settled half share the three-row region [99, 102);
        // two prints on the tape land in that same region.
        for (agg_id, timestamp_ms, price) in [
            (1_u64, 100_i64, "100"),
            (2, 300, "101"),
            (3, 3_100, "100"),
            (4, 3_300, "101"),
        ] {
            history.record_aggression(&Trade {
                agg_id,
                timestamp_ms,
                price: dec(price),
                quantity: Decimal::ONE,
                side: Side::Buy,
            });
        }
        let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            None,
            Some(crate::orderflow::LiveEdge {
                now_ms: 3_900,
                window_ms: 1_500,
                reference_ms: 1_500,
                on_newest_bar: true,
            }),
        );
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );

        let settled: Vec<_> = projection
            .aggressions
            .iter()
            .filter(|mark| !mark.live)
            .collect();
        assert_eq!(settled.len(), 1, "one region, one settled mark");
        assert_eq!(settled[0].quantity, dec("2"));
        assert_eq!(settled[0].trade_count, 2);
        let tape: Vec<_> = projection
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .collect();
        assert_eq!(tape.len(), 2, "the tape stays print by print");
        assert!(tape.iter().all(|mark| mark.quantity == Decimal::ONE));
    }

    /// Two prints in one region but adjacent bars, inside one region window,
    /// with the candle summary on: the WIN's tick bars close in seconds, and a
    /// fold that crossed the boundary would credit the whole summed quantity
    /// to the bar its midpoint lands in — a pie claiming volume the neighbour
    /// traded. One mark per bar, each with its own bar's quantity.
    #[test]
    fn regions_never_move_volume_across_a_bar_boundary() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_cluster_ms: 0,
            bubble_dust_merge_ms: 0,
            bubble_region_rows: 3,
            bubble_region_ms: 5_000,
            bubble_candle_summary: true,
            ..config()
        });
        history.install_snapshot(0, 1, snapshot(10)).unwrap();
        for (agg_id, timestamp_ms, price, quantity) in
            [(1_u64, 800_i64, "100", "1"), (2, 1_200, "102", "2")]
        {
            history.record_aggression(&Trade {
                agg_id,
                timestamp_ms,
                price: dec(price),
                quantity: dec(quantity),
                side: Side::Buy,
            });
        }
        let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            None,
            Some(crate::orderflow::LiveEdge {
                now_ms: 3_900,
                window_ms: 1_500,
                reference_ms: 1_500,
                on_newest_bar: true,
            }),
        );
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );

        let mut settled: Vec<_> = projection
            .aggressions
            .iter()
            .filter(|mark| !mark.live)
            .collect();
        settled.sort_by_key(|mark| mark.first_timestamp_ms);
        assert_eq!(settled.len(), 2, "one mark per bar, never one for both");
        assert_eq!(settled[0].quantity, dec("1"));
        assert_eq!(settled[1].quantity, dec("2"));
    }

    /// The chart is built in two halves that meet at a bar's open time. This is
    /// the seam: with the summary on, a bar whose prints landed on both sides of
    /// it must still draw *one* pie carrying all of them, never one pie per half
    /// — and the lane's raw prints must be neither dropped nor doubled.
    #[test]
    fn the_seam_between_the_halves_neither_splits_a_bar_nor_doubles_a_print() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_candle_summary: true,
            bubble_cluster_ms: 500,
            bubble_dust_merge_ms: 0,
            ..config()
        });
        history.install_snapshot(0, 1, snapshot(10)).unwrap();
        // Four bars of 1 000 ms, one print every 100 ms at one price.
        for step in 0..40_i64 {
            history.record_aggression(&Trade {
                agg_id: step as u64,
                timestamp_ms: step * 100 + 50,
                price: dec("101"),
                quantity: Decimal::ONE,
                side: Side::Buy,
            });
        }
        let closed: Vec<Bar> = (0..4).map(|i| bar(i * 1_000, i * 1_000 + 999)).collect();
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

        // Walk the live edge across a whole bar, so the seam lands at every
        // offset inside it — including exactly on a bar boundary.
        for now_ms in [3_400_i64, 3_500, 3_600, 3_999, 4_000] {
            let timeline = BarTimeline::from_bars(
                0,
                &closed,
                None,
                Some(crate::orderflow::LiveEdge {
                    now_ms,
                    // A lane wide enough to cover more than one bar, which is
                    // what makes the seam fall inside a bar rather than on it.
                    window_ms: 1_500,
                    reference_ms: 1_500,
                    on_newest_bar: true,
                }),
            );
            let projection = project(&history, &timeline, prices);

            let pies: Vec<_> = projection
                .aggressions
                .iter()
                .filter(|mark| !mark.live)
                .collect();
            // A print is carried by exactly one mark. Split the seam wrong and
            // this drops (a print landing in neither half) or doubles (a bar
            // summarized once per half).
            let summarized: Decimal = pies.iter().map(|pie| pie.quantity).sum();
            assert_eq!(
                summarized,
                Decimal::from(
                    history
                        .aggressions()
                        .filter(|print| timeline.locate(print.timestamp_ms).is_some())
                        .count()
                ),
                "now_ms={now_ms}: the pies must carry every print exactly once"
            );

            // And they carry it in one place. A summary is drawn in its bar's
            // slot, so the slot it lands in names the bar it is about: two
            // summaries in one slot at one price are one bar counted twice,
            // which is exactly what a seam left mid-bar would produce.
            let regions = timeline.region_count() as f64;
            let mut slots: Vec<(usize, Decimal)> = pies
                .iter()
                .map(|pie| ((pie.x * regions) as usize, pie.price_bucket))
                .collect();
            slots.sort_unstable();
            let mut once = slots.clone();
            once.dedup();
            assert_eq!(
                slots, once,
                "now_ms={now_ms}: a bar was summarized twice, once per half"
            );
        }
    }

    #[test]
    fn price_window_maps_high_to_top_and_low_to_bottom() {
        let window = PriceWindow::new(Decimal::from(100), Decimal::from(110)).unwrap();
        assert_eq!(window.y(Decimal::from(110)), Some(0.0));
        assert_eq!(window.y(Decimal::from(100)), Some(1.0));
        assert_eq!(window.y(Decimal::from(105)), Some(0.5));
        assert_eq!(window.y(Decimal::from(99)), None);
    }

    #[test]
    fn rejects_degenerate_price_windows() {
        assert!(PriceWindow::new(Decimal::ONE, Decimal::ONE).is_none());
        assert!(PriceWindow::new(Decimal::TWO, Decimal::ONE).is_none());
    }

    #[test]
    fn percentile_is_robust_to_one_large_outlier() {
        let values = (1..=100)
            .map(Decimal::from)
            .chain(std::iter::once(Decimal::from(1_000_000)));
        assert_eq!(percentile_99(values), Decimal::from(100));
    }

    #[test]
    fn log_intensity_is_monotonic_bounded_and_gamma_adjusted() {
        let reference = Decimal::from(100);
        let quiet = normalized_log_intensity(Decimal::ONE, reference, 1.0);
        let medium = normalized_log_intensity(Decimal::from(50), reference, 1.0);
        let full = normalized_log_intensity(reference, reference, 1.0);
        let above = normalized_log_intensity(Decimal::from(1_000), reference, 1.0);
        assert!(quiet > 0.0 && quiet < medium);
        assert!(medium < full);
        assert_eq!(full, 1.0);
        assert_eq!(above, 1.0);
        assert!(
            normalized_log_intensity(Decimal::from(10), reference, 0.5)
                > normalized_log_intensity(Decimal::from(10), reference, 1.0)
        );
    }

    #[test]
    fn aggression_size_uses_area_not_radius_proportionality() {
        let quarter = normalized_area_size(Decimal::from(25), Decimal::from(100));
        let full = normalized_area_size(Decimal::from(100), Decimal::from(100));
        assert!((quarter - 0.5).abs() < f32::EPSILON);
        assert_eq!(full, 1.0);
    }

    /// Four prints an order of magnitude apart, so the size ladder is visible.
    /// Both readability passes are off — temporal clustering and the dust
    /// merge — so these stay four distinct bubbles and the test measures the
    /// size mapping alone.
    fn history_with_a_size_ladder(config: HeatmapConfig) -> LiquidityHistory {
        size_ladder(config, 0)
    }

    /// The ladder above with the dust merge armed at `dust_merge_ms`.
    fn size_ladder(config: HeatmapConfig, dust_merge_ms: i64) -> LiquidityHistory {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_cluster_ms: 0,
            bubble_dust_merge_ms: dust_merge_ms,
            ..config
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        for (id, quantity) in [(1_u64, "1"), (2, "10"), (3, "50"), (4, "200")] {
            history.record_aggression(&Trade {
                agg_id: id,
                // Spread across time so clustering cannot merge them.
                timestamp_ms: 200 + (id as i64) * 100,
                price: dec("101"),
                quantity: dec(quantity),
                side: Side::Buy,
            });
        }
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        history
    }

    fn ladder_sizes(config: HeatmapConfig) -> Vec<(Decimal, f32)> {
        let history = history_with_a_size_ladder(config);
        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        let mut sizes: Vec<(Decimal, f32)> = projection
            .aggressions
            .iter()
            .map(|aggression| (aggression.quantity, aggression.size))
            .collect();
        sizes.sort_by_key(|pair| pair.0);
        sizes
    }

    #[test]
    fn the_dust_merge_folds_unreadable_prints_without_losing_quantity() {
        // The same ladder with the dust merge armed. Against a reference of
        // 200 the two smallest prints (1 and 10) fall below the radius where
        // the renderer stops dressing a bubble, so they arrive as one mark
        // carrying both — while the two readable prints are left alone.
        let history = size_ladder(
            HeatmapConfig {
                bubbles: BubbleStyle {
                    size_reference: BubbleSizeReference::VisibleMax,
                    ..BubbleStyle::default()
                },
                ..config()
            },
            5_000,
        );
        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );

        let mut quantities: Vec<Decimal> = projection
            .aggressions
            .iter()
            .map(|aggression| aggression.quantity)
            .collect();
        quantities.sort();
        assert_eq!(quantities, [dec("11"), dec("50"), dec("200")]);
        assert_eq!(
            quantities.iter().sum::<Decimal>(),
            dec("261"),
            "merging is a drawing decision and never loses quantity"
        );
        let folded = projection
            .aggressions
            .iter()
            .find(|aggression| aggression.quantity == dec("11"))
            .expect("the two dust prints fold into one bubble");
        assert_eq!(folded.trade_count, 2);
        assert_eq!(folded.agg_ids, [1, 2]);
    }

    #[test]
    fn big_prints_read_as_big_bubbles_under_every_size_reference() {
        // The size factor drives radius through area, so it must grow strictly
        // with quantity and reach 1.0 (the maximum radius) for the reference
        // print. This is the "can I see the large trades?" guarantee.
        let sizes = ladder_sizes(HeatmapConfig {
            bubbles: BubbleStyle {
                size_reference: BubbleSizeReference::VisibleMax,
                ..BubbleStyle::default()
            },
            ..config()
        });
        assert_eq!(sizes.len(), 4);
        for pair in sizes.windows(2) {
            assert!(
                pair[1].1 > pair[0].1,
                "size must grow with quantity: {pair:?}"
            );
        }
        assert_eq!(
            sizes.last().unwrap().1,
            1.0,
            "the biggest print is full size"
        );
        // Area proportionality: a quarter of the reference quantity is half the
        // size factor, i.e. a quarter of the drawn area.
        let fifty = sizes.iter().find(|(qty, _)| *qty == dec("50")).unwrap().1;
        assert!(
            (fifty - 0.5).abs() < 1e-6,
            "50/200 must map to 0.5, got {fifty}"
        );

        // A fixed reference pins the scale: the same print keeps the same size
        // no matter what else is visible, and anything above it saturates.
        let fixed = ladder_sizes(HeatmapConfig {
            bubbles: BubbleStyle {
                size_reference: BubbleSizeReference::Fixed,
                size_reference_quantity: 50.0,
                ..BubbleStyle::default()
            },
            ..config()
        });
        assert_eq!(
            fixed.iter().find(|(qty, _)| *qty == dec("50")).unwrap().1,
            1.0
        );
        assert_eq!(
            fixed.last().unwrap().1,
            1.0,
            "a print above the fixed reference clamps instead of overflowing"
        );
        let small = fixed.first().unwrap().1;
        assert!(small > 0.0 && small < 0.2, "1/50 stays a dot, got {small}");
    }

    #[test]
    fn hiding_small_prints_is_display_only_and_keeps_the_scale() {
        let bubbles = BubbleStyle {
            size_reference: BubbleSizeReference::VisibleMax,
            min_quantity: 20.0,
            ..BubbleStyle::default()
        };
        let sizes = ladder_sizes(HeatmapConfig {
            bubbles,
            ..config()
        });
        let quantities: Vec<Decimal> = sizes.iter().map(|(qty, _)| *qty).collect();
        assert_eq!(quantities, vec![dec("50"), dec("200")]);
        // The reference still comes from every visible print, so the surviving
        // bubbles keep the size they had before the floor was raised.
        assert!((sizes[0].1 - 0.5).abs() < 1e-6);
        assert_eq!(sizes[1].1, 1.0);
    }

    #[test]
    fn a_hidden_print_still_explains_the_reduction_it_caused() {
        // The floor is applied after association, so a small print that ate a
        // wall keeps the reduction marked as aggression-aligned even though the
        // bubble itself is not drawn. Hiding noise must never rewrite evidence.
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubbles: BubbleStyle {
                min_quantity: 100.0,
                ..BubbleStyle::default()
            },
            ..config()
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.record_aggression(&Trade {
            agg_id: 7,
            timestamp_ms: 400,
            price: dec("101"),
            quantity: dec("3"),
            side: Side::Buy,
        });
        // Ask 101: 4 -> 1, right after the print.
        history
            .apply_delta(
                450,
                &BookDelta::new(11, 11, vec![], vec![level("101", "1")]),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(12, 12, vec![], vec![]))
            .unwrap();

        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert!(
            projection.aggressions.is_empty(),
            "a 3-lot print is under the 100 floor"
        );
        let aligned = projection
            .liquidity_events
            .iter()
            .find(|event| event.price_bucket == dec("101"))
            .expect("the reduction is still projected");
        assert_eq!(aligned.evidence, LiquidityEvidence::AggressionAligned);
        assert!(aligned.matched_quantity > Decimal::ZERO);
    }

    #[test]
    fn unattributed_reduction_floor_hides_small_pulls_only() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        // Bid 100: 3 -> 2 (33% pull, unattributed): under the 50% floor.
        history
            .apply_delta(
                300,
                &BookDelta::new(11, 11, vec![level("100", "2")], vec![]),
            )
            .unwrap();
        // Ask 101: 4 -> 1 (75% pull, unattributed): over the floor.
        history
            .apply_delta(
                500,
                &BookDelta::new(12, 12, vec![], vec![level("101", "1")]),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(13, 13, vec![], vec![]))
            .unwrap();

        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
        let projection = project(&history, &timeline, prices);
        let buckets: Vec<_> = projection
            .liquidity_events
            .iter()
            .map(|event| event.price_bucket)
            .collect();
        assert!(buckets.contains(&dec("101")), "large pull must display");
        assert!(!buckets.contains(&dec("100")), "small pull must be hidden");

        // The fraction floor alone is not enough: with it at zero the small
        // pull is still gated by its size against the visible reference.
        let mut fraction_only = history.config().clone();
        fraction_only.min_unattributed_reduction = 0.0;
        history.update_config(fraction_only).unwrap();
        let projection = project(&history, &timeline, prices);
        assert!(
            !projection
                .liquidity_events
                .iter()
                .any(|event| event.price_bucket == dec("100")),
            "a small pull must stay hidden while the size gate holds"
        );

        // Lowering both display floors to zero shows the small pull too.
        let mut permissive_config = history.config().clone();
        permissive_config.min_unattributed_reduction = 0.0;
        permissive_config.min_unattributed_pull_share = 0.0;
        history.update_config(permissive_config).unwrap();
        let projection = project(&history, &timeline, prices);
        assert!(
            projection
                .liquidity_events
                .iter()
                .any(|event| event.price_bucket == dec("100"))
        );
    }

    #[test]
    fn compatible_aggression_rescues_a_small_reduction_from_the_floor() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.record_aggression(&Trade {
            agg_id: 1,
            timestamp_ms: 290,
            price: dec("100"),
            quantity: dec("1"),
            side: Side::Sell,
        });
        // The same 33% bid pull as above, but now a compatible sell hit it.
        history
            .apply_delta(
                300,
                &BookDelta::new(11, 11, vec![level("100", "2")], vec![]),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(12, 12, vec![], vec![]))
            .unwrap();

        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
        let projection = project(&history, &timeline, prices);
        assert!(
            projection.liquidity_events.iter().any(|event| {
                event.price_bucket == dec("100")
                    && matches!(event.evidence, LiquidityEvidence::AggressionAligned)
            }),
            "aligned bites always display regardless of the floor"
        );
    }

    #[test]
    fn event_cap_keeps_aligned_evidence_ahead_of_unattributed_pulls() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            max_visible_cells: 2,
            min_unattributed_reduction: 0.0,
            min_unattributed_pull_share: 0.0,
            ..HeatmapConfig::default()
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        // A small SELL print aligns with a small bid reduction at 100...
        history.record_aggression(&Trade {
            agg_id: 1,
            timestamp_ms: 290,
            price: dec("100"),
            quantity: dec("0.5"),
            side: Side::Sell,
        });
        history
            .apply_delta(
                300,
                &BookDelta::new(11, 11, vec![level("100", "2.5")], vec![]),
            )
            .unwrap();
        // ...followed by two much larger unattributed pulls.
        history
            .apply_delta(
                400,
                &BookDelta::new(12, 12, vec![level("99", "0.1")], vec![]),
            )
            .unwrap();
        history
            .apply_delta(
                500,
                &BookDelta::new(13, 13, vec![], vec![level("102", "0.5")]),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(14, 14, vec![], vec![]))
            .unwrap();

        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
        let projection = project(&history, &timeline, prices);

        assert_eq!(projection.liquidity_events.len(), 2, "cap of two applies");
        assert_eq!(projection.dropped_liquidity_events, 1);
        // The smallest event by removed quantity is the aligned one, yet it
        // must survive the cap: bubbles point at aligned events.
        assert!(
            projection.liquidity_events.iter().any(|event| {
                event.price_bucket == dec("100")
                    && matches!(event.evidence, LiquidityEvidence::AggressionAligned)
            }),
            "aligned evidence must outrank bigger unattributed pulls in the cap"
        );
    }

    /// Clearing the map on the candles may not delete the tape's.
    ///
    /// These cells span the whole normalized x axis, tape included, and the
    /// renderer clips them per pane. Gating their *production* on the candles'
    /// switch therefore emptied the tape as well: the trader switched off one
    /// pane and the other went dark with it, which no amount of correctness in
    /// the config layer could fix — the data was never built.
    #[test]
    fn hiding_the_map_on_the_candles_still_projects_the_tapes() {
        let base = HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            // The candles are clear; the tape keeps both of its layers, which
            // is what a fresh install now opens as.
            show_depth: false,
            ..HeatmapConfig::default()
        };
        assert!(!base.depth_visible(), "the candles draw no map");
        assert!(base.lane_depth_drawn(), "the tape does");

        // Enough book movement to close a run, so the cells below exist for a
        // reason other than the switch under test.
        let fill = |config: HeatmapConfig| {
            let mut history = LiquidityHistory::new(config);
            history.install_snapshot(100, 1, snapshot(10)).unwrap();
            history
                .apply_delta(
                    800,
                    &BookDelta::new(11, 11, vec![level("100", "6")], vec![]),
                )
                .unwrap();
            history
                .apply_delta(900, &BookDelta::new(11, 11, vec![], vec![]))
                .unwrap();
            history
        };
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();

        let projection = project(&fill(base.clone()), &timeline, prices);
        assert!(
            !projection.cells.is_empty(),
            "the tape reads these cells, so hiding the candles' map may not stop building them"
        );

        // And with the tape's own switch off too, nobody is reading them.
        let both_off = fill(HeatmapConfig {
            live_lane: LiveLaneStyle {
                show_depth: false,
                ..LiveLaneStyle::default()
            },
            ..base
        });
        assert!(
            project(&both_off, &timeline, prices).cells.is_empty(),
            "with neither pane drawing the map, none is built"
        );
    }

    #[test]
    fn disabled_projection_is_empty_even_with_data() {
        // Every layer off, said out loud: the default config stopped being
        // inert when the tape gained defaults of its own (both layers on), so a
        // test about a *disabled* projection has to disable the tape too.
        let mut history = LiquidityHistory::new(HeatmapConfig {
            live_lane: LiveLaneStyle {
                enabled: false,
                ..LiveLaneStyle::default()
            },
            ..HeatmapConfig::default()
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();
        let projection = project(&history, &timeline, prices);
        assert!(!projection.enabled);
        assert!(projection.cells.is_empty());
        assert!(projection.aggressions.is_empty());
    }

    #[test]
    fn projects_and_clips_runs_in_time_and_price() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history
            .apply_delta(
                800,
                &BookDelta::new(11, 11, vec![level("100", "6")], vec![]),
            )
            .unwrap();
        // A stale event advances display coverage without splitting any run.
        history
            .apply_delta(900, &BookDelta::new(11, 11, vec![], vec![]))
            .unwrap();

        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();
        let projection = project(&history, &timeline, prices);

        assert!(projection.enabled);
        assert!(!projection.cells.is_empty());
        assert!(
            projection
                .cells
                .iter()
                .all(|cell| (0.0..=1.0).contains(&cell.x0)
                    && (0.0..=1.0).contains(&cell.x1)
                    && (0.0..=1.0).contains(&cell.y0)
                    && (0.0..=1.0).contains(&cell.y1))
        );
        let old = projection
            .cells
            .iter()
            .find(|cell| cell.price_bucket == dec("100") && cell.quantity == dec("3"))
            .unwrap();
        assert!((old.x0 - 0.1).abs() < 1e-9);
        assert!((old.x1 - 0.8).abs() < 1e-9);
        // Only the lower half of bucket [100,101] is in [99.5,100.5].
        assert!((old.y0 - 0.0).abs() < 1e-9);
        assert!((old.y1 - 0.5).abs() < 1e-9);
    }

    /// A bar is timeless: its slot cannot say *when* inside the bar a wall was
    /// resting, only how much of the bar it rested for. So with a tape on
    /// screen the slots summarize — one band per level, weighted by presence —
    /// and the tape keeps the runs themselves.
    #[test]
    fn a_bar_slot_summarizes_the_book_while_the_tape_keeps_the_runs() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(0, 1, snapshot(10)).unwrap();
        // The 100 bid (quantity 3) is pulled halfway through the first bar.
        history
            .apply_delta(
                5_000,
                &BookDelta::new(11, 11, vec![level("100", "0")], vec![]),
            )
            .unwrap();
        history
            .apply_delta(20_000, &BookDelta::new(12, 12, vec![], vec![]))
            .unwrap();

        let closed = [bar(0, 10_000), bar(10_000, 20_000)];
        let prices = PriceWindow::new(dec("99.5"), dec("100.5")).unwrap();
        let of_bucket = |projection: &HeatmapProjection| {
            let mut cells: Vec<_> = projection
                .cells
                .iter()
                .filter(|cell| cell.price_bucket == dec("100"))
                .map(|cell| (cell.x0, cell.x1, cell.quantity))
                .collect();
            cells.sort_by(|a, b| a.0.total_cmp(&b.0));
            cells
        };

        // Without a tape the chart keeps drawing the run where it happened:
        // half of the first bar's slot, at its own quantity.
        let alone = project(
            &history,
            &BarTimeline::from_bars(0, &closed, None, None),
            prices,
        );
        assert_eq!(of_bucket(&alone), vec![(0.0, 0.25, dec("3"))]);

        // With a tape, that same run becomes one summary band per bar: the
        // whole slot wide, carrying what was typically resting there — three
        // for half the bar reads as one and a half.
        let summarized = project(
            &history,
            &BarTimeline::from_bars(
                0,
                &closed,
                None,
                Some(crate::orderflow::LiveEdge {
                    now_ms: 20_000,
                    window_ms: 5_000,
                    reference_ms: 5_000,
                    on_newest_bar: true,
                }),
            ),
            prices,
        );
        // Three regions: two bar slots and the tape. The run only touches the
        // first bar, and it is long gone by the time the tape's window opens.
        assert_eq!(
            of_bucket(&summarized),
            vec![(0.0, 1.0 / 3.0, dec("1.5"))],
            "one band per bar, weighted by how much of it the wall was there"
        );
    }

    #[test]
    fn marks_history_before_first_snapshot_as_unavailable() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(400, 1, snapshot(10)).unwrap();
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        let unavailable = projection
            .gaps
            .iter()
            .find(|gap| gap.reason == "book_unavailable_before_capture")
            .unwrap();
        assert_eq!(unavailable.x0, 0.0);
        assert!((unavailable.x1 - 0.4).abs() < 1e-9);
        assert_eq!(unavailable.to_generation, Some(1));
    }

    #[test]
    fn an_unsynchronized_history_marks_the_whole_timeline_unavailable() {
        let history = LiquidityHistory::new(config());
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert_eq!(
            projection.gaps.as_slice(),
            [GapPrimitive {
                from_generation: None,
                to_generation: None,
                x0: 0.0,
                x1: 1.0,
                reason: "book_unavailable_before_capture".to_owned(),
            }]
        );
    }

    #[test]
    fn resync_gap_is_a_primitive_and_runs_do_not_bridge_it() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.mark_gap(300, "sequence_gap").unwrap();
        history.install_snapshot(600, 2, snapshot(50)).unwrap();
        history
            .apply_delta(900, &BookDelta::new(50, 50, vec![], vec![]))
            .unwrap();

        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        let gap = projection
            .gaps
            .iter()
            .find(|gap| gap.reason == "sequence_gap")
            .unwrap();
        assert!((gap.x0 - 0.3).abs() < 1e-9);
        assert!((gap.x1 - 0.6).abs() < 1e-9);
        assert!(
            projection
                .cells
                .iter()
                .all(|cell| { cell.x1 <= gap.x0 || cell.x0 >= gap.x1 })
        );
    }

    #[test]
    fn aggression_uses_trade_side_without_affecting_liquidity_reference() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.record_aggression(&Trade {
            agg_id: 42,
            timestamp_ms: 500,
            price: dec("101"),
            quantity: dec("4"),
            side: Side::Buy,
        });
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        let aggression = &projection.aggressions[0];
        assert_eq!(aggression.agg_id, 42);
        assert_eq!(aggression.side, Side::Buy);
        assert_eq!(aggression.consumed_side, BookSide::Ask);
        assert!((aggression.x - 0.5).abs() < 1e-9);
        assert_eq!(aggression.size, 1.0);
        assert_eq!(projection.liquidity_reference, dec("5"));
    }

    #[test]
    fn bubbles_track_execution_price_not_a_flat_line() {
        // Regression guard: aggressions at different prices must land at
        // different chart heights (higher price -> higher on chart -> smaller
        // y), so a moving market shows bubbles riding the price, never a flat
        // horizontal band.
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_cluster_ms: 0,
            ..config()
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        for (id, price) in [(1_u64, "99.5"), (2, "100.5"), (3, "101.5")] {
            history.record_aggression(&Trade {
                agg_id: id,
                timestamp_ms: 200 + id as i64,
                price: dec(price),
                quantity: Decimal::ONE,
                side: Side::Buy,
            });
        }
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("99"), dec("102")).unwrap(),
        );
        assert_eq!(projection.aggressions.len(), 3);
        let y_at = |bucket: &str| {
            projection
                .aggressions
                .iter()
                .find(|aggression| aggression.price_bucket == dec(bucket))
                .unwrap_or_else(|| panic!("no bubble at bucket {bucket}"))
                .y
        };
        assert!(y_at("101") < y_at("100"), "higher price must sit higher");
        assert!(y_at("100") < y_at("99"), "higher price must sit higher");
    }

    #[test]
    fn bubbles_project_with_l2_capture_off() {
        // The aggression layer only needs the trade stream. With depth capture
        // off there is no map and no coverage story to tell, so cells and gap
        // primitives stay empty while bubbles still project.
        let mut history = LiquidityHistory::new(HeatmapConfig {
            enabled: false,
            bubble_cluster_ms: 0,
            ..config()
        });
        for (id, price) in [(1_u64, "99.5"), (2, "100.5")] {
            history.record_aggression(&Trade {
                agg_id: id,
                timestamp_ms: 200 + id as i64,
                price: dec(price),
                quantity: Decimal::ONE,
                side: Side::Buy,
            });
        }
        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("99"), dec("102")).unwrap(),
        );
        assert!(projection.enabled);
        assert_eq!(projection.aggressions.len(), 2);
        assert!(projection.cells.is_empty());
        assert!(projection.gaps.is_empty());
    }

    #[test]
    fn turning_l2_capture_off_hides_the_map_without_touching_bubbles_or_history() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            bubble_cluster_ms: 0,
            ..config()
        });
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.record_aggression(&Trade {
            agg_id: 1,
            timestamp_ms: 200,
            price: dec("100.5"),
            quantity: Decimal::ONE,
            side: Side::Buy,
        });
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("99"), dec("102")).unwrap();
        let before = project(&history, &timeline, prices);
        assert!(!before.cells.is_empty());
        assert_eq!(before.aggressions.len(), 1);
        let runs_before = history.runs().count();

        history
            .update_config(HeatmapConfig {
                enabled: false,
                bubble_cluster_ms: 0,
                ..config()
            })
            .unwrap();
        let after = project(&history, &timeline, prices);
        assert!(after.cells.is_empty(), "the map stops drawing");
        assert_eq!(after.aggressions.len(), 1, "bubbles are untouched");
        assert_eq!(
            history.runs().count(),
            runs_before,
            "retained L2 history survives the toggle"
        );
    }

    /// The depth caps still drop and still report it — a heat cell is a
    /// picture of the book, and a frame that cannot draw them all says so. The
    /// bubble budget is the one that changed: it folds instead, so it reports
    /// folds and the ink still adds up to what traded.
    #[test]
    fn primitive_caps_report_what_they_dropped_and_what_they_folded() {
        let limited = HeatmapConfig {
            max_visible_cells: 1,
            max_aggression_primitives: 4,
            bubble_cluster_ms: 0,
            ..config()
        };
        let mut history = LiquidityHistory::new(limited);
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        for id in 1..=3 {
            history.record_aggression(&Trade {
                agg_id: id,
                timestamp_ms: 200 + id as i64,
                price: dec("101"),
                quantity: Decimal::from(id),
                side: Side::Buy,
            });
        }
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert_eq!(projection.cells.len(), 1);
        assert_eq!(projection.dropped_cells, 3);
        assert_eq!(projection.cells[0].quantity, dec("5"));
        // Two marks for three prints: the budget folded the two smallest into
        // one and left the biggest — the mark a trader is actually reading —
        // exactly as it was.
        assert_eq!(projection.aggressions.len(), 2);
        assert_eq!(
            projection.folded_aggressions, 1,
            "one mark folded, none lost"
        );
        let drawn: Decimal = projection
            .aggressions
            .iter()
            .map(|mark| mark.quantity)
            .sum();
        assert_eq!(
            drawn,
            dec("6"),
            "prints of 1, 2 and 3 are six contracts of ink"
        );
        let fold = projection
            .aggressions
            .iter()
            .find(|mark| mark.folded_marks > 0)
            .expect("the budget folded something, so a mark must say so");
        assert_eq!(fold.quantity, dec("3"), "the two smallest folded together");
        assert_eq!(fold.folded_marks, 2, "and it says it stands for two");
        assert_eq!(fold.trade_count, 2);
    }

    #[test]
    fn projection_uses_live_end_of_partial_timeline() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history
            .apply_delta(750, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let closed = [bar(0, 200)];
        let partial = bar(300, 350);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(800, &closed));
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert!(
            projection
                .cells
                .iter()
                .any(|cell| cell.x1 > 0.9 && cell.x1 < 1.0)
        );
    }

    /// Bubbles only: no book, so nothing but the tape decides what is drawn.
    fn tape(config: HeatmapConfig, trades: &[(u64, i64, &str, &str, Side)]) -> LiquidityHistory {
        let mut history = LiquidityHistory::new(config);
        for (agg_id, timestamp_ms, price, quantity, side) in trades {
            history.record_aggression(&Trade {
                agg_id: *agg_id,
                timestamp_ms: *timestamp_ms,
                price: dec(price),
                quantity: dec(quantity),
                side: *side,
            });
        }
        history
    }

    fn bubbles_only() -> HeatmapConfig {
        HeatmapConfig {
            enabled: false,
            show_aggressions: true,
            price_grouping: Decimal::ONE,
            display_grouping: DisplayGrouping::Native,
            bubble_cluster_ms: 0,
            bubble_dust_merge_ms: 0,
            bubbles: BubbleStyle {
                readable_min_radius: 0.0,
                ..BubbleStyle::default()
            },
            ..HeatmapConfig::default()
        }
    }

    /// Zooming changes what is on screen, never what a quantity means: the
    /// same print maps to the same normalized size through every price window,
    /// in every reference mode — the automatic ones included. Only a change in
    /// the cluster's own quantity may change its bubble.
    #[test]
    fn a_prints_bubble_keeps_its_size_when_the_window_zooms_out() {
        let trades = [
            (1_u64, 6_000_i64, "100", "2", Side::Buy),
            (2, 7_000, "200", "100", Side::Sell),
        ];
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
        let partial = bar(15_000, 20_000);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
        // Zoomed in only the small print is on screen; zoomed out the large
        // one joins it and, today, silently rescales it.
        let zoomed_in = PriceWindow::new(dec("98"), dec("103")).unwrap();
        let zoomed_out = PriceWindow::new(dec("98"), dec("203")).unwrap();
        for reference in [
            BubbleSizeReference::VisibleP99,
            BubbleSizeReference::VisibleMax,
            BubbleSizeReference::Fixed,
        ] {
            let config = HeatmapConfig {
                bubbles: BubbleStyle {
                    size_reference: reference,
                    size_reference_quantity: 100.0,
                    ..bubbles_only().bubbles
                },
                ..bubbles_only()
            };
            let size_through = |prices: PriceWindow| {
                project(&tape(config.clone(), &trades), &timeline, prices)
                    .aggressions
                    .iter()
                    .find(|bubble| bubble.quantity == dec("2"))
                    .map(|bubble| bubble.size)
                    .expect("the small print is inside both windows")
            };
            let narrow = size_through(zoomed_in);
            let wide = size_through(zoomed_out);
            assert!(
                (narrow - wide).abs() < 1e-6,
                "{reference:?}: one quantity, one size — got {narrow} zoomed in, {wide} zoomed out"
            );
        }
    }

    /// The zoom rule holds for the summary tier too: a closed bar's pie maps
    /// to the same normalized size through every price window. This is the
    /// mark most of a chart is made of once the candle summary is on, so the
    /// print scale being stable is not enough — a 1mm zoom that rescales
    /// every pie rescales the chart.
    #[test]
    fn a_pie_keeps_its_size_when_the_window_zooms_out() {
        let trades = [
            (1_u64, 6_000_i64, "100", "2", Side::Buy),
            (2, 6_500, "100", "2", Side::Sell),
            (3, 7_000, "200", "100", Side::Sell),
        ];
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
        let partial = bar(15_000, 20_000);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
        // Zoomed in only the small pie is on screen; zoomed out the huge one
        // joins it.
        let zoomed_in = PriceWindow::new(dec("98"), dec("103")).unwrap();
        let zoomed_out = PriceWindow::new(dec("98"), dec("203")).unwrap();
        for reference in [
            BubbleSizeReference::VisibleP99,
            BubbleSizeReference::VisibleMax,
            BubbleSizeReference::Fixed,
        ] {
            let config = HeatmapConfig {
                bubble_candle_summary: true,
                bubbles: BubbleStyle {
                    size_reference: reference,
                    size_reference_quantity: 100.0,
                    ..bubbles_only().bubbles
                },
                ..bubbles_only()
            };
            let pie_size_through = |prices: PriceWindow| {
                project(&tape(config.clone(), &trades), &timeline, prices)
                    .aggressions
                    .iter()
                    .find(|mark| !mark.live && mark.quantity == dec("4"))
                    .map(|mark| mark.size)
                    .expect("the small bar's pie is inside both windows")
            };
            let narrow = pie_size_through(zoomed_in);
            let wide = pie_size_through(zoomed_out);
            assert!(
                (narrow - wide).abs() < 1e-6,
                "{reference:?}: one pie, one size — got {narrow} zoomed in, {wide} zoomed out"
            );
        }
    }

    /// The one legitimate way zoom grows a bubble: a coarser grouping merges
    /// prints into one cluster, and the merged mark carries their summed
    /// quantity — so it may only read *bigger* than the prints it swallowed,
    /// never smaller.
    #[test]
    fn a_cluster_merged_by_zooming_out_reads_bigger_not_smaller() {
        let trades = [
            (1_u64, 6_000_i64, "100", "3", Side::Buy),
            (2, 6_500, "101", "2", Side::Buy),
            (3, 7_000, "150", "50", Side::Sell),
        ];
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
        let partial = bar(15_000, 20_000);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
        // Ten target rows: at a 10-wide window prints one price apart keep
        // their own bucket; at a 200-wide window they share one.
        let zoomed_in = PriceWindow::new(dec("98"), dec("108")).unwrap();
        let zoomed_out = PriceWindow::new(dec("50"), dec("250")).unwrap();
        for reference in [
            BubbleSizeReference::VisibleP99,
            BubbleSizeReference::VisibleMax,
            BubbleSizeReference::Fixed,
        ] {
            let config = HeatmapConfig {
                display_grouping: DisplayGrouping::Adaptive { target_rows: 10 },
                bubble_cluster_ms: 1_000,
                bubbles: BubbleStyle {
                    size_reference: reference,
                    size_reference_quantity: 100.0,
                    ..bubbles_only().bubbles
                },
                ..bubbles_only()
            };
            let size_of = |prices: PriceWindow, quantity: &str| {
                project(&tape(config.clone(), &trades), &timeline, prices)
                    .aggressions
                    .iter()
                    .find(|bubble| bubble.quantity == dec(quantity))
                    .map(|bubble| bubble.size)
                    .unwrap_or_else(|| panic!("no bubble of quantity {quantity}"))
            };
            let three = size_of(zoomed_in, "3");
            let two = size_of(zoomed_in, "2");
            let merged = size_of(zoomed_out, "5");
            assert!(
                merged > three && merged > two,
                "{reference:?}: the merged cluster carries more quantity, so it must read \
                 bigger — got {merged} against {three} and {two}"
            );
        }
    }

    /// Opposing prints of the same bar collapse into one two-sided mark in its
    /// slot; the tape's own prints stay separate, because the tape is where
    /// flow is read print by print.
    #[test]
    fn the_summary_folds_a_bar_and_leaves_the_live_lane_alone() {
        // Five-second bars, so the rolling window is five seconds wide and the
        // first bar's prints have long left it.
        let trades = [
            (1_u64, 1_000_i64, "100", "3", Side::Buy),
            (2, 2_000, "100", "1", Side::Sell),
            (3, 16_000, "100", "2", Side::Buy),
            (4, 17_000, "100", "2", Side::Sell),
        ];
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
        let partial = bar(15_000, 20_000);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
        assert_eq!(timeline.lane_start_ms(), Some(15_000));
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

        let off = project(&tape(bubbles_only(), &trades), &timeline, prices);
        assert_eq!(off.aggressions.len(), 4, "the default draws every print");
        assert!(
            off.aggressions
                .iter()
                .all(|bubble| bubble.buy_share == 1.0 || bubble.buy_share == 0.0),
            "no mark carries both sides until the summary is switched on"
        );

        let summarized = project(
            &tape(
                HeatmapConfig {
                    bubble_candle_summary: true,
                    ..bubbles_only()
                },
                &trades,
            ),
            &timeline,
            prices,
        );
        // Two pies — the closed bar's and the forming bar's running total —
        // plus the tape's two prints, untouched.
        assert_eq!(summarized.aggressions.len(), 4);
        let pies: Vec<_> = summarized
            .aggressions
            .iter()
            .filter(|bubble| bubble.buy_share > 0.0 && bubble.buy_share < 1.0)
            .collect();
        assert_eq!(pies.len(), 2);
        assert_eq!(pies[0].quantity, dec("4"));
        assert!((pies[0].buy_share - 0.75).abs() < 1e-6);
        assert!(!pies[0].live, "a summary belongs to a bar, not to the tape");
        // The forming bar's pie carries what it has taken so far — the very
        // prints still rolling across the tape.
        assert_eq!(pies[1].quantity, dec("4"));
        assert!((pies[1].buy_share - 0.5).abs() < 1e-6);
        assert_eq!(
            summarized
                .aggressions
                .iter()
                .filter(|bubble| bubble.live)
                .count(),
            2,
            "both live prints keep their own mark"
        );
    }

    /// A summary is a statement about a finished bar, so it is complete the
    /// moment the bar is — even while the prints behind it are still rolling
    /// across the tape. That is the one place the two views overlap: the pie is
    /// an aggregate of the same flow, not a second copy of a raw print.
    #[test]
    fn a_bar_gets_its_pie_at_close_without_taking_its_prints_off_the_tape() {
        let trades = [
            (1_u64, 16_000_i64, "100", "3", Side::Buy),
            (2, 17_000, "100", "1", Side::Sell),
        ];
        // The last closed bar runs to 18 s and the window reaches back to 15 s,
        // so both prints are inside a bar that is over *and* on the tape.
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 18_000)];
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&bar(18_000, 20_000)),
            live(20_000, &closed),
        );
        assert_eq!(timeline.lane_start_ms(), Some(15_000));
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

        let summarized = project(
            &tape(
                HeatmapConfig {
                    bubble_candle_summary: true,
                    ..bubbles_only()
                },
                &trades,
            ),
            &timeline,
            prices,
        );
        // Two marks still rolling, plus the finished bar's pie.
        assert_eq!(summarized.aggressions.iter().filter(|b| b.live).count(), 2);
        let pies: Vec<_> = summarized
            .aggressions
            .iter()
            .filter(|bubble| !bubble.live)
            .collect();
        assert_eq!(pies.len(), 1, "the closed bar summarizes immediately");
        assert_eq!(pies[0].quantity, dec("4"));
        assert!((pies[0].buy_share - 0.75).abs() < 1e-6);
        // The pie sits in its bar's slot, left of where the tape begins.
        let lane_left = 3.0 / 4.0;
        assert!(pies[0].x < lane_left, "the pie belongs to the bar's slot");
        assert!(
            summarized
                .aggressions
                .iter()
                .filter(|b| b.live)
                .all(|b| b.x >= lane_left)
        );

        // Without the summary a raw print is drawn exactly once, on the tape.
        let plain = project(&tape(bubbles_only(), &trades), &timeline, prices);
        assert_eq!(plain.aggressions.len(), 2);
        assert!(plain.aggressions.iter().all(|bubble| bubble.live));
    }

    /// The forming bar summarizes too, and its pie grows with the flow: that is
    /// how the compressed left side reports what is happening *now* instead of
    /// only what already happened once the bar closed.
    #[test]
    fn the_forming_bars_pie_grows_with_every_order() {
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&bar(15_000, 30_000)),
            live(30_000, &closed),
        );
        assert_eq!(timeline.lane_start_ms(), Some(25_000));
        let summarize = HeatmapConfig {
            bubble_candle_summary: true,
            ..bubbles_only()
        };
        let pie_of = |trades: &[(u64, i64, &str, &str, Side)]| {
            let projected = project(
                &tape(summarize.clone(), trades),
                &timeline,
                PriceWindow::new(dec("98"), dec("103")).unwrap(),
            );
            assert_eq!(projected.aggressions.len(), 1, "one bar, one mark");
            let pie = projected.aggressions[0].clone();
            assert!(!pie.live, "a summary belongs to a bar, not to the tape");
            (pie.quantity, pie.buy_share, pie.trade_count)
        };

        // One buy so far: the mark is all buy and carries exactly that print.
        let first = [(1_u64, 16_000_i64, "100", "3", Side::Buy)];
        assert_eq!(pie_of(&first), (dec("3"), 1.0, 1));
        // A sell arrives: the same mark grows and the proportion moves with it,
        // without waiting for the bar to close.
        let second = [
            (1_u64, 16_000_i64, "100", "3", Side::Buy),
            (2, 17_000, "100", "1", Side::Sell),
        ];
        assert_eq!(pie_of(&second), (dec("4"), 0.75, 2));
    }

    /// A summary carries a whole bar's quantity. Sized against the reference
    /// single prints set, every one of them would peg at the largest radius
    /// and the summaries would stop saying anything about each other — so
    /// pies read on their own scale, the session's busiest minute per level,
    /// and stay ordered among themselves.
    #[test]
    fn a_summary_is_sized_against_the_sessions_minutes_and_a_pinned_reference_never_moves() {
        // One busy bar and one quiet one, both closed, plus a live print.
        let mut trades: Vec<(u64, i64, &str, &str, Side)> = Vec::new();
        for index in 0..20_u64 {
            trades.push((
                index + 1,
                1_000 + index as i64 * 500,
                if index % 2 == 0 { "100" } else { "101" },
                "1",
                if index % 2 == 0 {
                    Side::Buy
                } else {
                    Side::Sell
                },
            ));
        }
        trades.push((100, 21_000, "100", "1", Side::Buy));
        trades.push((101, 21_500, "100", "1", Side::Sell));
        trades.push((200, 41_000, "100", "1", Side::Buy));
        let closed = [bar(0, 20_000), bar(20_000, 40_000)];
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&bar(40_000, 45_000)),
            live(45_000, &closed),
        );
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

        let summarized = project(
            &tape(
                HeatmapConfig {
                    bubble_candle_summary: true,
                    ..bubbles_only()
                },
                &trades,
            ),
            &timeline,
            prices,
        );
        // History is measured against history, the lane against the tape.
        assert!(summarized.summary_reference > summarized.aggression_reference);
        let busiest = summarized
            .aggressions
            .iter()
            .filter(|bubble| !bubble.live)
            .map(|bubble| bubble.size)
            .fold(0.0_f32, f32::max);
        let quietest = summarized
            .aggressions
            .iter()
            .filter(|bubble| !bubble.live)
            .map(|bubble| bubble.size)
            .fold(1.0_f32, f32::min);
        assert!(
            quietest < busiest,
            "summaries must still differ in size: {quietest} vs {busiest}"
        );

        // Without a summary the two regions share one scale, exactly as before.
        let plain = project(&tape(bubbles_only(), &trades), &timeline, prices);
        assert_eq!(plain.summary_reference, plain.aggression_reference);

        // A pinned reference is pinned: the user chose an absolute quantity so
        // that nothing on screen may rescale a bubble.
        let pinned = project(
            &tape(
                HeatmapConfig {
                    bubble_candle_summary: true,
                    bubbles: BubbleStyle {
                        size_reference: BubbleSizeReference::Fixed,
                        size_reference_quantity: 8.0,
                        readable_min_radius: 0.0,
                        ..BubbleStyle::default()
                    },
                    ..bubbles_only()
                },
                &trades,
            ),
            &timeline,
            prices,
        );
        assert_eq!(pinned.aggression_reference, dec("8"));
        assert_eq!(pinned.summary_reference, dec("8"));
    }

    /// The lane clusters on its own window, so the region with room to spare
    /// can show detail the compressed history cannot.
    #[test]
    fn the_lane_clusters_on_its_own_window() {
        let trades = [
            (1_u64, 1_000_i64, "100", "1", Side::Buy),
            (2, 1_050, "100", "1", Side::Buy),
            (3, 16_000, "100", "1", Side::Buy),
            (4, 16_050, "100", "1", Side::Buy),
        ];
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
        let partial = bar(15_000, 20_000);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

        // History gathers its pair, the lane keeps its prints apart.
        let split = project(
            &tape(
                HeatmapConfig {
                    bubble_cluster_ms: 100,
                    live_lane: LiveLaneStyle {
                        cluster_ms: Some(0),
                        ..LiveLaneStyle::default()
                    },
                    ..bubbles_only()
                },
                &trades,
            ),
            &timeline,
            prices,
        );
        assert_eq!(split.aggressions.len(), 3);
        assert_eq!(
            split.aggressions.iter().filter(|b| b.live).count(),
            2,
            "the lane drew both prints"
        );

        // Inheriting means inheriting: the same window on both sides of the
        // boundary gathers both pairs.
        let inherited = project(
            &tape(
                HeatmapConfig {
                    bubble_cluster_ms: 100,
                    ..bubbles_only()
                },
                &trades,
            ),
            &timeline,
            prices,
        );
        assert_eq!(inherited.aggressions.len(), 2);
    }

    /// A cluster must never straddle the boundary: half a bubble cannot be
    /// both a print still rolling across the tape and a settled summary. The
    /// boundary is the rolling window's left edge, so what decides it is a
    /// print's age, not which bar it came from.
    #[test]
    fn no_cluster_spans_the_lane_boundary() {
        let trades = [
            (1_u64, 14_900_i64, "100", "1", Side::Buy),
            (2, 15_100, "100", "1", Side::Buy),
        ];
        // A five-second window ending at 20 s reaches back to 15 s and cuts
        // between the two prints, a fifth of a second apart.
        let closed = [bar(0, 5_000), bar(5_000, 10_000), bar(10_000, 15_000)];
        let partial = bar(15_000, 20_000);
        let timeline = BarTimeline::from_bars(0, &closed, Some(&partial), live(20_000, &closed));
        assert_eq!(timeline.lane_start_ms(), Some(15_000));
        let projection = project(
            &tape(
                HeatmapConfig {
                    // A window wide enough to swallow both, were it allowed to.
                    bubble_cluster_ms: 2_000,
                    ..bubbles_only()
                },
                &trades,
            ),
            &timeline,
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        assert_eq!(projection.aggressions.len(), 2);
        assert_eq!(projection.aggressions.iter().filter(|b| b.live).count(), 1);
    }

    /// The live edge is the lane's right edge, so it is the right edge of the
    /// chart — and it is only reported for a frame that follows one.
    #[test]
    fn the_live_edge_is_the_right_edge_of_the_lane() {
        let closed = [bar(0, 20_000), bar(20_000, 40_000)];
        let history = tape(bubbles_only(), &[(1, 41_000, "100", "1", Side::Buy)]);
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

        let following = project(
            &history,
            &BarTimeline::from_bars(
                0,
                &closed,
                Some(&bar(40_000, 50_000)),
                live(50_000, &closed),
            ),
            prices,
        );
        assert_eq!(
            following.live_now_x,
            Some(1.0),
            "market time always reaches the lane's right edge"
        );

        let settled = project(
            &history,
            &BarTimeline::from_bars(0, &closed, Some(&bar(40_000, 50_000)), None),
            prices,
        );
        assert_eq!(settled.live_now_x, None);
    }

    #[test]
    fn display_grouping_changes_without_resetting_capture_history() {
        let mut history = LiquidityHistory::new(config());
        history
            .install_snapshot(
                100,
                1,
                BookSnapshot::new(
                    10,
                    vec![level("100", "2"), level("101", "3")],
                    vec![level("102", "4")],
                    BookCoverage::Full,
                ),
            )
            .unwrap();
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let runs_before = history.runs().count();
        let status_before = history.status();
        let next = HeatmapConfig {
            display_grouping: DisplayGrouping::Multiple(2),
            ..history.config().clone()
        };
        history.update_config(next).unwrap();

        assert_eq!(history.runs().count(), runs_before);
        assert_eq!(history.status(), status_before);
        assert!(history.book().is_initialized());

        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("99"), dec("104")).unwrap(),
        );
        assert_eq!(projection.effective_grouping.multiple, 2);
        assert_eq!(projection.effective_grouping.bucket_width, dec("2"));
        assert!(projection.cells.iter().any(|cell| {
            cell.side == BookSide::Bid
                && cell.price_bucket == dec("100")
                && cell.quantity == dec("5")
        }));
    }

    /// Baseline for every evidence-related test.
    fn event_config() -> HeatmapConfig {
        HeatmapConfig {
            enabled: true,
            show_aggressions: true,
            price_grouping: Decimal::ONE,
            display_grouping: DisplayGrouping::Native,
            bubble_cluster_ms: 100,
            liquidity_correlation_ms: 250,
            ..HeatmapConfig::default()
        }
    }

    /// One buy print plus two ask reductions: the one at t=500 has compatible
    /// aggression evidence, the full pull at t=800 is depth-only.
    fn reduction_history(config: HeatmapConfig) -> LiquidityHistory {
        let mut history = LiquidityHistory::new(config);
        history
            .install_snapshot(
                100,
                1,
                BookSnapshot::new(
                    10,
                    vec![level("100", "2")],
                    vec![level("101", "10")],
                    BookCoverage::Full,
                ),
            )
            .unwrap();
        history.record_aggression(&Trade {
            agg_id: 77,
            timestamp_ms: 480,
            price: dec("101"),
            quantity: dec("3"),
            side: Side::Buy,
        });
        history
            .apply_delta(
                500,
                &BookDelta::new(11, 11, vec![], vec![level("101", "6")]),
            )
            .unwrap();
        history
            .apply_delta(
                800,
                &BookDelta::new(12, 12, vec![], vec![level("101", "0")]),
            )
            .unwrap();
        history
    }

    fn project_reductions(config: HeatmapConfig) -> HeatmapProjection {
        let history = reduction_history(config);
        project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("99"), dec("103")).unwrap(),
        )
    }

    #[test]
    fn projects_partial_and_full_reductions_with_conserved_aggression_evidence() {
        let projection = project_reductions(event_config());
        assert_eq!(projection.liquidity_events.len(), 2);
        let partial = projection
            .liquidity_events
            .iter()
            .find(|event| event.timestamp_ms == 500)
            .unwrap();
        assert_eq!(partial.before, dec("10"));
        assert_eq!(partial.after, dec("6"));
        assert_eq!(partial.removed, dec("4"));
        assert!(!partial.full_removal);
        assert_eq!(partial.matched_quantity, dec("3"));
        assert_eq!(partial.matched_fraction, 0.75);
        assert_eq!(partial.evidence, LiquidityEvidence::AggressionAligned);

        let full = projection
            .liquidity_events
            .iter()
            .find(|event| event.timestamp_ms == 800)
            .unwrap();
        assert_eq!(full.removed, dec("6"));
        assert!(full.full_removal);
        assert_eq!(full.evidence, LiquidityEvidence::DepthOnly);

        let bubble = projection.aggressions.first().unwrap();
        assert_eq!(bubble.trade_count, 1);
        assert_eq!(bubble.agg_ids, [77]);
        assert_eq!(bubble.matched_quantity, dec("3"));
        assert_eq!(bubble.matched_fraction, 1.0);
        assert_eq!(bubble.liquidity_event_ids, [partial.event_id]);
        let total_event_match: Decimal = projection
            .liquidity_events
            .iter()
            .map(|event| event.matched_quantity)
            .sum();
        assert_eq!(total_event_match, bubble.matched_quantity);
    }

    #[test]
    fn evidence_toggles_hide_their_markers_but_keep_bubble_evidence() {
        let no_aligned = project_reductions(HeatmapConfig {
            show_aligned_depletion: false,
            ..event_config()
        });
        assert_eq!(no_aligned.liquidity_events.len(), 1);
        assert_eq!(
            no_aligned.liquidity_events[0].evidence,
            LiquidityEvidence::DepthOnly
        );
        // The hidden marker's evidence still reaches the bubble: association
        // ran, only the marker itself is off screen.
        assert_eq!(no_aligned.aggressions[0].matched_quantity, dec("3"));

        let no_unattributed = project_reductions(HeatmapConfig {
            show_unattributed_reductions: false,
            ..event_config()
        });
        assert_eq!(no_unattributed.liquidity_events.len(), 1);
        assert_eq!(
            no_unattributed.liquidity_events[0].evidence,
            LiquidityEvidence::AggressionAligned
        );

        // With both depletion layers off no association runs at all, and the
        // bubble honestly loses its consumption marks: there is no factual
        // reduction on screen (or off it) for the mark to point at.
        let neither = project_reductions(HeatmapConfig {
            show_aligned_depletion: false,
            show_unattributed_reductions: false,
            ..event_config()
        });
        assert!(neither.liquidity_events.is_empty());
        assert_eq!(neither.aggressions[0].matched_quantity, Decimal::ZERO);
    }

    #[test]
    fn hidden_liquidity_clears_cells_but_keeps_reference_and_markers() {
        let projection = project_reductions(HeatmapConfig {
            show_liquidity: false,
            ..event_config()
        });
        assert!(projection.cells.is_empty());
        assert_eq!(
            projection.dropped_cells, 0,
            "hidden cells are a choice, not a cap drop"
        );
        assert_eq!(
            projection.liquidity_events.len(),
            2,
            "markers outlive the heat behind them"
        );
        assert!(
            projection.liquidity_reference > Decimal::ZERO,
            "the depletion floors must not move when the heat is hidden"
        );
    }

    #[test]
    fn side_toggles_hide_one_side_without_rescaling_the_other() {
        let projection_with = |show_buy: bool, show_sell: bool| {
            let mut history = LiquidityHistory::new(HeatmapConfig {
                bubble_cluster_ms: 0,
                bubble_dust_merge_ms: 0,
                show_buy_aggressions: show_buy,
                show_sell_aggressions: show_sell,
                bubbles: BubbleStyle {
                    size_reference: BubbleSizeReference::VisibleMax,
                    ..BubbleStyle::default()
                },
                ..config()
            });
            history.install_snapshot(100, 1, snapshot(10)).unwrap();
            history.record_aggression(&Trade {
                agg_id: 1,
                timestamp_ms: 300,
                price: dec("101"),
                quantity: dec("4"),
                side: Side::Buy,
            });
            history.record_aggression(&Trade {
                agg_id: 2,
                timestamp_ms: 400,
                price: dec("100"),
                quantity: dec("1"),
                side: Side::Sell,
            });
            history
                .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
                .unwrap();
            project(
                &history,
                &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
                PriceWindow::new(dec("98"), dec("103")).unwrap(),
            )
        };

        let both = projection_with(true, true);
        assert_eq!(both.aggressions.len(), 2);
        let sell_size = both
            .aggressions
            .iter()
            .find(|bubble| bubble.side == Side::Sell)
            .expect("the sell bubble draws with both sides on")
            .size;

        // A side switch is a display choice, so the projection still carries
        // both prints: what a frame *draws* is decided in the renderer
        // (`RenderContext::bubbles`), which is what lets the live strip read
        // the same clusters while a side — or the whole bubble layer — is
        // hidden. The invariant this test exists for survives that move: the
        // size reference saw both sides, so hiding the buys must not inflate
        // the sell that stayed.
        let only_sell = projection_with(false, true);
        assert_eq!(only_sell.aggressions.len(), 2);
        // Same clusters, print for print: the live strip buckets these, and a
        // bubble switch may not reshape its histogram.
        assert_eq!(
            both.aggressions
                .iter()
                .map(|a| a.agg_id)
                .collect::<Vec<_>>(),
            only_sell
                .aggressions
                .iter()
                .map(|a| a.agg_id)
                .collect::<Vec<_>>()
        );
        let hidden_buy_sell_size = only_sell
            .aggressions
            .iter()
            .find(|bubble| bubble.side == Side::Sell)
            .expect("the sell print is still a fact of the frame")
            .size;
        assert_eq!(hidden_buy_sell_size, sell_size);
    }

    #[test]
    fn hidden_gaps_emit_no_coverage_primitives() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            show_gaps: false,
            ..config()
        });
        history.install_snapshot(400, 1, snapshot(10)).unwrap();
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let projection = project(
            &history,
            &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
            PriceWindow::new(dec("98"), dec("103")).unwrap(),
        );
        // The same fixture with the flag on yields the
        // "book_unavailable_before_capture" span (see the test above).
        assert!(projection.gaps.is_empty());
    }

    /// Hiding the map is display-only, so the projection must stop producing
    /// depth work while the recorded history stays exactly where it was.
    #[test]
    fn a_hidden_depth_map_projects_no_depth_primitives() {
        let mut history = LiquidityHistory::new(config());
        history.install_snapshot(400, 1, snapshot(10)).unwrap();
        history
            .apply_delta(900, &BookDelta::new(10, 10, vec![], vec![]))
            .unwrap();
        let timeline = BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None);
        let prices = PriceWindow::new(dec("98"), dec("103")).unwrap();

        let shown = project(&history, &timeline, prices);
        assert!(!shown.cells.is_empty(), "the visible map has heat cells");
        assert!(!shown.gaps.is_empty(), "and its pre-capture boundary");

        // Hidden on the candles alone is *not* hidden: the tape draws the same
        // cells and the renderer clips them per pane, so the primitives have to
        // survive or the tape goes dark with the chart.
        history
            .update_config(HeatmapConfig {
                show_depth: false,
                ..config()
            })
            .unwrap();
        let candles_only = project(&history, &timeline, prices);
        assert!(
            !candles_only.cells.is_empty(),
            "the tape still draws the map, so its cells are still built"
        );

        // Hidden on both panes is hidden, and that is where the saving is.
        history
            .update_config(HeatmapConfig {
                show_depth: false,
                live_lane: LiveLaneStyle {
                    show_depth: false,
                    ..LiveLaneStyle::default()
                },
                ..config()
            })
            .unwrap();
        let hidden = project(&history, &timeline, prices);
        assert!(hidden.cells.is_empty());
        assert!(hidden.gaps.is_empty());
        assert!(
            hidden.liquidity_events.is_empty(),
            "no depth primitive survives a map hidden on every pane"
        );
    }

    // ----------------------------------------------------------------------
    // The budget conserves. A frame may fold marks together; it may never
    // delete one. And the two panes each answer for their own canvas: what
    // the candles draw can not decide what the tape draws, and the tape's
    // window can not decide what the candles draw.
    // ----------------------------------------------------------------------

    /// Total quantity a pane draws, so a test can compare ink against tape.
    fn drawn_quantity(projection: &HeatmapProjection, live: bool) -> Decimal {
        projection
            .aggressions
            .iter()
            .filter(|mark| mark.live == live)
            .map(|mark| mark.quantity)
            .sum()
    }

    /// A ladder of prints either side of the lane boundary, dense enough that
    /// any small budget bites.
    fn crowded_history(config: HeatmapConfig) -> LiquidityHistory {
        let mut history = LiquidityHistory::new(config);
        for i in 0..60_u64 {
            history.record_aggression(&Trade {
                agg_id: i + 1,
                timestamp_ms: 100 + i as i64 * 300,
                // Spread over price so no two prints share a bucket by luck.
                price: dec("100") + Decimal::from(i % 12),
                quantity: Decimal::from(i % 7 + 1),
                side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
            });
        }
        history
    }

    fn crowded_timeline() -> BarTimeline {
        let closed: Vec<Bar> = (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
        let partial = bar(18_000, 21_000);
        BarTimeline::from_bars(
            0,
            &closed,
            Some(&partial),
            Some(crate::orderflow::LiveEdge {
                now_ms: 18_100,
                window_ms: 3_000,
                reference_ms: 3_000,
                on_newest_bar: true,
            }),
        )
    }

    /// The heart of the mission: a bubble that is too much for the frame is
    /// folded into its neighbour, never deleted. A trader reads pressure off
    /// these marks, so a quantity that traded must still be on the canvas
    /// whatever the budget — carried by a bigger bubble if need be, but
    /// carried.
    #[test]
    fn the_budget_folds_and_never_deletes() {
        let tight = HeatmapConfig {
            max_aggression_primitives: 6,
            ..bubbles_only()
        };
        let history = crowded_history(tight);
        let timeline = crowded_timeline();
        let prices = PriceWindow::new(dec("98"), dec("120")).unwrap();
        let projection = project(&history, &timeline, prices);

        // The budget is a performance target, and correctness outranks it. A
        // fold may not cross a bar — a mark inside a bar's slot is a claim that
        // *that* bar took the volume it carries — so with more (side, bar)
        // groups than the budget has marks, the frame draws the extra marks
        // rather than misattribute volume. Six bars and two sides is a floor of
        // twelve, and the fixture sits on it.
        assert!(
            projection.aggressions.len() <= 12,
            "folded past the point where a fold would have to cross a bar: {} marks",
            projection.aggressions.len()
        );
        assert!(
            projection.aggressions.len() < 60,
            "the budget did not bite at all"
        );
        let recorded: Decimal = history.aggressions().map(|trade| trade.quantity).sum();
        let drawn = drawn_quantity(&projection, true) + drawn_quantity(&projection, false);
        assert_eq!(
            drawn, recorded,
            "every contract that traded is still on the canvas, folded but never dropped"
        );
        assert!(
            projection.folded_aggressions > 0,
            "a frame this far over budget has to report the folds it made"
        );
    }

    /// A folded mark says so. The trader must be able to tell a bubble that is
    /// one print from a bubble the budget squeezed out of several — reading
    /// the second as the first is reading a size that never traded at once.
    #[test]
    fn a_folded_mark_carries_how_many_it_stands_for() {
        let tight = HeatmapConfig {
            max_aggression_primitives: 6,
            ..bubbles_only()
        };
        let history = crowded_history(tight);
        let projection = project(
            &history,
            &crowded_timeline(),
            PriceWindow::new(dec("98"), dec("120")).unwrap(),
        );
        assert!(
            projection
                .aggressions
                .iter()
                .any(|mark| mark.folded_marks > 0),
            "a frame this far over budget has to have folded something"
        );
        for mark in &projection.aggressions {
            if mark.folded_marks > 0 {
                assert!(
                    mark.trade_count >= mark.folded_marks as usize,
                    "a fold of {} marks can not stand for fewer prints than that",
                    mark.folded_marks
                );
            }
        }
    }

    /// The tape's own budget is its own. Whatever the candles do with theirs —
    /// and a summarized chart spends it on pies carrying whole bars — the tape
    /// keeps drawing what it drew.
    #[test]
    fn each_pane_answers_for_its_own_budget() {
        let timeline = crowded_timeline();
        let prices = PriceWindow::new(dec("98"), dec("120")).unwrap();
        let ink_of = |limit: usize| {
            let history = crowded_history(HeatmapConfig {
                max_aggression_primitives: limit,
                ..bubbles_only()
            });
            let projection = project(&history, &timeline, prices);
            (
                drawn_quantity(&projection, true),
                drawn_quantity(&projection, false),
                projection.aggressions.iter().any(|mark| mark.live),
            )
        };
        let (roomy_lane, roomy_chart, roomy_has_lane) = ink_of(400);
        let (tight_lane, tight_chart, tight_has_lane) = ink_of(6);

        assert!(
            roomy_has_lane && tight_has_lane,
            "both frames must have a tape to compare"
        );
        assert_eq!(
            roomy_lane, tight_lane,
            "squeezing the budget changed how much the tape says traded"
        );
        assert_eq!(
            roomy_chart, tight_chart,
            "squeezing the budget changed how much the candles say traded"
        );
    }

    /// A fold never carries one bar's volume into another bar's slot.
    ///
    /// A bubble drawn inside a bar's slot is a claim about *that* bar. Merging
    /// a neighbour's prints into it would be a fabricated fact — the same
    /// reason `regionalize_clusters` keeps `bar_index` in its key — so the
    /// budget stops folding at that boundary and the frame carries the extra
    /// marks instead. Correctness outranks the performance target.
    #[test]
    fn a_fold_never_moves_volume_into_another_bars_slot() {
        let history = crowded_history(HeatmapConfig {
            max_aggression_primitives: 4,
            ..bubbles_only()
        });
        let timeline = crowded_timeline();
        let projection = project(
            &history,
            &timeline,
            PriceWindow::new(dec("98"), dec("120")).unwrap(),
        );

        for mark in projection.aggressions.iter().filter(|mark| !mark.live) {
            let first = timeline
                .locate(mark.first_timestamp_ms)
                .map(|position| position.bar_index);
            let last = timeline
                .locate(mark.last_timestamp_ms)
                .map(|position| position.bar_index);
            assert_eq!(
                first, last,
                "a candle mark spans two bars, so its slot claims volume the                  neighbouring bar traded"
            );
        }
        // And the pane really was squeezed past its budget, so the assertion
        // above was exercised rather than trivially true.
        assert!(
            projection
                .aggressions
                .iter()
                .any(|mark| mark.folded_marks > 1),
            "a budget of four over sixty prints has to have folded something"
        );
    }

    /// Widening the tape buys it more marks, because marks need room.
    ///
    /// The split is the lane's own width share rather than a constant of its
    /// own, so the one control the trader already has over the tape moves both
    /// its band and its budget. Without this the number would be a magic
    /// constant that disagrees with the canvas the moment the divider moves.
    #[test]
    fn the_tape_budget_follows_the_room_the_tape_was_given() {
        let lane_of = |share: f32| LiveLaneStyle {
            enabled: true,
            width_share: share,
            ..LiveLaneStyle::default()
        };
        let (narrow_chart, narrow_lane) = pane_budgets(100, &lane_of(MIN_LIVE_LANE_SHARE));
        let (wide_chart, wide_lane) = pane_budgets(100, &lane_of(MAX_LIVE_LANE_SHARE));
        assert!(
            wide_lane > narrow_lane,
            "a wider tape has room for more marks and did not get them"
        );
        assert!(
            wide_chart < narrow_chart,
            "and it takes that room from the candles, not from thin air"
        );
        for share in [
            f32::NAN,
            -1.0,
            5.0,
            MIN_LIVE_LANE_SHARE,
            MAX_LIVE_LANE_SHARE,
        ] {
            let (chart, lane) = pane_budgets(100, &lane_of(share));
            assert!(chart >= 2 && lane >= 2, "every pane can draw both sides");
            assert!(
                chart + lane <= 100,
                "the two shares together overspent the frame's budget"
            );
        }
        // A budget too small to split still leaves both panes able to draw.
        let (chart, lane) = pane_budgets(1, &lane_of(DEFAULT_LIVE_LANE_SHARE));
        assert!(chart >= 2 && lane >= 2);

        // With the tape switched off there is no second pane to protect, and
        // reserving a share for a band nobody is drawing would fold the candles
        // harder for nothing.
        let (all, none) = pane_budgets(
            100,
            &LiveLaneStyle {
                enabled: false,
                ..LiveLaneStyle::default()
            },
        );
        assert_eq!((all, none), (100, 0), "the candles get the whole budget");
    }

    /// The fold is paid where it costs least, and the rest of the pane is
    /// untouched.
    ///
    /// This is the rule the product owner chose over "merge everything
    /// evenly": on the tape the newest prints at the right edge are what a
    /// scalper is reading right now, so they stay one mark per execution and
    /// the left edge — where a print was sliding out anyway — carries the
    /// loss. On the candles the big prints carry the story, so the small ones
    /// fold and the big ones are left exactly as they were.
    #[test]
    fn the_fold_spares_the_newest_on_the_tape_and_the_biggest_on_the_candles() {
        // Two budgets, because the sparing rule is about a pane that is *over*
        // its budget rather than swamped: past half over, everything folds and
        // there is no tail left to spare. This one squeezes the tape.
        let tape_pressure = project(
            &crowded_history(HeatmapConfig {
                max_aggression_primitives: 20,
                ..bubbles_only()
            }),
            &crowded_timeline(),
            PriceWindow::new(dec("98"), dec("120")).unwrap(),
        );
        // And this one squeezes only the candles.
        let projection = project(
            &crowded_history(HeatmapConfig {
                max_aggression_primitives: 62,
                ..bubbles_only()
            }),
            &crowded_timeline(),
            PriceWindow::new(dec("98"), dec("120")).unwrap(),
        );

        let lane: Vec<_> = tape_pressure
            .aggressions
            .iter()
            .filter(|mark| mark.live)
            .collect();
        assert!(lane.len() >= 2, "the tape needs marks to compare");
        let newest = lane
            .iter()
            .max_by_key(|mark| mark.last_timestamp_ms)
            .expect("the tape has a newest mark");
        assert_eq!(
            newest.folded_marks, 0,
            "the newest print on the tape was folded into something else"
        );

        let candles: Vec<_> = projection
            .aggressions
            .iter()
            .filter(|mark| !mark.live)
            .collect();
        // A fold sums, so a fold of four small prints can out-weigh the
        // biggest single one — that is arithmetic, not a lost print. What has
        // to hold is that the biggest *print* is still drawn as itself: the
        // ladder tops out at seven contracts, and seven has to be on the canvas
        // as one untouched mark.
        assert!(candles.len() > 1, "the candles need marks to compare");
        assert_eq!(
            candles
                .iter()
                .filter(|mark| mark.folded_marks == 0)
                .map(|mark| mark.quantity)
                .max(),
            Some(dec("7")),
            "the biggest print on the candles was folded away"
        );
    }

    /// Zooming the candles is a statement about the candles. Every mark in the
    /// lane must come out of the projection identical — same count, same
    /// quantities, same folds — because nothing about the tape changed.
    #[test]
    fn zooming_the_chart_leaves_every_lane_primitive_alone() {
        let history = crowded_history(HeatmapConfig {
            display_grouping: DisplayGrouping::Adaptive { target_rows: 8 },
            ..bubbles_only()
        });
        let timeline = crowded_timeline();
        let lane_of = |prices: PriceWindow| {
            project(&history, &timeline, prices)
                .aggressions
                .iter()
                .filter(|mark| mark.live)
                .map(|mark| {
                    (
                        mark.side,
                        mark.price_bucket,
                        mark.quantity,
                        mark.trade_count,
                    )
                })
                .collect::<Vec<_>>()
        };
        // Both windows hold every price the ladder traded at, so the only
        // thing that differs between the two frames is the adaptive grouping
        // the candles resolved — which is exactly what must not reach the tape.
        let zoomed_in = lane_of(PriceWindow::new(dec("95"), dec("135")).unwrap());
        let zoomed_out = lane_of(PriceWindow::new(dec("90"), dec("240")).unwrap());
        assert!(!zoomed_in.is_empty(), "the tape must have marks to compare");
        assert_eq!(
            zoomed_in, zoomed_out,
            "the candles' zoom decided what the tape shows"
        );
    }

    /// And the mirror: the tape's window is a statement about the tape. Widen
    /// it and the candles must draw exactly what they drew — the prints behind
    /// the seam are the same prints, and they belong to the same bars.
    #[test]
    fn changing_the_tape_window_leaves_every_chart_primitive_alone() {
        let history = crowded_history(bubbles_only());
        let prices = PriceWindow::new(dec("98"), dec("120")).unwrap();
        let chart_of = |window_ms: i64| {
            let closed: Vec<Bar> = (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
            let partial = bar(18_000, 21_000);
            let timeline = BarTimeline::from_bars(
                0,
                &closed,
                Some(&partial),
                Some(crate::orderflow::LiveEdge {
                    now_ms: 18_100,
                    window_ms,
                    reference_ms: 3_000,
                    on_newest_bar: true,
                }),
            );
            project(&history, &timeline, prices)
                .aggressions
                .iter()
                .map(|mark| (mark.live, mark.quantity))
                .collect::<Vec<_>>()
        };
        let quick = chart_of(2_000);
        let slow = chart_of(9_000);
        assert!(
            !quick.is_empty() && !slow.is_empty(),
            "the candles must have marks to compare"
        );
        // Prints legitimately *move* between the panes as the tape's window
        // grows — that is what the window means. What may never happen is a
        // print leaving the canvas: whatever the speed, the two panes together
        // still account for every contract that traded.
        assert_eq!(
            quick.iter().map(|mark| mark.1).sum::<Decimal>(),
            slow.iter().map(|mark| mark.1).sum::<Decimal>(),
            "changing the tape's speed changed how much the frame says traded"
        );
    }

    /// The mission's invariant, over the whole grid a trader can put the chart
    /// in: every grouping mode crossed with every tape speed, and both budget
    /// regimes.
    ///
    /// For each cell: the contracts drawn, plus the contracts an explicit
    /// display floor removed, equal the contracts that traded inside the
    /// window. Nothing else may go missing, whatever the zoom or the speed.
    #[test]
    fn conservation_holds_across_every_grouping_and_every_tape_speed() {
        let groupings = [
            DisplayGrouping::Native,
            DisplayGrouping::Multiple(4),
            DisplayGrouping::Adaptive { target_rows: 8 },
            DisplayGrouping::Adaptive { target_rows: 160 },
        ];
        let speeds = [1_500_i64, 3_000, 9_000, 21_000];
        // Roomy enough that nothing folds, and tight enough that everything
        // does — the invariant may not notice the difference.
        let budgets = [4_usize, 400];
        let prices = PriceWindow::new(dec("90"), dec("140")).unwrap();

        for display_grouping in groupings {
            for window_ms in speeds {
                for max_aggression_primitives in budgets {
                    let history = crowded_history(HeatmapConfig {
                        display_grouping,
                        max_aggression_primitives,
                        ..bubbles_only()
                    });
                    let closed: Vec<Bar> =
                        (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
                    let partial = bar(18_000, 21_000);
                    let timeline = BarTimeline::from_bars(
                        0,
                        &closed,
                        Some(&partial),
                        Some(crate::orderflow::LiveEdge {
                            now_ms: 18_100,
                            window_ms,
                            reference_ms: 3_000,
                            on_newest_bar: true,
                        }),
                    );
                    let projection = project(&history, &timeline, prices);
                    let cell = format!(
                        "grouping {display_grouping:?}, window {window_ms} ms, budget \
                         {max_aggression_primitives}"
                    );

                    // Every print of the fixture is inside this price window and
                    // inside the timeline, so the tape is the whole retained set.
                    let traded: Decimal = history.aggressions().map(|trade| trade.quantity).sum();
                    let drawn: Decimal = projection
                        .aggressions
                        .iter()
                        .map(|mark| mark.quantity)
                        .sum();
                    assert_eq!(
                        drawn + projection.floored_quantity,
                        traded,
                        "contracts went missing at {cell}"
                    );
                    assert_eq!(
                        projection.floored_quantity,
                        Decimal::ZERO,
                        "no floor is set in this fixture, so nothing may be floored at {cell}"
                    );
                    // And a fold never crossed a bar to get there.
                    for mark in projection.aggressions.iter().filter(|mark| !mark.live) {
                        assert_eq!(
                            timeline
                                .locate(mark.first_timestamp_ms)
                                .map(|position| position.bar_index),
                            timeline
                                .locate(mark.last_timestamp_ms)
                                .map(|position| position.bar_index),
                            "a candle mark spans two bars at {cell}"
                        );
                    }
                }
            }
        }
    }

    /// The one discard left is the trader's own, and the frame says how big it
    /// is — in contracts, because what matters is the size of what is missing
    /// and not the number of dots.
    #[test]
    fn the_display_floor_is_the_only_thing_missing_and_it_is_declared() {
        let floored = project(
            &crowded_history(HeatmapConfig {
                bubbles: BubbleStyle {
                    min_quantity: 5.0,
                    readable_min_radius: 0.0,
                    ..BubbleStyle::default()
                },
                ..bubbles_only()
            }),
            &crowded_timeline(),
            PriceWindow::new(dec("90"), dec("140")).unwrap(),
        );
        let history = crowded_history(bubbles_only());
        let traded: Decimal = history.aggressions().map(|trade| trade.quantity).sum();
        let drawn: Decimal = floored.aggressions.iter().map(|mark| mark.quantity).sum();

        assert!(
            floored.floored_quantity > Decimal::ZERO,
            "a floor of five over a ladder of ones has to remove something"
        );
        assert_eq!(
            drawn + floored.floored_quantity,
            traded,
            "the floor's residue does not account for what is off the canvas"
        );
        for mark in &floored.aggressions {
            assert!(
                mark.quantity >= dec("5"),
                "a mark under the floor survived it"
            );
        }
    }

    /// Reversibility, on both axes. The projection is a function of the window
    /// it is handed, so a trader who zooms out to look around — or slows the
    /// tape down and speeds it back up — finds exactly the frame they left.
    #[test]
    fn returning_to_a_window_or_a_speed_returns_its_frame() {
        let history = crowded_history(HeatmapConfig {
            display_grouping: DisplayGrouping::Adaptive { target_rows: 8 },
            max_aggression_primitives: 12,
            ..bubbles_only()
        });
        let frame_at = |window_ms: i64, prices: PriceWindow| {
            let closed: Vec<Bar> = (0..6).map(|i| bar(i * 3_000, i * 3_000 + 2_999)).collect();
            let partial = bar(18_000, 21_000);
            let timeline = BarTimeline::from_bars(
                0,
                &closed,
                Some(&partial),
                Some(crate::orderflow::LiveEdge {
                    now_ms: 18_100,
                    window_ms,
                    reference_ms: 3_000,
                    on_newest_bar: true,
                }),
            );
            project(&history, &timeline, prices).aggressions
        };
        let home = PriceWindow::new(dec("99"), dec("104")).unwrap();
        let away = PriceWindow::new(dec("90"), dec("140")).unwrap();

        let first = frame_at(3_000, home);
        let _ = frame_at(3_000, away);
        assert_eq!(
            first,
            frame_at(3_000, home),
            "the same price window gave a different frame the second time"
        );

        let quick = frame_at(1_500, away);
        let _ = frame_at(21_000, away);
        assert_eq!(
            quick,
            frame_at(1_500, away),
            "the same tape speed gave a different frame the second time"
        );
    }

    /// No hysteresis: the projection is a function of the window it is given,
    /// so coming back to a window comes back to its frame. A trader who zooms
    /// out to look around and zooms back in must find the tape they left.
    #[test]
    fn returning_to_a_window_returns_its_frame() {
        let history = crowded_history(HeatmapConfig {
            display_grouping: DisplayGrouping::Adaptive { target_rows: 8 },
            max_aggression_primitives: 12,
            ..bubbles_only()
        });
        let timeline = crowded_timeline();
        let home = PriceWindow::new(dec("99"), dec("104")).unwrap();
        let away = PriceWindow::new(dec("90"), dec("140")).unwrap();
        let first = project(&history, &timeline, home);
        let _ = project(&history, &timeline, away);
        let back = project(&history, &timeline, home);
        assert_eq!(
            first.aggressions, back.aggressions,
            "the same window gave a different frame the second time"
        );
    }

    /// Wall-clock cost of one projection over a dense tape, printed for the
    /// mission's performance gate. Ignored by default: it is a measurement,
    /// not an assertion.
    #[test]
    #[ignore]
    fn bench_projection_over_a_dense_tape() {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            display_grouping: DisplayGrouping::Adaptive { target_rows: 128 },
            bubble_candle_summary: true,
            ..bubbles_only()
        });
        // 40 000 prints over 200 bars: a busy session, well past the budget.
        for i in 0..40_000_u64 {
            history.record_aggression(&Trade {
                agg_id: i + 1,
                timestamp_ms: 100 + i as i64 * 25,
                price: dec("100") + Decimal::from(i % 400),
                quantity: Decimal::from(i % 23 + 1),
                side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
            });
        }
        let closed: Vec<Bar> = (0..200)
            .map(|i| bar(i * 5_000, i * 5_000 + 4_999))
            .collect();
        let partial = bar(1_000_000, 1_005_000);
        let timeline = BarTimeline::from_bars(
            0,
            &closed,
            Some(&partial),
            Some(crate::orderflow::LiveEdge {
                now_ms: 1_000_100,
                window_ms: 5_000,
                reference_ms: 5_000,
                on_newest_bar: true,
            }),
        );
        let prices = PriceWindow::new(dec("90"), dec("520")).unwrap();

        // Warm the caches the allocator and the CPU keep.
        for _ in 0..3 {
            let _ = project(&history, &timeline, prices);
        }
        let runs = 30;
        let started = std::time::Instant::now();
        let mut marks = 0;
        for _ in 0..runs {
            marks = project(&history, &timeline, prices).aggressions.len();
        }
        let per_run = started.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
        eprintln!("BENCH projection_ms_per_frame={per_run:.3} marks={marks}");
    }
}
