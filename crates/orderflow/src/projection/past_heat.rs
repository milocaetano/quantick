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
use super::model::{
    BOOK_BEYOND_LEAD, BOOK_PENDING, GapPrimitive, HeatmapCell, HeatmapProjection, PriceWindow,
};
use super::tape_geometry::MAX_LEAD_SHARE;
use crate::grouping::{EffectiveGrouping, GroupingWindow, sweep_grouped_runs};
use crate::history::LiquidityHistory;

/// The recorded book over a past tape's blocks. Horizontal positions are
/// fractions of `[from_ms, until_ms]`, not chart positions: see
/// [`placed`](Self::placed).
#[derive(Debug, Clone, PartialEq)]
pub struct PastHeat {
    /// Half-open stretch of market time the bands were cut from: the tape's
    /// blocks and [`lead_ms`](Self::lead_ms) beyond each side of them.
    pub from_ms: i64,
    pub until_ms: i64,
    /// Market time read beyond each side of the tape's window, for the
    /// insets the native tape pads its dots with
    /// ([`TapeHorizontalGeometry::lead_share`](super::TapeHorizontalGeometry::lead_share));
    /// zero for a tape whose book does not share the dots' clock.
    pub lead_ms: i64,
    /// Where the recorded book ended when the bands were cut: past it the
    /// stretch is [`BOOK_PENDING`] until a re-read.
    pub recorded_until_ms: i64,
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
            lead_ms: 0,
            recorded_until_ms: until_ms,
            prices,
            liquidity_reference: Decimal::ZERO,
            effective_grouping,
            cells: Vec::new(),
            gaps: Vec::new(),
        }
    }

    /// Whether these bands still answer for `prices`, `grouping` and a lead
    /// of `lead_ms` over `history`: their inputs besides the frozen history,
    /// and a book recorded since a read that ended before its stretch did.
    #[must_use]
    pub fn fits(
        &self,
        history: &LiquidityHistory,
        prices: PriceWindow,
        grouping: EffectiveGrouping,
        lead_ms: i64,
    ) -> bool {
        let recorded = self.recorded_until_ms >= self.until_ms
            || history
                .latest_book_ms()
                .is_none_or(|latest| latest <= self.recorded_until_ms);
        self.prices == prices
            && self.effective_grouping == grouping
            && self.lead_ms == lead_ms
            && recorded
    }

    /// The bands on a frame of `slot_count` regions whose last region is the
    /// tape, showing the `window_ms` of market time that ends at `end_ms`:
    /// clipped to that window, and to nothing else. A stretch of the window
    /// these blocks do not reach — a drag outran them — is a
    /// [`BOOK_PENDING`] gap, never an empty book.
    ///
    /// Every `x` is at or right of the lane's opening: the caller places them
    /// on the tape alone, the divider included. What the bands hold beyond
    /// the window, up to [`lead_ms`](Self::lead_ms) a side, is placed on the
    /// dots' clock in the tape's insets ([`HeatmapCell::lead_span`]); a lead
    /// cut at its cap leaves the inset farther out a [`BOOK_BEYOND_LEAD`] gap.
    #[must_use]
    pub fn placed(&self, end_ms: i64, window_ms: i64, slot_count: usize) -> HeatmapProjection {
        let mut projection = HeatmapProjection::empty(true, self.effective_grouping);
        projection.liquidity_reference = self.liquidity_reference;
        let Some(place) = Placement::new(
            (self.from_ms, self.until_ms),
            (end_ms, window_ms),
            self.lead_ms,
            slot_count,
        ) else {
            return projection;
        };
        projection.cells = Arc::new(
            self.cells
                .iter()
                .flat_map(|cell| {
                    place.pieces(place.unit(cell.x0), place.unit(cell.x1)).map(
                        move |(x0, x1, lead_span)| HeatmapCell {
                            x0,
                            x1,
                            lead_span,
                            ..cell.clone()
                        },
                    )
                })
                .collect(),
        );
        let mut gaps: Vec<GapPrimitive> = self
            .gaps
            .iter()
            .flat_map(|gap| {
                place.pieces(place.unit(gap.x0), place.unit(gap.x1)).map(
                    move |(x0, x1, lead_span)| GapPrimitive {
                        x0,
                        x1,
                        lead_span,
                        ..gap.clone()
                    },
                )
            })
            .collect();
        let (need_from, need_until) = (
            place.time_unit(self.from_ms.min(end_ms) as f64),
            // Past the blocks read, and in the trail past the newest
            // recorded book; inside the window a held tape draws what was
            // recorded, as it always has.
            place.time_unit(self.until_ms.max(self.from_ms) as f64).min(
                place
                    .time_unit(self.recorded_until_ms.max(self.from_ms) as f64)
                    .max(1.0),
            ),
        );
        for (from, to, reason) in [
            (f64::NEG_INFINITY, need_from, BOOK_PENDING),
            (need_until, f64::INFINITY, BOOK_PENDING),
        ] {
            gaps.extend(
                place
                    .pieces(from, to)
                    .map(|(x0, x1, lead_span)| GapPrimitive {
                        lead_span,
                        ..reason_gap(x0, x1, reason)
                    }),
            );
        }
        if place.lead_capped() {
            for (from, to) in [
                (f64::NEG_INFINITY, -place.lead),
                (1.0 + place.lead, f64::INFINITY),
            ] {
                gaps.push(GapPrimitive {
                    lead_span: Some((from, to)),
                    ..reason_gap(place.lane_x0, place.lane_x0, BOOK_BEYOND_LEAD)
                });
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
                if let Some(place) =
                    Placement::new((end_ms, end_ms), (end_ms, window_ms), 0, slot_count)
                {
                    let lane = (place.lane_x0, place.lane_x0 + place.lane_width);
                    unread.gaps = Arc::new(vec![reason_gap(lane.0, lane.1, BOOK_PENDING)]);
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
    /// [`PastHeat::lead_ms`] in windows: how far past each side of the
    /// window a piece is still placed, in the tape's insets.
    lead: f64,
    lane_x0: f64,
    lane_width: f64,
}

/// One placed piece: frame `x0`, `x1`, and its span on the dots' clock when
/// it lies in an inset ([`HeatmapCell::lead_span`]).
type Piece = (f64, f64, Option<(f64, f64)>);

impl Placement {
    /// The blocks `(from_ms, until_ms)` on a frame of `slot_count` regions
    /// whose tape shows the `window_ms` ending at `end_ms`, read `lead_ms`
    /// beyond each side of it.
    fn new(
        (from_ms, until_ms): (i64, i64),
        (end_ms, window_ms): (i64, i64),
        lead_ms: i64,
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
                lead: lead_ms.max(0) as f64 / window_ms as f64,
                lane_x0: (regions - 1.0) / regions,
                lane_width: 1.0 / regions,
            }
        })
    }

    /// Where `time_ms` sits on the window, unclamped: zero at its start, one
    /// at its end.
    fn time_unit(&self, time_ms: f64) -> f64 {
        (time_ms - self.window_start_ms) / self.window_ms
    }

    /// [`Self::time_unit`] of a fraction of the blocks.
    fn unit(&self, x: f64) -> f64 {
        self.time_unit(self.from_ms + x * self.span_ms)
    }

    /// Whether the lead stopped at its cap, short of the inset it pads.
    fn lead_capped(&self) -> bool {
        self.lead > 0.0 && self.lead >= MAX_LEAD_SHARE - 1.0 / self.window_ms
    }

    /// The window units `[u0, u1]` cut into what the tape draws: the lead
    /// before the window, the window itself, and the trail after it, each
    /// only where it is not empty.
    fn pieces(&self, u0: f64, u1: f64) -> impl Iterator<Item = Piece> + '_ {
        let lane_end = self.lane_x0 + self.lane_width;
        [
            (-self.lead, 0.0, Some(self.lane_x0)),
            (0.0, 1.0, None),
            (1.0, 1.0 + self.lead, Some(lane_end)),
        ]
        .into_iter()
        .filter_map(move |(low, high, inset)| {
            let (from, to) = (u0.max(low), u1.min(high));
            (to > from).then_some(match inset {
                Some(x) => (x, x, Some((from, to))),
                None => (
                    self.lane_x0 + from * self.lane_width,
                    self.lane_x0 + to * self.lane_width,
                    None,
                ),
            })
        })
    }
}

