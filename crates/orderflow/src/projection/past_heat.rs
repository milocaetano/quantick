//! The book beside a tape held at a past instant.
//!
//! The live frame's bands are today's book on today's clock, so a past tape
//! cannot borrow them. It reads the runs the history kept for its own blocks
//! instead, cut by the same rules as the live frame ([`super::heat_cells`])
//! and coloured against its own book, and places them on the tape's clock
//! when it is drawn: a drag moves the bands with the prints and re-reads
//! nothing.

use std::sync::Arc;

use rust_decimal::Decimal;

use super::heat_cells::{DraftCell, book_gaps, bucket_rows, finish_cells, liquidity_reference};
use super::model::{BOOK_PENDING, GapPrimitive, HeatmapCell, HeatmapProjection, PriceWindow};
use crate::grouping::{EffectiveGrouping, GroupingWindow, sweep_grouped_runs};
use crate::history::LiquidityHistory;

/// The recorded book over a past tape's blocks. Horizontal positions are
/// fractions of `[from_ms, until_ms]`, not chart positions: see
/// [`placed`](Self::placed).
#[derive(Debug, Clone, PartialEq)]
pub struct PastHeat {
    /// Half-open stretch of market time the bands were cut from.
    pub from_ms: i64,
    pub until_ms: i64,
    /// The price window the bands' `y` were normalized on.
    pub prices: PriceWindow,
    /// The quantity a band reads full against, from this book alone: the
    /// live book moving never recolours a held past.
    pub liquidity_reference: Decimal,
    pub effective_grouping: EffectiveGrouping,
    /// Resting runs; `x` in `[0, 1]` over `[from_ms, until_ms]`.
    pub cells: Vec<HeatmapCell>,
    /// Where no book was recorded; `x` likewise.
    pub gaps: Vec<GapPrimitive>,
}

impl PastHeat {
    /// Nothing to draw: the tape's map is off, or the history holds no book.
    #[must_use]
    pub fn empty(
        from_ms: i64,
        until_ms: i64,
        prices: PriceWindow,
        effective_grouping: EffectiveGrouping,
    ) -> Self {
        Self {
            from_ms,
            until_ms,
            prices,
            liquidity_reference: Decimal::ZERO,
            effective_grouping,
            cells: Vec::new(),
            gaps: Vec::new(),
        }
    }

    /// Whether these bands still answer for `prices` and `grouping`: their
    /// only inputs besides the frozen history.
    #[must_use]
    pub fn fits(&self, prices: PriceWindow, grouping: EffectiveGrouping) -> bool {
        self.prices == prices && self.effective_grouping == grouping
    }

    /// The bands on a frame of `slot_count` regions whose last region is the
    /// tape, showing the `window_ms` of market time that ends at `end_ms`:
    /// clipped to that window, and to nothing else. A stretch of the window
    /// these blocks do not reach — a drag outran them — is a
    /// [`BOOK_PENDING`] gap, never an empty book.
    ///
    /// Every `x` is at or right of the lane's opening: the caller places them
    /// on the tape alone, the divider included.
    #[must_use]
    pub fn placed(&self, end_ms: i64, window_ms: i64, slot_count: usize) -> HeatmapProjection {
        let mut projection = HeatmapProjection::empty(true, self.effective_grouping);
        projection.liquidity_reference = self.liquidity_reference;
        let Some(place) =
            Placement::new(self.from_ms, self.until_ms, end_ms, window_ms, slot_count)
        else {
            return projection;
        };
        projection.cells = Arc::new(
            self.cells
                .iter()
                .filter_map(|cell| {
                    let (x0, x1) = place.span(cell.x0, cell.x1)?;
                    Some(HeatmapCell {
                        x0,
                        x1,
                        ..cell.clone()
                    })
                })
                .collect(),
        );
        let mut gaps: Vec<GapPrimitive> = self
            .gaps
            .iter()
            .filter_map(|gap| {
                let (x0, x1) = place.span(gap.x0, gap.x1)?;
                Some(GapPrimitive {
                    x0,
                    x1,
                    ..gap.clone()
                })
            })
            .collect();
        let start_ms = end_ms.saturating_sub(window_ms.max(1));
        for (from, to) in [
            (start_ms, self.from_ms.min(end_ms)),
            (self.until_ms.max(start_ms), end_ms),
        ] {
            let (x0, x1) = (place.at(from as f64), place.at(to as f64));
            if x1 > x0 {
                gaps.push(pending_gap(x0, x1));
            }
        }
        gaps.sort_by(|a, b| a.x0.total_cmp(&b.x0).then_with(|| a.x1.total_cmp(&b.x1)));
        projection.gaps = Arc::new(gaps);
        projection
    }
}

/// [`PastHeat::placed`] kept for the clock it was placed on: the painter and
/// the cursor ask every UI frame, and a held tape answers with the same
/// bands until its book or its clock moves.
#[derive(Debug, Default)]
pub struct PlacedPastHeat {
    held: Option<(PlacedKey, Arc<HeatmapProjection>)>,
}

/// The book placed, and the clock: end, window and the frame's regions.
type PlacedKey = (Option<Arc<PastHeat>>, (i64, i64, usize));

