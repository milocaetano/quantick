//! Frozen matched workloads for settled-projection decomposition evidence.

use quantick_engine::{Bar, Side, Trade};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};
use quantick_orderflow::{
    BarTimeline, BubbleStyle, DisplayGrouping, HeatmapConfig, HeatmapProjection, LiquidityHistory,
    LiveEdge, LiveLaneStyle, PriceWindow, project_live, project_settled,
};
use rust_decimal::Decimal;

pub struct Fixture {
    pub history: LiquidityHistory,
    pub timeline: BarTimeline,
    pub prices: PriceWindow,
}

pub fn bar(open_time: i64, close_time: i64) -> Bar {
    Bar {
        open_time,
        close_time,
        open: Decimal::from(100),
        high: Decimal::from(101),
        low: Decimal::from(99),
        close: Decimal::from(100),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    }
}

// Exact configuration and fixture values of the existing W1 and W2 unit
// benchmarks. Their original bodies and timing loops remain unchanged.
pub fn config(workload: u8) -> HeatmapConfig {
    match workload {
        1 => HeatmapConfig {
            enabled: false,
            show_aggressions: true,
            price_grouping: Decimal::ONE,
            display_grouping: DisplayGrouping::Adaptive { target_rows: 128 },
            bubble_cluster_ms: 0,
            bubble_dust_merge_ms: 0,
            bubble_candle_summary: true,
            bubbles: BubbleStyle {
                readable_min_radius: 0.0,
                ..BubbleStyle::default()
            },
            ..HeatmapConfig::default()
        },
        2 => HeatmapConfig {
            enabled: true,
            show_aggressions: true,
            price_grouping: Decimal::ONE,
            display_grouping: DisplayGrouping::Adaptive { target_rows: 128 },
            bubble_candle_summary: true,
            bubble_cluster_ms: 500,
            bubble_dust_merge_ms: 1_500,
            live_lane: LiveLaneStyle {
                enabled: true,
                cluster_ms: Some(100),
                ..LiveLaneStyle::default()
            },
            ..HeatmapConfig::default()
        },
        3 => HeatmapConfig {
            max_visible_cells: 1_024,
            max_history_runs: 500_000,
            max_history_bytes: 128 * 1024 * 1024,
            min_unattributed_reduction: 0.0,
            min_unattributed_pull_share: 0.0,
            ..config(2)
        },
        _ => panic!("unknown frozen workload"),
    }
}

fn level(price: u64, quantity: u64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).unwrap()
}

fn snapshot(update_id: u64) -> BookSnapshot {
    BookSnapshot::new(
        update_id,
        (0..128).map(|i| level(300 - i, 10 + i % 23)).collect(),
        (0..128).map(|i| level(301 + i, 12 + i % 19)).collect(),
        BookCoverage::Full,
    )
}

pub fn dense(workload: u8) -> Fixture {
    let mut history = LiquidityHistory::new(config(workload));
    if workload == 3 {
        history.install_snapshot(1_000, 1, snapshot(1)).unwrap();
    }
    let mut update_id = 1;
    for i in 0..40_000_u64 {
        let timestamp_ms = 100 + i as i64 * 25;
        // W3: 16 changing levels per side each 100 ms, rotating across all
        // 128 levels. The zero phase records explicit removals. One 1-second
        // coverage interruption ends generation 1 before resynchronization.
        if workload == 3 && timestamp_ms >= 1_100 && i % 4 == 0 {
            if timestamp_ms == 500_000 {
                history.mark_gap(timestamp_ms, "fixture_resync").unwrap();
            } else if timestamp_ms == 501_000 {
                update_id += 1;
                history
                    .install_snapshot(timestamp_ms, 2, snapshot(update_id))
                    .unwrap();
            } else if !(500_000..501_000).contains(&timestamp_ms) {
                update_id += 1;
                let step = (timestamp_ms - 1_100) as u64 / 100;
                let quantities = [28, 7, 0, 19];
                let bids = (0..16)
                    .map(|j| {
                        let index = (step * 16 + j) % 128;
                        level(300 - index, quantities[((step / 8 + j) % 4) as usize])
                    })
                    .collect();
                let asks = (0..16)
                    .map(|j| {
                        let index = (step * 16 + j) % 128;
                        level(301 + index, quantities[((step / 8 + j + 1) % 4) as usize])
                    })
                    .collect();
                history
                    .apply_delta(
                        timestamp_ms,
                        &BookDelta::new(update_id, update_id, bids, asks),
                    )
                    .unwrap();
            }
        }
        history.record_aggression(&Trade {
            agg_id: i + 1,
            timestamp_ms,
            price: Decimal::from(100 + i % 400),
            quantity: Decimal::from(i % 23 + 1),
            side: if i % 2 == 0 { Side::Buy } else { Side::Sell },
        });
    }
    let closed: Vec<_> = (0..200)
        .map(|i| bar(i * 5_000, i * 5_000 + 4_999))
        .collect();
    let partial = bar(1_000_000, 1_005_000);
    let timeline = BarTimeline::from_bars(
        0,
        &closed,
        Some(&partial),
        Some(LiveEdge {
            now_ms: 1_000_100,
            window_ms: 5_000,
            reference_ms: 5_000,
            on_newest_bar: true,
        }),
    );
    Fixture {
        history,
        timeline,
        prices: PriceWindow::new(Decimal::from(90), Decimal::from(520)).unwrap(),
    }
}

pub fn whole(f: &Fixture) -> HeatmapProjection {
    let settled = project_settled(&f.history, &f.timeline, f.prices);
    let live = project_live(&f.history, &f.timeline, f.prices, &settled);
    settled.with_live(live, f.history.config())
}
