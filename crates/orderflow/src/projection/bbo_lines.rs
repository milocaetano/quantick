//! The best ask and the best bid as stepped lines on the tape, read from the
//! history's recorded top of the book ([`BboTrail`]).
//!
//! A line holds its price until the top moves, then steps straight up or
//! down to the new one. It breaks wherever there is no top to draw: a gap, a
//! new generation, an empty side. The stretch the live book is carried past
//! its last confirmation ([`BookCarry`](super::BookCarry)) is its own line,
//! flagged [`carried`](BboLine::carried) so the painter dims it as it dims
//! the carried bands.

use rust_decimal::Decimal;

use super::lane_heat::{CARRIED_BOOK_ALPHA, book_carry};
use super::model::PriceWindow;
use crate::history::{BboStep, BboTrail, LiquidityHistory, RestingSide};
use crate::timeline::BarTimeline;

/// A step vertex in market terms: `(time_ms, price)`.
pub type StepPoint = (i64, Decimal);

/// One unbroken stretch of one side's best price, its vertices `P`: a
/// horizontal then a vertical leg per change of the price.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BboLine<P> {
    /// Bid for the best bid, ask for the best ask.
    pub side: RestingSide,
    /// Past the last confirmation, carried to the live edge.
    pub carried: bool,
    /// The vertices, oldest first.
    pub points: Vec<P>,
}

/// A [`BboLine`] placed on the lane: normalized `[x, y]` vertices, `y`
/// outside `[0, 1]` where the price is off the window (the painter clips).
pub type BboPath = BboLine<[f64; 2]>;

impl<P> BboLine<P> {
    /// The share of its side's colour the line is drawn at: a carried line
    /// as faint as a carried band ([`CARRIED_BOOK_ALPHA`]).
    #[must_use]
    pub fn alpha(&self) -> f32 {
        if self.carried {
            CARRIED_BOOK_ALPHA
        } else {
            1.0
        }
    }

    /// The same line on other vertices; `None` when one does not map.
    pub fn map<Q>(&self, point: impl FnMut(&P) -> Option<Q>) -> Option<BboLine<Q>> {
        Some(BboLine {
            side: self.side,
            carried: self.carried,
            points: self.points.iter().map(point).collect::<Option<_>>()?,
        })
    }
}

impl BboLine<StepPoint> {
    /// The line cut to `[from_ms, until_ms]`: a horizontal leg crossing an
    /// edge ends on it; `None` when nothing of it is left to draw.
    #[must_use]
    pub fn clipped(&self, from_ms: i64, until_ms: i64) -> Option<Self> {
        let mut points: Vec<StepPoint> = Vec::new();
        for leg in self.points.windows(2) {
            let ((t0, p0), (t1, p1)) = (leg[0], leg[1]);
            if t1 < from_ms || t0 > until_ms {
                continue;
            }
            for point in [(t0.max(from_ms), p0), (t1.min(until_ms), p1)] {
                if points.last() != Some(&point) {
                    points.push(point);
                }
            }
        }
        spans_time(&points).then_some(Self {
            side: self.side,
            carried: self.carried,
            points,
        })
    }
}

/// Whether step vertices cover any time at all.
fn spans_time(points: &[StepPoint]) -> bool {
    matches!((points.first(), points.last()), (Some(first), Some(last)) if first.0 < last.0)
}

