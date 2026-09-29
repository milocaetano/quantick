//! What a painter may keep of the native tape from one frame to the next.
//!
//! A tape window of many minutes holds tens of thousands of native cells, and
//! every one of them used to be re-read, re-folded and re-placed on every
//! frame although only the newest few can still change. The worker therefore
//! seals the cells before its newest window: a later publication of the same
//! [`TapeLineage`] carries them unchanged, so the painter reconciles only what
//! lies after the seal. The seal is proven rather than assumed — each
//! publication is compared with the one before it below that one's seal, and
//! any difference (a late print, a late reduction, a new configuration, a
//! new price grid) starts a new lineage, which makes the painter reread
//! everything once.
//!
//! The accepted prints the worker has not published yet travel beside the
//! published frame as a [`TapeOverlay`]: the cells they touch, folded exactly
//! as the complete pending projection folds them. That projection stays
//! available, built only when a reader asks for it.

use std::sync::{Arc, OnceLock};

use quantick_engine::{Side, Trade};
use rust_decimal::Decimal;

use super::dots::window_start;
use super::{AggressionPrimitive, HeatmapProjection, PriceWindow, TapeFacts, VolumeDots, pending};
use crate::HeatmapConfig;
use crate::config::theme::OrderflowRenderStyle;
use crate::interaction::AggressionCluster;
use crate::timeline::BarTimeline;

/// One run of publications that agree on every sealed cell. Compared by
/// allocation, never by value: two runs are never the same run.
#[derive(Debug, Default, PartialEq)]
pub struct TapeLineage;

/// The part of a published tape a painter may keep from an earlier frame of
/// the same lineage.
#[derive(Debug, Clone, PartialEq)]
pub struct TapeSeal {
    /// Frames sharing this allocation agree on every cell before
    /// [`through_ms`](Self::through_ms), eviction at the front aside.
    pub lineage: Arc<TapeLineage>,
    /// Native windows starting before this instant are sealed.
    pub through_ms: i64,
    /// The worker's configuration revision the cells were folded under.
    pub revision: u64,
    /// The native window the cells are keyed on.
    pub window_ms: i64,
    /// The native price bucket the cells are keyed on. The capture grid can
    /// change without a configuration revision — auto-sized from the tape's
    /// prints — and cells on another grid are other cells.
    pub native_width: Decimal,
    /// Whether any cell carries depth evidence. Such a cell is drawn by the
    /// depth map too, at the frame's own placement, so only a complete
    /// frame may carry pending prints beside it.
    pub evidence: bool,
    /// Where the tape the cells were folded for began.
    pub lane_from_ms: Option<i64>,
    /// Prints the worker's history had accepted when the cells were folded.
    pub recorded: u64,
}

/// What the worker knew when it sealed a tape, beside the cells themselves.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SealInputs {
    /// The worker's configuration revision.
    pub(crate) revision: u64,
    /// The native window the cells are keyed on.
    pub(crate) window_ms: i64,
    /// The native price bucket the cells are keyed on.
    pub(crate) native_width: Decimal,
    /// Cells before this instant's window are sealed; `None` seals nothing.
    pub(crate) seal_from_ms: Option<i64>,
    /// Where the tape began.
    pub(crate) lane_from_ms: Option<i64>,
    /// Prints the history had accepted.
    pub(crate) recorded: u64,
}

/// Stamp `next` with the seal a painter may trust.
///
/// Cells before the window of `seal_from_ms` are sealed: prints arrive in
/// time order, so none of them should change again. When one does anyway,
/// the comparison with `previous` below its own seal catches it and the new
/// frame starts a new lineage.
pub(crate) fn seal_tape(next: &mut TapeFacts, previous: Option<&TapeFacts>, inputs: SealInputs) {
    let SealInputs {
        revision,
        window_ms,
        native_width,
        seal_from_ms,
        lane_from_ms,
        recorded,
    } = inputs;
    let Some(seal_from_ms) = seal_from_ms else {
        next.seal = None;
        return;
    };
    let through_ms = window_start(seal_from_ms, window_ms);
    let evidence = next
        .clusters
        .iter()
        .any(|cell| cell.matched_quantity > Decimal::ZERO || !cell.liquidity_event_ids.is_empty());
    let lineage = previous
        .and_then(|previous| {
            let seal = previous.seal.as_ref()?;
            (seal.revision == revision
                && seal.window_ms == window_ms
                && seal.native_width == native_width
                && sealed_cells(previous, next.clusters.first(), seal.through_ms, window_ms)
                    == sealed_cells(next, next.clusters.first(), seal.through_ms, window_ms))
            .then(|| Arc::clone(&seal.lineage))
        })
        .unwrap_or_default();
    next.seal = Some(TapeSeal {
        lineage,
        through_ms,
        revision,
        window_ms,
        native_width,
        evidence,
        lane_from_ms,
        recorded,
    });
}

