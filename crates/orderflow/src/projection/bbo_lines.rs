//! The best ask and the best bid as stepped lines on the tape, read from the
//! history's recorded top of the book ([`BboTrail`]).
//!
//! A line holds its price until the top moves, then steps straight up or
//! down to the new one. It breaks wherever there is no top to draw: a gap, a
//! new generation, an empty side. The stretch the live book is carried past
//! its last confirmation ([`BookCarry`]) is its own line, flagged
//! [`carried`](BboPath::carried) so the painter dims it as it dims the
//! carried bands.

use rust_decimal::Decimal;

use super::lane_heat::{CARRIED_BOOK_ALPHA, book_carry};
use super::model::PriceWindow;
use crate::history::{BboStep, BboTrail, LiquidityHistory, RestingSide};
use crate::timeline::BarTimeline;

/// A step vertex: `(time_ms, price)`.
type StepPoint = (i64, Decimal);

/// One unbroken stretch of one side's best price, in market time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BboLine {
    /// Bid for the best bid, ask for the best ask.
    pub side: RestingSide,
    /// Past the last confirmation, carried to the live edge.
    pub carried: bool,
    /// Step vertices: `(time_ms, price)`, a horizontal then a vertical leg
    /// per change of the price.
    pub points: Vec<(i64, Decimal)>,
}

/// A [`BboLine`] placed on the lane: normalized `[x, y]` vertices, `y`
/// outside `[0, 1]` where the price is off the window (the painter clips).
#[derive(Debug, Clone, PartialEq)]
pub struct BboPath {
    /// Bid for the best bid, ask for the best ask.
    pub side: RestingSide,
    /// Past the last confirmation, carried to the live edge.
    pub carried: bool,
    /// Normalized `[x, y]` vertices.
    pub points: Vec<[f64; 2]>,
}

impl BboPath {
    /// The share of its side's colour the path is drawn at: a carried line
    /// as faint as a carried band ([`CARRIED_BOOK_ALPHA`]).
    #[must_use]
    pub fn alpha(&self) -> f32 {
        if self.carried {
            CARRIED_BOOK_ALPHA
        } else {
            1.0
        }
    }
}

/// The stepped best-bid and best-ask lines over `[from_ms, observed_until_ms)`,
/// then, when `carried_until_ms` is past it, the top held at
/// `observed_until_ms` carried out to there. Bid lines first, then ask, each
/// in time order with its carried line last.
#[must_use]
pub fn bbo_lines(
    trail: &BboTrail,
    from_ms: i64,
    observed_until_ms: i64,
    carried_until_ms: Option<i64>,
) -> Vec<BboLine> {
    let mut lines = Vec::new();
    if observed_until_ms <= from_ms {
        return lines;
    }
    let steps: Vec<&BboStep> = trail.steps_between(from_ms, observed_until_ms).collect();
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
        let held = steps
            .last()
            .filter(|step| step.generation.is_some())
            .and_then(|step| price(step));
        if let (Some(until_ms), Some(value)) = (carried_until_ms, held)
            && until_ms > observed_until_ms
        {
            lines.push(BboLine {
                side,
                carried: true,
                points: vec![(observed_until_ms, value), (until_ms, value)],
            });
        }
    }
    lines.retain(|line| line.points.len() >= 2);
    lines
}

/// The lines of the tape on `timeline`, placed in `prices`: observed up to
/// the book's last confirmation, carried from there to the lane's live edge
/// while the book is current. Empty while
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
    .into_iter()
    .filter_map(|line| {
        let points = line
            .points
            .iter()
            .map(|&(time_ms, price)| Some([x(time_ms)?, prices.y_unclamped(price)?]))
            .collect::<Option<Vec<_>>>()?;
        Some(BboPath {
            side: line.side,
            carried: line.carried,
            points,
        })
    })
    .collect()
}

#[cfg(test)]
#[path = "bbo_lines_tests.rs"]
mod tests;
