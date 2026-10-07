//! The book beside a tape held at a past instant.
//!
//! The live frame's bands are today's book on today's clock, so a past tape
//! cannot borrow them. It reads the runs the history kept for its own blocks
//! instead, on the same grouping and intensity scale as the live frame, and
//! places them on the tape's clock when it is drawn: a drag moves the bands
//! with the prints and re-reads nothing.

use std::sync::Arc;

use rust_decimal::Decimal;

use super::model::{BEFORE_CAPTURE, GapPrimitive, HeatmapCell, HeatmapProjection, PriceWindow};
use super::{SettledProjection, normalized_log_intensity};
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
    /// The intensity reference the bands were coloured against: the live
    /// frame's, so a wall reads the same on either side of a pan.
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
        settled: &SettledProjection,
    ) -> Self {
        Self {
            from_ms,
            until_ms,
            prices,
            liquidity_reference: settled.liquidity_reference,
            effective_grouping: settled.effective_grouping,
            cells: Vec::new(),
            gaps: Vec::new(),
        }
    }

    /// Whether these bands still answer for `prices` and `settled`'s scale.
    #[must_use]
    pub fn fits(&self, prices: PriceWindow, settled: &SettledProjection) -> bool {
        self.prices == prices
            && self.liquidity_reference == settled.liquidity_reference
            && self.effective_grouping == settled.effective_grouping
    }

    /// The bands on a frame of `slot_count` regions whose last region is the
    /// tape, showing the `window_ms` of market time that ends at `end_ms`:
    /// clipped to that window, and to nothing else.
    #[must_use]
    pub fn placed(&self, end_ms: i64, window_ms: i64, slot_count: usize) -> HeatmapProjection {
        let mut projection = HeatmapProjection::empty(true, self.effective_grouping);
        projection.liquidity_reference = self.liquidity_reference;
        let Some(place) = Placement::new(self, end_ms, window_ms, slot_count) else {
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
        projection.gaps = Arc::new(
            self.gaps
                .iter()
                .filter_map(|gap| {
                    let (x0, x1) = place.span(gap.x0, gap.x1)?;
                    Some(GapPrimitive {
                        x0,
                        x1,
                        ..gap.clone()
                    })
                })
                .collect(),
        );
        projection
    }
}

/// The map from a fraction of the blocks to a position on the frame's tape.
struct Placement {
    from_ms: f64,
    span_ms: f64,
    window_start_ms: f64,
    window_ms: f64,
    lane_x0: f64,
    lane_width: f64,
}

impl Placement {
    fn new(heat: &PastHeat, end_ms: i64, window_ms: i64, slot_count: usize) -> Option<Self> {
        let (span_ms, window_ms) = (heat.until_ms - heat.from_ms, window_ms.max(1));
        (slot_count >= 1 && span_ms > 0).then(|| {
            let regions = slot_count as f64;
            Self {
                from_ms: heat.from_ms as f64,
                span_ms: span_ms as f64,
                window_start_ms: end_ms.saturating_sub(window_ms) as f64,
                window_ms: window_ms as f64,
                lane_x0: (regions - 1.0) / regions,
                lane_width: 1.0 / regions,
            }
        })
    }

    /// One `[x0, x1]` span of the blocks on the tape; `None` off the window.
    fn span(&self, x0: f64, x1: f64) -> Option<(f64, f64)> {
        let on_tape = |x: f64| {
            let time = self.from_ms + x * self.span_ms;
            let unit = ((time - self.window_start_ms) / self.window_ms).clamp(0.0, 1.0);
            self.lane_x0 + unit * self.lane_width
        };
        let (left, right) = (on_tape(x0), on_tape(x1));
        (right > left).then_some((left, right))
    }
}

/// Read the book the history kept over `[from_ms, until_ms)`, the way the
/// live frame reads it: the same runs, grouping, clip and intensity scale.
#[must_use]
pub fn project_past_heat(
    history: &LiquidityHistory,
    from_ms: i64,
    until_ms: i64,
    prices: PriceWindow,
    settled: &SettledProjection,
) -> PastHeat {
    let mut heat = PastHeat::empty(from_ms, until_ms, prices, settled);
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
    let grouping = settled.effective_grouping;
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
    if config.show_liquidity {
        for run in &grouped.runs {
            let low = run.price_bucket.max(prices.low);
            let high = (run.price_bucket + grouping.bucket_width).min(prices.high);
            let (Some(y0), Some(y1)) = (prices.y(high), prices.y(low)) else {
                continue;
            };
            let (x0, x1) = (fraction(run.start_ms), fraction(run.end_ms));
            if y1 <= y0 || x1 <= x0 {
                continue;
            }
            let intensity =
                normalized_log_intensity(run.quantity, settled.liquidity_reference, config.gamma);
            heat.cells.push(HeatmapCell {
                generation: run.generation,
                side: run.side,
                price_bucket: run.price_bucket,
                quantity: run.quantity,
                x0,
                x1,
                y0,
                y1,
                intensity,
                alpha: intensity * config.opacity,
            });
        }
        // The same deterministic cap the live frame applies: strongest first.
        if heat.cells.len() > config.max_visible_cells {
            heat.cells.sort_by(|a, b| {
                b.quantity
                    .cmp(&a.quantity)
                    .then_with(|| a.generation.cmp(&b.generation))
                    .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                    .then_with(|| a.x0.total_cmp(&b.x0))
            });
            heat.cells.truncate(config.max_visible_cells);
        }
    }
    if config.show_gaps {
        heat.gaps = past_gaps(history, from_ms, until_ms, &fraction);
    }
    heat
}

/// Where the book is missing over the blocks: the coverage gaps inside them,
/// and the stretch before the oldest book the history still holds.
fn past_gaps(
    history: &LiquidityHistory,
    from_ms: i64,
    until_ms: i64,
    fraction: &dyn Fn(i64) -> f64,
) -> Vec<GapPrimitive> {
    let mut gaps: Vec<GapPrimitive> = history
        .coverage_gaps()
        .filter(|gap| gap.start_ms < until_ms && gap.end_ms.is_none_or(|end| end > from_ms))
        .map(|gap| GapPrimitive {
            from_generation: gap.from_generation,
            to_generation: gap.to_generation,
            x0: fraction(gap.start_ms),
            x1: fraction(gap.end_ms.unwrap_or(until_ms)),
            reason: gap.reason.clone(),
        })
        .collect();
    let first = history.coverage_segments().next();
    let recorded_from = first.map_or(until_ms, |segment| segment.start_ms);
    if recorded_from > from_ms {
        gaps.push(GapPrimitive {
            from_generation: None,
            to_generation: first.map(|segment| segment.generation),
            x0: 0.0,
            x1: fraction(recorded_from),
            reason: BEFORE_CAPTURE.to_owned(),
        });
    }
    gaps.retain(|gap| gap.x1 > gap.x0);
    gaps.sort_by(|a, b| a.x0.total_cmp(&b.x0).then_with(|| a.x1.total_cmp(&b.x1)));
    gaps
}
