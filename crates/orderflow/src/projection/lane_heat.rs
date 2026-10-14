//! The live book on the tape: read from the history on every frame, on that
//! frame's own clock, and carried from its last confirmation out to the
//! lane's live edge while the book is still confirming.
//!
//! It used to be part of the settled half, which is re-cut at most once per
//! projection interval: between two cuts the tape's bubbles rolled on the
//! frame's clock while its book stood still on the clock of the last cut, and
//! the newest confirmations waited up to an interval to be drawn. Native rows
//! reuse the captured RLE intervals directly, including in a five-minute lane;
//! only wider display rows require another quantity sweep. An averaged lane
//! is folded once per closed run instead ([`LaneFold`]), on bins that never
//! move with the clock.

use rust_decimal::Decimal;

use std::collections::BTreeMap;

use quantick_orderbook::OrderBook;

use super::heat_cells::{DraftCell, PriceRows, finish_with_priority};
use super::model::{BookSource, HeatmapCell, PriceWindow};
use crate::grouping::{EffectiveGrouping, GroupingWindow, bucket_for_price, sweep_grouped_runs};
use crate::history::{HistoryStatus, LiquidityHistory, RestingSide};
use crate::timeline::BarTimeline;

mod lane_fold;
mod native_runs;

use lane_fold::DepthRun;
pub use lane_fold::LaneFold;

#[cfg(test)]
#[path = "lane_heat/tests.rs"]
mod cap_tests;

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

/// Whether a book confirmed at `confirmed_ms` is still the current book at
/// `now_ms`: it confirmed no more than [`BOOK_CARRY_MAX_MS`] before, or at
/// or after `now_ms`. A stamp ahead of a trade-anchored clock is the newest
/// book there is, never an expired one; past the bound the book stopped
/// confirming.
///
/// The one staleness rule: the worker reads current rows only from a book it
/// holds, the pane keeps previewing them only while it holds, and a current
/// row is never drawn carried — so its alpha cannot flip from one
/// publication of a live book to the next.
#[must_use]
pub const fn book_is_current(now_ms: i64, confirmed_ms: i64) -> bool {
    now_ms.saturating_sub(confirmed_ms) <= BOOK_CARRY_MAX_MS
}

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
    (span_ms > 0 && book_is_current(live_edge_ms, from_ms)).then_some(BookCarry {
        from_ms,
        until_ms: live_edge_ms,
    })
}

/// The synchronized book's generation and source, while it is still
/// current at the lane's live edge ([`book_is_current`]).
fn current_source(history: &LiquidityHistory, live_edge_ms: i64) -> Option<(u64, BookSource)> {
    let HistoryStatus::Synced {
        generation,
        last_event_ms,
        ..
    } = history.status()
    else {
        return None;
    };
    book_is_current(live_edge_ms, last_event_ms).then(|| {
        (
            generation,
            BookSource {
                update_id: history.book().last_update_id(),
                event_ms: last_event_ms,
            },
        )
    })
}

/// The book's quantity in every row the window shows any of, summed over
/// whole buckets as the grouped runs are: a row the window's edge cuts
/// still reads every level it holds.
fn current_quantities(
    book: &OrderBook,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
) -> BTreeMap<(RestingSide, Decimal), Decimal> {
    let low = bucket_for_price(prices.low, grouping);
    let high = bucket_for_price(prices.high, grouping) + grouping.bucket_width;
    let mut quantities = BTreeMap::new();
    for (side, levels) in [
        (RestingSide::Bid, book.bids()),
        (RestingSide::Ask, book.asks()),
    ] {
        for (&price, &quantity) in levels.range(low..high) {
            *quantities
                .entry((side, bucket_for_price(price, grouping)))
                .or_insert(Decimal::ZERO) += quantity;
        }
    }
    quantities
}

/// The book's bands on the lane of `timeline`, coloured against `reference`
/// (the settled half's, so a wall reads the same on both panes). Returns the
/// bands, how many the cell cap dropped, and the one book the current rows
/// were read from.
#[cfg(test)]
pub(super) fn project_lane_heat(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
    reference: Decimal,
) -> (Vec<HeatmapCell>, usize, Option<BookSource>) {
    let mut fold = LaneFold::default();
    project_lane_heat_folded(history, timeline, prices, grouping, reference, &mut fold)
}

