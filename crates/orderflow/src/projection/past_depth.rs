//! The book under a tape held in the past: the depth the retained history
//! recorded for the held window, as tape cells on the live frame's intensity
//! scale, and every stretch of it the history holds no book for, labelled.
//!
//! Nothing here is today's book. A level is drawn only for the time a run of
//! the history says it rested, and ends where the book was last known; a
//! stretch before the retained capture is a gap, never filled.

use std::sync::Arc;

use super::{
    BEFORE_CAPTURE, BOOK_EVICTED, GapPrimitive, HeatmapCell, HeatmapProjection, PastBars, PastTape,
    PriceWindow, SettledProjection, normalized_log_intensity, past_tape::past_timeline,
};
use crate::grouping::{GroupingWindow, sweep_grouped_runs};
use crate::history::LiquidityHistory;
use crate::timeline::BarTimeline;

impl PastTape {
    /// This tape with the book of its own stretch ([`PastTape::book`]),
    /// projected on `prices` and read on `settled`'s intensity scale, so a
    /// wall looks the same held as live. Rebuilt with every request, like the
    /// price window it rows on; the prints' facts are left as they were.
    #[must_use]
    pub fn with_depth(
        mut self,
        history: &LiquidityHistory,
        bars: PastBars<'_>,
        reference_ms: i64,
        prices: PriceWindow,
        settled: &SettledProjection,
    ) -> Self {
        self.book = history.config().depth_visible_anywhere().then(|| {
            let timeline = past_timeline(bars, self.from_ms, self.until_ms, reference_ms);
            let stretch = (self.from_ms, self.until_ms);
            let mut book = HeatmapProjection::empty(true, settled.effective_grouping);
            book.cells = Arc::new(cells(history, &timeline, prices, settled, stretch));
            book.gaps = Arc::new(gaps(history, &timeline, stretch));
            book.liquidity_reference = settled.liquidity_reference;
            Arc::new(book)
        });
        self
    }
}

/// Every run resting inside `[from_ms, until_ms)`, as a cell on the tape.
fn cells(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    prices: PriceWindow,
    settled: &SettledProjection,
    (from_ms, until_ms): (i64, i64),
) -> Vec<HeatmapCell> {
    let config = history.config();
    if !config.show_liquidity {
        return Vec::new();
    }
    let grouping = settled.effective_grouping;
    let start_ms = history
        .retention_start_ms()
        .map_or(from_ms, |retained| retained.max(from_ms));
    let coverage: Vec<_> = history.coverage_segments().cloned().collect();
    let grouped = sweep_grouped_runs(
        history.runs_intersecting(start_ms, until_ms),
        coverage.iter(),
        grouping,
        GroupingWindow {
            start_ms,
            end_ms: until_ms,
            open_run_end_ms: history.latest_book_ms().unwrap_or(until_ms),
            price_low: prices.low,
            price_high: prices.high,
        },
    );
    let mut cells: Vec<HeatmapCell> = grouped
        .runs
        .iter()
        .filter_map(|run| {
            let (y0, y1) = prices.rows(run.price_bucket, grouping.bucket_width)?;
            let x0 = timeline.locate_in_lane_clamped(run.start_ms)?.normalized;
            let x1 = timeline.locate_in_lane_clamped(run.end_ms)?.normalized;
            let intensity =
                normalized_log_intensity(run.quantity, settled.liquidity_reference, config.gamma);
            (x1 > x0).then_some(HeatmapCell {
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
                tape_ms: Some((run.start_ms, run.end_ms)),
            })
        })
        .collect();
    if cells.len() > config.max_visible_cells {
        // The strongest walls first, as the live frame keeps them.
        cells.sort_by(|a, b| {
            b.quantity
                .cmp(&a.quantity)
                .then_with(|| a.generation.cmp(&b.generation))
                .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                .then_with(|| a.x0.total_cmp(&b.x0))
        });
        cells.truncate(config.max_visible_cells);
    }
    cells
}

/// The stretches of `[from_ms, until_ms)` the history holds no book for:
/// before the retained capture — evicted where the history says it let the
/// book go, never captured before that — and every discontinuity inside it.
fn gaps(
    history: &LiquidityHistory,
    timeline: &BarTimeline,
    (from_ms, until_ms): (i64, i64),
) -> Vec<GapPrimitive> {
    if !history.config().show_gaps {
        return Vec::new();
    }
    let mut gaps = Vec::new();
    let mut push = |start_ms: i64, end_ms: i64, reason: &str, generations| {
        let (start_ms, end_ms) = (start_ms.max(from_ms), end_ms.min(until_ms));
        let (Some(x0), Some(x1)) = (
            timeline.locate_in_lane_clamped(start_ms),
            timeline.locate_in_lane_clamped(end_ms),
        ) else {
            return;
        };
        if end_ms > start_ms {
            let (from_generation, to_generation) = generations;
            gaps.push(GapPrimitive {
                from_generation,
                to_generation,
                x0: x0.normalized,
                x1: x1.normalized,
                reason: reason.to_owned(),
                tape_ms: Some((start_ms, end_ms)),
            });
        }
    };
    let first = history.coverage_segments().next();
    let captured_from = first.map_or(until_ms, |segment| segment.start_ms);
    let evicted_through = history
        .book_evicted_through_ms()
        .map_or(from_ms, |evicted| evicted.min(captured_from));
    let opened = first.map(|segment| segment.generation);
    push(from_ms, evicted_through, BOOK_EVICTED, (None, opened));
    push(
        evicted_through,
        captured_from,
        BEFORE_CAPTURE,
        (None, opened),
    );
    for gap in history.coverage_gaps() {
        push(
            gap.start_ms,
            gap.end_ms.unwrap_or(until_ms),
            &gap.reason,
            (gap.from_generation, gap.to_generation),
        );
    }
    gaps
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;

    #[test]
    fn a_book_row_is_cut_to_the_price_window() {
        let prices = PriceWindow::new(Decimal::from(100), Decimal::from(110)).unwrap();
        assert_eq!(
            prices.rows(Decimal::from(104), Decimal::from(2)),
            Some((0.4, 0.6))
        );
        assert_eq!(
            prices.rows(Decimal::from(109), Decimal::from(5)),
            Some((0.0, 0.1))
        );
        assert_eq!(prices.rows(Decimal::from(120), Decimal::from(5)), None);
    }
}