/// The stepped best-bid and best-ask lines over `[from_ms, observed_until_ms)`,
/// then, when `carried_until_ms` is past it, the top in force at
/// `observed_until_ms` — a step starting exactly there included — carried
/// out to there. Bid lines first, then ask, each in time order with its
/// carried line last.
#[must_use]
pub fn bbo_lines(
    trail: &BboTrail,
    from_ms: i64,
    observed_until_ms: i64,
    carried_until_ms: Option<i64>,
) -> Vec<BboLine<StepPoint>> {
    let mut lines = Vec::new();
    if observed_until_ms < from_ms {
        return lines;
    }
    let steps: Vec<&BboStep> = trail.steps_between(from_ms, observed_until_ms).collect();
    let held = trail
        .step_at(observed_until_ms)
        .filter(|step| step.generation.is_some());
    for side in [RestingSide::Bid, RestingSide::Ask] {
        let price = |step: &BboStep| match side {
            RestingSide::Bid => step.bid,
            RestingSide::Ask => step.ask,
        };
        let mut open: Option<(Option<u64>, Vec<StepPoint>)> = None;
        for (index, step) in steps.iter().enumerate() {
            let start_ms = step.start_ms.max(from_ms);
            let end_ms = steps.get(index + 1).map_or(observed_until_ms, |next| {
                next.start_ms.min(observed_until_ms)
            });
            let value = price(step).filter(|_| step.generation.is_some());
            let joins = matches!(&open, Some((generation, _)) if *generation == step.generation);
            if !joins || value.is_none() {
                lines.extend(open.take().map(|(_, points)| BboLine {
                    side,
                    carried: false,
                    points,
                }));
            }
            let Some(value) = value else {
                continue;
            };
            let (_, points) = open.get_or_insert_with(|| (step.generation, Vec::new()));
            match points.last_mut() {
                // The other side moved: this one's horizontal leg runs on.
                Some(last) if last.1 == value => last.0 = end_ms,
                _ => points.extend([(start_ms, value), (end_ms, value)]),
            }
        }
        lines.extend(open.take().map(|(_, points)| BboLine {
            side,
            carried: false,
            points,
        }));
        if let (Some(until_ms), Some(value)) = (carried_until_ms, held.and_then(price))
            && until_ms > observed_until_ms
        {
            lines.push(BboLine {
                side,
                carried: true,
                points: vec![(observed_until_ms, value), (until_ms, value)],
            });
        }
    }
    lines.retain(|line| spans_time(&line.points));
    lines
}

/// The lines of the live tape on `timeline`, placed in `prices`: observed up
/// to the book's last confirmation, carried from there to the lane's live
/// edge while the book is current. Empty while
/// [`LiveLaneStyle::bbo_lines`](crate::config::LiveLaneStyle::bbo_lines) is
/// off, or with no lane.
#[must_use]
pub fn project_bbo_paths(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
) -> Vec<BboPath> {
    if !history.config().live_lane.bbo_lines {
        return Vec::new();
    }
    let (Some((lane_start_ms, lane_end_ms)), Some(latest_ms)) =
        (timeline.lane_bounds_ms(), history.latest_book_ms())
    else {
        return Vec::new();
    };
    let from_ms = history
        .retention_start_ms()
        .map_or(lane_start_ms, |retained| retained.max(lane_start_ms));
    let carry = book_carry(history, lane_end_ms);
    let observed_until_ms = carry.map_or(latest_ms.min(lane_end_ms), |carry| carry.from_ms);
    let x = |time_ms: i64| {
        timeline
            .locate_in_lane_clamped(time_ms)
            .map(|position| position.normalized)
    };
    bbo_lines(
        history.bbo(),
        from_ms,
        observed_until_ms,
        carry.map(|carry| carry.until_ms),
    )
    .iter()
    .filter_map(|line| {
        line.map(|&(time_ms, price)| Some([x(time_ms)?, prices.y_unclamped(price)?]))
    })
    .collect()
}

/// The lines over `[from_ms, until_ms)` a held tape draws: observed only,
/// since a past book is never carried. Empty while the switch is off.
#[must_use]
pub fn past_bbo_lines(
    history: &LiquidityHistory,
    from_ms: i64,
    until_ms: i64,
) -> Vec<BboLine<StepPoint>> {
    let latest_ms = history.latest_book_ms().unwrap_or(until_ms);
    if !history.config().live_lane.bbo_lines {
        return Vec::new();
    }
    let start_ms = history
        .retention_start_ms()
        .map_or(from_ms, |retained| retained.max(from_ms));
    bbo_lines(history.bbo(), start_ms, until_ms.min(latest_ms), None)
}

/// How far, in screen pixels, a side's stroke centre sits from its true
/// price: outward, away from the spread, by half the stroke. The ask's
/// stroke then lies wholly on the far side of the ask and the bid's wholly on
/// the far side of the bid, so the two strokes meet at most edge to edge —
/// adjacent when the spread is under a pixel or zero, never overlapping, never
/// crossing a true price inward — and a wide spread only gains the half
/// stroke. `high_price_up` is the axis's orientation: the ask is above the
/// bid unless the chart is inverted.
#[must_use]
pub fn bbo_outward_offset_px(side: RestingSide, stroke_px: f32, high_price_up: bool) -> f32 {
    let away_is_up = (side == RestingSide::Ask) == high_price_up;
    if away_is_up {
        -stroke_px / 2.0
    } else {
        stroke_px / 2.0
    }
}

