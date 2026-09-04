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
    /// A frame with nothing to draw: the seed the chart's render tests build a
    /// projection from, so they exercise the same struct the pipeline emits.
    /// Not `cfg(test)`: those tests live in the crate that links this one.
    ///
    /// The pipeline itself starts from [`SettledProjection::empty`] — it always
    /// has a live half to attach, even when that half is empty too.
    pub fn empty(enabled: bool, effective_grouping: EffectiveGrouping) -> Self {
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
    pub fn empty(enabled: bool, effective_grouping: EffectiveGrouping) -> Self {
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

/// Shared with the live strip in the chart, which normalizes its
/// depth rows against the same reference the heatmap cells used, so one wall
/// reads with one colour on both sides of the chart edge.
pub fn normalized_log_intensity(quantity: Decimal, reference: Decimal, gamma: f32) -> f32 {
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
pub fn normalized_area_size(quantity: Decimal, reference: Decimal) -> f32 {
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
mod tests;
