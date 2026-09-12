//! The read model of a projection: the normalized primitives a renderer
//! draws, and the two halves of a frame they arrive in.
//!
//! Nothing here walks the tape or the book. The pipeline in the parent
//! module builds these values; [`SettledProjection::with_live`] is the one
//! operation they carry themselves, because joining the two halves is a
//! statement about the frame's shape, not about how either half was built.

use std::sync::Arc;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive as _;

use super::cap_events;
use crate::config::HeatmapConfig;
use crate::grouping::EffectiveGrouping;
use crate::history::{AggressorSide, RestingSide};
use crate::interaction::{LiquidityEvent, LiquidityEvidence};

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