/// One stroke the painter lays, in paint order: a halo (`side` `None`, the
/// theme's shadow colour) or a side's line (its side colour).
#[derive(Debug, Clone, PartialEq)]
pub struct BboStroke {
    /// Whose colour: `None` is the halo.
    pub side: Option<RestingSide>,
    /// Stroke width, pixels.
    pub width: f32,
    /// Share of the colour: dimmed on the carried stretch only.
    pub alpha: f32,
    /// Screen vertices, decimated to the pixel column.
    pub points: Vec<[f32; 2]>,
}

/// The strokes of `paths` on screen: each placed by `place`, moved outward
/// off the spread ([`bbo_outward_offset_px`]) and decimated
/// ([`decimate_columns`]); every halo first, so no halo covers the other
/// side's colour, then every line at
/// [`BBO_LINE_WIDTH_PX`](crate::config::painting::BBO_LINE_WIDTH_PX).
#[must_use]
pub fn bbo_strokes(
    paths: &[BboPath],
    place: impl Fn([f64; 2]) -> [f32; 2],
    high_price_up: bool,
) -> Vec<BboStroke> {
    use crate::config::painting::{BBO_HALO_PX, BBO_LINE_WIDTH_PX};
    let lines: Vec<(&BboPath, Vec<[f32; 2]>)> = paths
        .iter()
        .map(|path| {
            let lift = bbo_outward_offset_px(path.side, BBO_LINE_WIDTH_PX, high_price_up);
            let at = |&point: &[f64; 2]| {
                let [x, y] = place(point);
                [x, y + lift]
            };
            (path, decimate_columns(path.points.iter().map(at)))
        })
        .collect();
    let stroke = |(path, points): &(&BboPath, Vec<[f32; 2]>), halo: bool| BboStroke {
        side: (!halo).then_some(path.side),
        width: BBO_LINE_WIDTH_PX + if halo { 2.0 * BBO_HALO_PX } else { 0.0 },
        alpha: path.alpha(),
        points: points.clone(),
    };
    let halos = lines.iter().map(|line| stroke(line, true));
    halos
        .chain(lines.iter().map(|line| stroke(line, false)))
        .collect()
}

/// `points` — screen `[x, y]`, `x` non-decreasing — cut to what one pixel
/// column can show: a column holding more than four vertices keeps its
/// first, its lowest and highest `y` (in the order they came) and its last,
/// so the step shape survives and the vertex count is bounded by the width.
#[must_use]
pub fn decimate_columns(points: impl IntoIterator<Item = [f32; 2]>) -> Vec<[f32; 2]> {
    let mut kept = Vec::new();
    let mut column: Vec<[f32; 2]> = Vec::new();
    for point in points {
        if column
            .first()
            .is_some_and(|first| first[0].floor() != point[0].floor())
        {
            keep_column(&mut column, &mut kept);
        }
        column.push(point);
    }
    keep_column(&mut column, &mut kept);
    kept.dedup();
    kept
}

/// Move one pixel column's vertices into `kept`, at most four of them.
fn keep_column(column: &mut Vec<[f32; 2]>, kept: &mut Vec<[f32; 2]>) {
    if column.len() <= 4 {
        kept.append(column);
        return;
    }
    let by_y = |a: &(usize, &[f32; 2]), b: &(usize, &[f32; 2])| a.1[1].total_cmp(&b.1[1]);
    let low = column.iter().enumerate().min_by(by_y).map(|(at, _)| at);
    let high = column.iter().enumerate().max_by(by_y).map(|(at, _)| at);
    let mut keep = vec![0, column.len() - 1];
    keep.extend(low.into_iter().chain(high));
    keep.sort_unstable();
    keep.dedup();
    kept.extend(keep.into_iter().map(|at| column[at]));
    column.clear();
}

#[cfg(test)]
#[path = "bbo_lines_tests.rs"]
mod tests;