/// [`project_lane_heat`] on a lane `fold` kept between requests: an averaged
/// lane folds only the runs closed since the last one.
pub(super) fn project_lane_heat_folded(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    grouping: EffectiveGrouping,
    reference: Decimal,
    fold: &mut LaneFold,
) -> (Vec<HeatmapCell>, usize, Option<BookSource>) {
    let config = history.config();
    let Some((lane_start_ms, lane_end_ms)) = timeline.lane_bounds_ms() else {
        return (Vec::new(), 0, None);
    };
    if !config.depth_visible_anywhere() {
        return (Vec::new(), 0, None);
    }
    let averaging = config.live_lane.depth_time_averaging;
    let start_ms = history
        .retention_start_ms()
        .map_or(lane_start_ms, |retained| retained.max(lane_start_ms));
    let carry = book_carry(history, lane_end_ms);
    let open_run_end_ms = carry.map_or_else(
        || history.latest_book_ms().unwrap_or(lane_end_ms),
        |carry| carry.until_ms,
    );
    let window = GroupingWindow {
        start_ms,
        end_ms: lane_end_ms,
        open_run_end_ms,
        price_low: prices.low,
        price_high: prices.high,
    };
    let runs = if averaging {
        fold.project(
            history,
            grouping,
            window,
            history
                .latest_book_ms()
                .unwrap_or(lane_end_ms)
                .min(lane_end_ms),
            config.max_visible_cells,
        )
    } else {
        fold.clear();
        let runs = if grouping.multiple == 1 && grouping.bucket_width == config.price_grouping {
            native_runs::project(history, window)
        } else {
            sweep_grouped_runs(
                history.runs_intersecting(start_ms, lane_end_ms),
                history.coverage_segments(),
                grouping,
                window,
            )
            .runs
        };
        runs.into_iter()
            .map(|run| DepthRun {
                run,
                time_averaged: false,
            })
            .collect()
    };
    let x = |time_ms: i64| {
        timeline
            .locate_in_lane_clamped(time_ms)
            .map(|position| position.normalized)
    };
    let mut drafts = Vec::with_capacity(runs.len());
    let mut rows = PriceRows::new(grouping, prices);
    for depth in &runs {
        let run = &depth.run;
        let Some((y0, y1)) = rows.get(run.price_bucket) else {
            continue;
        };
        // Only an open level reaches past the last confirmation, and only
        // when the book is carried.
        let observed_end_ms = carry.map_or(run.end_ms, |carry| run.end_ms.min(carry.from_ms));
        for (from_ms, to_ms, carried) in [
            (run.start_ms, observed_end_ms, false),
            (observed_end_ms, run.end_ms, true),
        ] {
            if to_ms <= from_ms || (carried && averaging) {
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
                    time_averaged: depth.time_averaged,
                    current_book: false,
                });
            }
        }
    }
    // The RLE history deliberately tolerates small quantity churn. Current
    // rows instead read the authoritative synchronized book on this worker,
    // including a publication exactly at NOW with no invented time span.
    // With averaging they fill the lane from the confirmation to the live
    // edge, where the averaged runs stop; without it the lane's own runs
    // carry the book there, so the rows claim no lane time and stand at the
    // edge for the forward preview alone, and only a lane that asks for that
    // preview reads them. They are never carried.
    let source = (averaging || config.live_lane.forward_book_preview)
        .then(|| current_source(history, lane_end_ms))
        .flatten();
    if let Some((generation, source)) = source
        && let Some(x1) = x(lane_end_ms)
        && let Some(x0) = if averaging {
            x(source.event_ms)
        } else {
            Some(x1)
        }
    {
        for ((side, price_bucket), quantity) in current_quantities(history.book(), prices, grouping)
        {
            if let Some((y0, y1)) = rows.get(price_bucket) {
                drafts.push(DraftCell {
                    generation,
                    side,
                    price_bucket,
                    quantity,
                    x0,
                    x1,
                    y0,
                    y1,
                    carried: false,
                    time_averaged: false,
                    current_book: true,
                });
            }
        }
    }
    let (cells, dropped) = finish_with_priority(drafts, reference, config, averaging);
    (cells, dropped, source.map(|(_, source)| source))
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
