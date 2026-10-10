//! The live book on the tape: read from the history on every frame, on that
//! frame's own clock, and carried from its last confirmation out to the
//! lane's live edge while the book is still confirming.
//!
//! It used to be part of the settled half, which is re-cut at most once per
//! projection interval: between two cuts the tape's bubbles rolled on the
//! frame's clock while its book stood still on the clock of the last cut, and
//! the newest confirmations waited up to an interval to be drawn. The tape's
//! window is short, so reading just that window each frame is cheap.

use rust_decimal::Decimal;

use super::heat_cells::{DraftCell, bucket_rows, finish_cells};
use super::model::{HeatmapCell, PriceWindow};
use crate::grouping::{EffectiveGrouping, GroupingWindow, sweep_grouped_runs};
use crate::history::LiquidityHistory;
use crate::timeline::BarTimeline;

/// The longest stretch the live book is carried past its last confirmation.
///
/// A connected book confirms on a fixed beat — every 50 ms from the MT5
/// bridge, every 100 ms on Binance's depth stream — and its stamps can trail
/// the tape clock by how late the newest print reached the bridge: the
/// trader's WIN frames measured 17-152 ms. Half a second is ten MT5 beats and
/// five Binance ones, over three times the worst lag measured: inside it the
/// book is still confirming and only its stamp trails, past it the book has
/// stopped confirming and the empty stretch is the honest picture.
pub const BOOK_CARRY_MAX_MS: i64 = 500;

/// Share of its observed alpha a carried band is drawn at: the same wall,
/// visibly fainter, so carried depth never passes for observed depth.
pub const CARRIED_BOOK_ALPHA: f32 = 0.45;

/// The stretch the live book is carried over: from its last confirmation to
/// the lane's live edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BookCarry {
    /// The last confirmation: observed depth ends here.
    pub from_ms: i64,
    /// The lane's live edge: carried depth ends here.
    pub until_ms: i64,
}

/// Whether the live book is carried to `live_edge_ms`, and over what.
///
/// Never across a gap, a disconnect or a resync (each of them closes the
/// generation, and with it every open level), never before a book exists,
/// and never past [`BOOK_CARRY_MAX_MS`] after the last confirmation: a book
/// that stopped confirming is not the book any more.
#[must_use]
pub fn book_carry(history: &LiquidityHistory, live_edge_ms: i64) -> Option<BookCarry> {
    if !history.is_synchronized() {
        return None;
    }
    let from_ms = history.latest_book_ms()?;
    let span_ms = live_edge_ms.checked_sub(from_ms)?;
    (span_ms > 0 && span_ms <= BOOK_CARRY_MAX_MS).then_some(BookCarry {
        from_ms,
        until_ms: live_edge_ms,
    })
}

/// The book's bands on the lane of `timeline`, coloured against `reference`
/// (the settled half's, so a wall reads the same on both panes). Returns the
/// bands and how many the cell cap dropped.
pub(super) fn project_lane_heat(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
    reference: Decimal,
) -> (Vec<HeatmapCell>, usize) {
    let config = history.config();
    let Some((lane_start_ms, lane_end_ms)) = timeline.lane_bounds_ms() else {
        return (Vec::new(), 0);
    };
    if !config.depth_visible_anywhere() {
        return (Vec::new(), 0);
    }
    let start_ms = history
        .retention_start_ms()
        .map_or(lane_start_ms, |retained| retained.max(lane_start_ms));
    let carry = book_carry(history, lane_end_ms);
    let open_run_end_ms = carry.map_or_else(
        || history.latest_book_ms().unwrap_or(lane_end_ms),
        |carry| carry.until_ms,
    );
    let grouped = sweep_grouped_runs(
        history.runs_intersecting(start_ms, lane_end_ms),
        history.coverage_segments(),
        grouping,
        GroupingWindow {
            start_ms,
            end_ms: lane_end_ms,
            open_run_end_ms,
            price_low: prices.low,
            price_high: prices.high,
        },
    );
    let x = |time_ms: i64| {
        timeline
            .locate_in_lane_clamped(time_ms)
            .map(|position| position.normalized)
    };
    let mut drafts = Vec::with_capacity(grouped.runs.len());
    for run in &grouped.runs {
        let Some((y0, y1)) = bucket_rows(run.price_bucket, grouping, prices) else {
            continue;
        };
        // Only an open level reaches past the last confirmation, and only
        // when the book is carried.
        let observed_end_ms = carry.map_or(run.end_ms, |carry| run.end_ms.min(carry.from_ms));
        for (from_ms, to_ms, carried) in [
            (run.start_ms, observed_end_ms, false),
            (observed_end_ms, run.end_ms, true),
        ] {
            if to_ms <= from_ms {
                continue;
            }
            if let (Some(x0), Some(x1)) = (x(from_ms), x(to_ms))
                && x1 > x0
            {
                drafts.push(DraftCell {
                    generation: run.generation,
                    side: run.side,
                    price_bucket: run.price_bucket,
                    quantity: run.quantity,
                    x0,
                    x1,
                    y0,
                    y1,
                    carried,
                });
            }
        }
    }
    finish_cells(drafts, reference, config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HeatmapConfig;
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

    fn history() -> LiquidityHistory {
        let mut history = LiquidityHistory::new(HeatmapConfig::default());
        history
            .install_snapshot(
                1_000,
                1,
                BookSnapshot::new(
                    10,
                    vec![BookLevel::new(Decimal::from(95), Decimal::from(5)).unwrap()],
                    Vec::new(),
                    BookCoverage::Limited {
                        levels_per_side: 10,
                    },
                ),
            )
            .unwrap();
        history
    }

    #[test]
    fn a_confirming_book_is_carried_to_the_edge() {
        let history = history();
        assert_eq!(
            book_carry(&history, 1_000 + BOOK_CARRY_MAX_MS),
            Some(BookCarry {
                from_ms: 1_000,
                until_ms: 1_000 + BOOK_CARRY_MAX_MS
            })
        );
    }

    #[test]
    fn nothing_is_carried_past_the_bound_or_onto_the_confirmation() {
        let history = history();
        assert_eq!(book_carry(&history, 1_001 + BOOK_CARRY_MAX_MS), None);
        assert_eq!(book_carry(&history, 1_000), None, "nothing to carry");
        assert_eq!(book_carry(&history, 900), None, "an edge behind the book");
    }

    #[test]
    fn nothing_is_carried_across_a_gap_or_before_a_book() {
        let mut history = history();
        history
            .apply_delta(1_050, &BookDelta::new(11, 11, Vec::new(), Vec::new()))
            .unwrap();
        history.mark_gap(1_100, "transport_closed").unwrap();
        assert_eq!(book_carry(&history, 1_150), None, "a gap ends the book");
        let empty = LiquidityHistory::new(HeatmapConfig::default());
        assert_eq!(book_carry(&empty, 1_150), None, "no book yet");
    }
}