impl PlacedPastHeat {
    /// `heat` placed on `clock` — `(end_ms, window_ms, slot_count)` — and
    /// re-placed only when one of them changed. With no book published yet
    /// the whole held window is [`BOOK_PENDING`] on `grouping`.
    pub fn place(
        &mut self,
        heat: Option<&Arc<PastHeat>>,
        clock: (i64, i64, usize),
        grouping: EffectiveGrouping,
    ) -> Arc<HeatmapProjection> {
        if let Some(((held, at), placed)) = &self.held
            && *at == clock
            && match (held, heat) {
                (Some(held), Some(heat)) => Arc::ptr_eq(held, heat),
                (None, None) => true,
                _ => false,
            }
        {
            return Arc::clone(placed);
        }
        let (end_ms, window_ms, slot_count) = clock;
        let placed = Arc::new(match heat {
            Some(heat) => heat.placed(end_ms, window_ms, slot_count),
            None => {
                let mut unread = HeatmapProjection::empty(true, grouping);
                if let Some(place) = Placement::new(end_ms, end_ms, end_ms, window_ms, slot_count) {
                    let lane = (place.lane_x0, place.lane_x0 + place.lane_width);
                    unread.gaps = Arc::new(vec![pending_gap(lane.0, lane.1)]);
                }
                unread
            }
        });
        self.held = Some(((heat.cloned(), clock), Arc::clone(&placed)));
        placed
    }
}

/// The map from market time to a position on the frame's tape.
struct Placement {
    from_ms: f64,
    span_ms: f64,
    window_start_ms: f64,
    window_ms: f64,
    lane_x0: f64,
    lane_width: f64,
}

impl Placement {
    fn new(
        from_ms: i64,
        until_ms: i64,
        end_ms: i64,
        window_ms: i64,
        slot_count: usize,
    ) -> Option<Self> {
        let (span_ms, window_ms) = (until_ms - from_ms, window_ms.max(1));
        (slot_count >= 1 && span_ms >= 0).then(|| {
            let regions = slot_count as f64;
            Self {
                from_ms: from_ms as f64,
                span_ms: span_ms as f64,
                window_start_ms: end_ms.saturating_sub(window_ms) as f64,
                window_ms: window_ms as f64,
                lane_x0: (regions - 1.0) / regions,
                lane_width: 1.0 / regions,
            }
        })
    }

    /// Where `time_ms` sits on the tape, clamped to its window.
    fn at(&self, time_ms: f64) -> f64 {
        let unit = ((time_ms - self.window_start_ms) / self.window_ms).clamp(0.0, 1.0);
        self.lane_x0 + unit * self.lane_width
    }

    /// One `[x0, x1]` span of the blocks on the tape; `None` off the window.
    fn span(&self, x0: f64, x1: f64) -> Option<(f64, f64)> {
        let time = |x: f64| self.from_ms + x * self.span_ms;
        let (left, right) = (self.at(time(x0)), self.at(time(x1)));
        (right > left).then_some((left, right))
    }
}

/// Read the book the history kept over `[from_ms, until_ms)` the way the live
/// frame reads its own ([`super::heat_cells`]), on `grouping`, and coloured
/// against these runs.
#[must_use]
pub fn project_past_heat(
    history: &LiquidityHistory,
    from_ms: i64,
    until_ms: i64,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
) -> PastHeat {
    let mut heat = PastHeat::empty(from_ms, until_ms, prices, grouping);
    let config = history.config();
    let span_ms = until_ms - from_ms;
    if !config.lane_depth_drawn() || span_ms <= 0 {
        return heat;
    }
    let fraction =
        |time_ms: i64| (time_ms.clamp(from_ms, until_ms) - from_ms) as f64 / span_ms as f64;
    let start_ms = history
        .retention_start_ms()
        .map_or(from_ms, |retained| retained.max(from_ms));
    let grouped = sweep_grouped_runs(
        history.runs_intersecting(start_ms, until_ms),
        history.coverage_segments(),
        grouping,
        GroupingWindow {
            start_ms,
            end_ms: until_ms,
            open_run_end_ms: history.latest_book_ms().unwrap_or(until_ms),
            price_low: prices.low,
            price_high: prices.high,
        },
    );
    let drafts: Vec<DraftCell> = grouped
        .runs
        .iter()
        .filter_map(|run| {
            let (y0, y1) = bucket_rows(run.price_bucket, grouping, prices)?;
            let (x0, x1) = (fraction(run.start_ms), fraction(run.end_ms));
            (x1 > x0).then_some(DraftCell {
                generation: run.generation,
                side: run.side,
                price_bucket: run.price_bucket,
                quantity: run.quantity,
                x0,
                x1,
                y0,
                y1,
            })
        })
        .collect();
    heat.liquidity_reference =
        liquidity_reference(config, drafts.iter().map(|draft| draft.quantity));
    (heat.cells, _) = finish_cells(drafts, heat.liquidity_reference, config);
    if config.show_gaps {
        heat.gaps = book_gaps(history, from_ms, until_ms, |time_ms| {
            Some(fraction(time_ms))
        });
    }
    heat
}

/// A stretch of the tape, `[x0, x1]`, no book was read for yet.
fn pending_gap(x0: f64, x1: f64) -> GapPrimitive {
    GapPrimitive {
        from_generation: None,
        to_generation: None,
        x0,
        x1,
        reason: BOOK_PENDING.to_owned(),
    }
}