/// Read the book the history kept over `[from_ms, until_ms)`, and `lead_ms`
/// beyond each side of it for the tape's insets ([`PastHeat::lead_ms`]),
/// the way the live frame reads its own ([`super::heat_cells`]), on
/// `grouping`, and coloured against these runs.
#[must_use]
pub fn project_past_heat(
    history: &LiquidityHistory,
    (from_ms, until_ms): (i64, i64),
    lead_ms: i64,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
) -> PastHeat {
    let lead_ms = lead_ms.max(0);
    let (from_ms, until_ms) = (
        from_ms.saturating_sub(lead_ms),
        until_ms.saturating_add(lead_ms),
    );
    let mut heat = PastHeat::empty(from_ms, until_ms, prices, grouping);
    heat.lead_ms = lead_ms;
    let config = history.config();
    let span_ms = until_ms - from_ms;
    if !config.lane_depth_drawn() || span_ms <= 0 {
        return heat;
    }
    // Past the newest recorded book nothing was read yet: a held tape near
    // NOW reads its trail again as the book arrives ([`PastHeat::fits`]).
    heat.recorded_until_ms = history
        .latest_book_ms()
        .map_or(until_ms, |latest| latest.clamp(from_ms, until_ms));
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
                carried: false,
                time_averaged: false,
                current_book: false,
                lead_span: None,
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

/// A stretch of the tape, `[x0, x1]`, with no book read for `reason`.
fn reason_gap(x0: f64, x1: f64, reason: &str) -> GapPrimitive {
    GapPrimitive {
        from_generation: None,
        to_generation: None,
        x0,
        x1,
        reason: reason.to_owned(),
        lead_span: None,
    }
}
