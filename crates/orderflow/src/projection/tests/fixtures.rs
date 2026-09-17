//! Shared original projection-test fixtures, with unchanged bodies.
use crate::config::{DisplayGrouping, HeatmapConfig};
use crate::history::LiquidityHistory;
use crate::projection::{HeatmapProjection, PriceWindow, project};
use crate::timeline::BarTimeline;
use quantick_engine::{Bar, Side, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use rust_decimal::Decimal;
use std::str::FromStr as _;

pub(in crate::projection) fn dec(value: &str) -> Decimal {
    Decimal::from_str(value).unwrap()
}

/// The live edge as the chart supplies it while the newest bar is on
/// screen: the lane shows the recent bars' typical duration, unzoomed.
pub(in crate::projection) fn live(now_ms: i64, closed: &[Bar]) -> Option<crate::LiveEdge> {
    Some(crate::LiveEdge {
        now_ms,
        window_ms: crate::reserved_span_ms(closed),
        reference_ms: crate::reserved_span_ms(closed),
        on_newest_bar: true,
    })
}

pub(in crate::projection) fn level(price: &str, quantity: &str) -> BookLevel {
    BookLevel::new(dec(price), dec(quantity)).unwrap()
}

pub(in crate::projection) fn snapshot(update_id: u64) -> BookSnapshot {
    BookSnapshot::new(
        update_id,
        vec![level("99", "2"), level("100", "3")],
        vec![level("101", "4"), level("102", "5")],
        BookCoverage::Full,
    )
}

pub(in crate::projection) fn bar(open_ms: i64, close_ms: i64) -> Bar {
    Bar {
        open_time: open_ms,
        close_time: close_ms,
        open: dec("100"),
        high: dec("101"),
        low: dec("99"),
        close: dec("100"),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

pub(in crate::projection) fn config() -> HeatmapConfig {
    HeatmapConfig {
        enabled: true,
        show_aggressions: true,
        price_grouping: Decimal::ONE,
        ..HeatmapConfig::default()
    }
}

/// Baseline for every evidence-related test.
pub(in crate::projection) fn event_config() -> HeatmapConfig {
    HeatmapConfig {
        enabled: true,
        show_aggressions: true,
        price_grouping: Decimal::ONE,
        display_grouping: DisplayGrouping::Native,
        bubble_cluster_ms: 100,
        liquidity_correlation_ms: 250,
        ..HeatmapConfig::default()
    }
}

/// One buy print plus two ask reductions: the one at t=500 has compatible
/// aggression evidence, the full pull at t=800 is depth-only.
pub(in crate::projection) fn reduction_history(config: HeatmapConfig) -> LiquidityHistory {
    let mut history = LiquidityHistory::new(config);
    history
        .install_snapshot(
            100,
            1,
            BookSnapshot::new(
                10,
                vec![level("100", "2")],
                vec![level("101", "10")],
                BookCoverage::Full,
            ),
        )
        .unwrap();
    history.record_aggression(&Trade {
        agg_id: 77,
        timestamp_ms: 480,
        price: dec("101"),
        quantity: dec("3"),
        side: Side::Buy,
    });
    history
        .apply_delta(
            500,
            &BookDelta::new(11, 11, vec![], vec![level("101", "6")]),
        )
        .unwrap();
    history
        .apply_delta(
            800,
            &BookDelta::new(12, 12, vec![], vec![level("101", "0")]),
        )
        .unwrap();
    history
}

pub(in crate::projection) fn project_reductions(config: HeatmapConfig) -> HeatmapProjection {
    let history = reduction_history(config);
    project(
        &history,
        &BarTimeline::from_bars(0, &[bar(0, 1_000)], None, None),
        PriceWindow::new(dec("99"), dec("103")).unwrap(),
    )
}
