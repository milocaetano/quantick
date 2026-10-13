//! Native display rows already have the capture's quantities. Clip and merge
//! their intervals without rebuilding per-level timestamp trees or transitions
//! that the heat painter never reads.

use crate::grouping::{GroupingWindow, VisualLiquidityRun};
use crate::history::LiquidityHistory;
use quantick_orderbook::BookSide;

pub(super) fn project(
    history: &LiquidityHistory,
    window: GroupingWindow,
) -> Vec<VisualLiquidityRun> {
    let width = history.config().price_grouping;
    let render_end = window.end_ms.min(window.open_run_end_ms);
    if render_end <= window.start_ms || window.price_high <= window.price_low {
        return Vec::new();
    }
    let mut runs: Vec<_> = history
        .runs_intersecting(window.start_ms, window.end_ms)
        .filter_map(|run| {
            let start_ms = run.start_ms.max(window.start_ms);
            let end_ms = run.end_ms.unwrap_or(render_end).min(render_end);
            (end_ms > start_ms
                && run.price_bucket < window.price_high
                && run.price_bucket + width > window.price_low)
                .then_some(VisualLiquidityRun {
                    generation: run.generation,
                    side: run.side,
                    price_bucket: run.price_bucket,
                    quantity: run.quantity,
                    start_ms,
                    end_ms,
                })
        })
        .collect();
    runs.sort_unstable_by_key(|run| {
        (
            run.generation,
            run.side == BookSide::Ask,
            run.price_bucket,
            run.start_ms,
            run.end_ms,
        )
    });
    // Two updates at one timestamp can briefly change a quantity and restore
    // it. Their zero-length middle run is invisible, so the surrounding equal
    // quantities must join exactly as the general timestamp sweep joins them.
    let mut merged: Vec<VisualLiquidityRun> = Vec::with_capacity(runs.len());
    for run in runs {
        if let Some(last) = merged.last_mut()
            && last.generation == run.generation
            && last.side == run.side
            && last.price_bucket == run.price_bucket
            && last.quantity == run.quantity
            && last.end_ms == run.start_ms
        {
            last.end_ms = run.end_ms;
        } else {
            merged.push(run);
        }
    }
    merged.sort_unstable_by_key(|run| {
        (
            run.start_ms,
            run.end_ms,
            run.generation,
            run.side == BookSide::Ask,
            run.price_bucket,
        )
    });
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DisplayGrouping, HeatmapConfig};
    use crate::grouping::{EffectiveGrouping, sweep_grouped_runs};
    use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
    use rust_decimal::Decimal;

    fn level(price: i64, quantity: i64) -> BookLevel {
        BookLevel::new(Decimal::new(price, 2), Decimal::from(quantity)).unwrap()
    }

    fn history(retention_ms: i64) -> LiquidityHistory {
        let mut history = LiquidityHistory::new(HeatmapConfig {
            price_grouping: Decimal::new(1, 1),
            retention_ms,
            ..HeatmapConfig::default()
        });
        history
            .install_snapshot(
                1_000,
                1,
                BookSnapshot::new(
                    0,
                    vec![level(9_995, 7), level(9_990, 3)],
                    vec![level(10_005, 8), level(10_090, 1)],
                    BookCoverage::Full,
                ),
            )
            .unwrap();
        for (id, time, bid, ask) in [
            (1, 1_200, 9, 8),
            (2, 1_200, 7, 8),
            (3, 1_600, 0, 12),
            (4, 1_800, 4, 0),
            (5, 2_000, 4, 0),
        ] {
            history
                .apply_delta(
                    time,
                    &BookDelta::new(id, id, vec![level(9_995, bid)], vec![level(10_005, ask)]),
                )
                .unwrap();
        }
        history
    }

    fn assert_sweep_matches(history: &LiquidityHistory) {
        let grouping = EffectiveGrouping::resolve(
            DisplayGrouping::Native,
            history.config().price_grouping,
            Decimal::ZERO,
        );
        for (start_ms, end_ms, open_run_end_ms) in [
            (500, 2_000, 2_000),
            (1_200, 1_600, 2_000),
            (1_250, 1_900, 2_000),
            (1_000, 2_500, 2_500),
            (1_000, 2_501, 2_000),
            (2_500, 4_000, 3_500),
            (500, 4_000, 3_500),
            (4_000, 4_000, 4_000),
        ] {
            let start_ms = history
                .retention_start_ms()
                .map_or(start_ms, |floor| start_ms.max(floor));
            for (low, high) in [(9_000, 11_000), (9_992, 10_003), (10_002, 10_080)] {
                let window = GroupingWindow {
                    start_ms,
                    end_ms,
                    open_run_end_ms,
                    price_low: Decimal::new(low, 2),
                    price_high: Decimal::new(high, 2),
                };
                assert_eq!(
                    project(history, window),
                    sweep_grouped_runs(
                        history.runs_intersecting(start_ms, end_ms),
                        history.coverage_segments(),
                        grouping,
                        window,
                    )
                    .runs,
                    "{window:?}",
                );
            }
        }
    }

    #[test]
    fn native_rows_preserve_clipping_carry_and_same_timestamp_changes() {
        assert_sweep_matches(&history(30_000));
        let mut clipped = history(1_000);
        clipped
            .apply_delta(2_500, &BookDelta::new(6, 6, vec![], vec![]))
            .unwrap();
        assert_eq!(clipped.retention_start_ms(), Some(1_500));
        assert_sweep_matches(&clipped);
    }

    #[test]
    fn native_rows_preserve_generation_gaps_and_replaced_snapshots() {
        let mut history = history(30_000);
        history.mark_gap(2_200, "transport_closed").unwrap();
        assert_sweep_matches(&history);
        history
            .install_snapshot(
                2_800,
                2,
                BookSnapshot::new(
                    50,
                    vec![level(9_995, 4)],
                    vec![level(10_005, 8)],
                    BookCoverage::Full,
                ),
            )
            .unwrap();
        history
            .apply_delta(3_500, &BookDelta::new(51, 51, vec![], vec![]))
            .unwrap();
        assert_sweep_matches(&history);
    }
}