/// The cells of `facts` from the window of `front` (the newer frame's oldest
/// cell) up to `through_ms`. Cells are ordered by their first execution, so a
/// window is one contiguous run.
fn sealed_cells<'a>(
    facts: &'a TapeFacts,
    front: Option<&AggressionCluster>,
    through_ms: i64,
    window_ms: i64,
) -> &'a [AggressionCluster] {
    let window = |cell: &AggressionCluster| window_start(cell.first_timestamp_ms, window_ms);
    let front = front.map_or(through_ms, window).min(through_ms);
    let low = facts.clusters.partition_point(|cell| window(cell) < front);
    let high = facts
        .clusters
        .partition_point(|cell| window(cell) < through_ms);
    &facts.clusters[low..high.max(low)]
}

/// The accepted prints a published frame does not hold yet, as the native
/// cells they touch.
#[derive(Debug)]
pub struct TapeOverlay {
    /// The touched cells after folding in the published cell of the same key:
    /// each supersedes that published cell.
    pub cells: Vec<AggressionCluster>,
    /// The pending lane's start: published cells before it are left out.
    pub from_ms: i64,
    /// The eviction horizon across the published frame and the suffix.
    pub evicted_through_ms: Option<i64>,
    /// The display floor a cell must reach to be drawn.
    pub floor: Decimal,
    /// Recorded opening windows across the published frame and the suffix.
    pub opening_bursts: Vec<i64>,
    pub(crate) complete: PendingInputs,
    projection: OnceLock<HeatmapProjection>,
}

/// Everything the complete pending projection is built from, kept so a
/// reader that needs it gets exactly the frame it used to.
#[derive(Debug)]
pub(crate) struct PendingInputs {
    pub(crate) trades: Vec<Trade>,
    pub(crate) published: Arc<HeatmapProjection>,
    pub(crate) config: HeatmapConfig,
    pub(crate) timeline: BarTimeline,
    pub(crate) prices: PriceWindow,
    pub(crate) dots: VolumeDots,
    /// `(regions, published slots)` when the tape moves into the published
    /// frame's lane.
    pub(crate) lanes: Option<(usize, usize)>,
}

impl TapeOverlay {
    pub(crate) fn new(
        cells: Vec<AggressionCluster>,
        from_ms: i64,
        evicted_through_ms: Option<i64>,
        opening_bursts: Vec<i64>,
        complete: PendingInputs,
    ) -> Self {
        Self {
            cells,
            from_ms,
            evicted_through_ms,
            floor: complete
                .config
                .bubbles
                .min_quantity_decimal()
                .unwrap_or_default(),
            opening_bursts,
            complete,
            projection: OnceLock::new(),
        }
    }

    /// The complete pending projection: the published frame with every
    /// pending print placed on it, exactly as it was built before the
    /// overlay existed. Built on first use.
    pub fn projection(&self) -> &HeatmapProjection {
        self.projection.get_or_init(|| {
            let inputs = &self.complete;
            let mut projection = pending::project_suffix(
                inputs.trades.iter(),
                self.evicted_through_ms,
                self.opening_bursts.clone(),
                Some(inputs.published.as_ref()),
                &inputs.config,
                &inputs.timeline,
                inputs.prices,
                &inputs.dots,
            );
            if let Some((regions, slots)) = inputs.lanes {
                into_lane_of(&mut projection, regions, slots);
            }
            projection
        })
    }
}

/// Move the tape's marks from the lane of a frame split into `from` regions
/// to the lane of one split into `to`: the lane is the last region, and a
/// mark keeps its fraction of it. Only the pending tape's marks are live
/// here; the published candle marks already speak in `to` regions.
pub(crate) fn into_lane_of(projection: &mut HeatmapProjection, from: usize, to: usize) {
    let relabel = lane_relabel(from, to);
    for mark in projection.aggressions.iter_mut().filter(|mark| mark.live) {
        mark.x = relabel(mark.x);
    }
    projection.live_now_x = projection.live_now_x.map(relabel);
}

/// [`into_lane_of`]'s mapping of one normalized x.
pub(crate) fn lane_relabel(from: usize, to: usize) -> impl Fn(f64) -> f64 {
    let (from, to) = (from as f64, to as f64);
    move |x: f64| (to - 1.0 + (x * from - (from - 1.0))) / to
}

/// Whether the painter draws `mark` as a bubble: its pane's switch is on, it
/// does not claim a side the canvas hides, and its side is shown.
///
/// A projection is the fact the frame observed, and more than one surface
/// reads it: the bubbles, the consumption carve, and the live strip's
/// histogram. Each renderer therefore decides what it draws. A mark carrying
/// both sides — a merged cluster, or a bar summary — is sized by the two
/// together, so with one side hidden its area would state a quantity the
/// canvas is not showing; it is withheld rather than drawn at a lie of a size.
#[must_use]
pub fn draws_bubble(style: &OrderflowRenderStyle, mark: &AggressionPrimitive) -> bool {
    let pane = if mark.live {
        style.lane_aggression_layer
    } else {
        style.aggression_layer
    };
    if !pane {
        return false;
    }
    if !(style.show_buy && style.show_sell) && mark.buy_share > 0.0 && mark.buy_share < 1.0 {
        return false;
    }
    match mark.side {
        Side::Buy => style.show_buy,
        Side::Sell => style.show_sell,
    }
}
