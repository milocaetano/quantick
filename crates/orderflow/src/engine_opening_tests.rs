//! The tape discovers its native price grid after executions have arrived.

use super::*;
use crate::projection::{DotSizing, TapeDotGeometry, TapeDotMemory, TapeDotView};
use quantick_engine::Side;

fn print(id: u64, time: i64, price: i64, quantity: i64, side: Side) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: time,
        price: Decimal::from(price),
        quantity: Decimal::from(quantity),
        side,
    }
}

fn engine(max_aggressions: usize) -> BookEngine {
    let mut engine = BookEngine::new("WINV26");
    engine.apply_visual_config(HeatmapConfig {
        show_aggressions: true,
        max_aggressions,
        live_lane: crate::LiveLaneStyle {
            enabled: true,
            show_depth: false,
            show_aggressions: true,
            tape_only: true,
            window: crate::LaneWindow::Fixed { ms: 30_000 },
            ..Default::default()
        },
        volume_dots: crate::config::VolumeDotStyle {
            enabled: true,
            ..Default::default()
        },
        ..Default::default()
    });
    engine
}

#[test]
fn inferred_tape_grid_preserves_the_exact_opening_prefix_and_its_clock() {
    let mut engine = engine(100_000);
    // The real WIN opening has 74,365 at its first native price. The first
    // 46 executions total 74,967 before the asynchronous grid command lands.
    for trade in [
        print(1, 717, 187_225, 74_365, Side::Buy),
        print(2, 727, 187_230, 602, Side::Buy),
    ] {
        engine.record_trade(&trade);
    }
    let before: Vec<_> = engine.history.aggressions().cloned().collect();
    let counters = engine.history.counters();
    engine.size_from_tape(Decimal::from(5), Some(Decimal::from(187_225)));
    assert_eq!(engine.base_capture_grouping(), Decimal::from(5));
    assert_eq!(engine.history.aggressions().cloned().collect::<Vec<_>>(), before);
    assert_eq!(engine.history.latest_print_ms(), Some(727));
    assert_eq!(engine.history.recorded_from_ms(), Some(717));
    assert_eq!(engine.history.evicted_through_ms(), None);
    assert_eq!(engine.history.opening_bursts(), &[700]);
    assert_eq!(engine.history.counters(), counters);
    assert_eq!(engine.history.status(), HistoryStatus::Empty);
    assert!(!engine.history.book().is_initialized());
}

#[test]
fn inferred_grid_does_not_tombstone_the_initial_native_tape_window() {
    let mut engine = engine(100_000);
    let first = print(1, 717, 187_225, 74_967, Side::Buy);
    engine.record_trade(&first);
    engine.size_from_tape(Decimal::from(5), Some(first.price));
    engine.record_trade(&print(2, 799, 187_055, 3_229, Side::Buy));
    engine.record_trade(&print(3, 1_800, 186_800, 100, Side::Sell));
    let mut partial = Bar::opened_by(&first);
    partial.close_time = 1_800;
    let frame = engine
        .project_at(
            &ProjectionRequest {
                timeline_revision: 1,
                first_bar_index: 0,
                closed: Vec::new(),
                partial: Some(partial),
                lane: true,
                on_newest_bar: true,
                lane_reference_ms: Some(30_000),
                lane_now_ms: Some(20_700),
                price_range: (186_500.0, 187_300.0),
                dot_zoom: Some(DotZoom {
                    tape_only: true,
                    tape_window_ms: 100,
                    tape_level_ticks: 1,
                    candle_level_ticks: 1,
                    lane_bars: vec![(717, 1_800)],
                }),
            },
            Instant::now(),
        )
        .unwrap();
    let facts = frame.projection.tape_facts.as_ref().unwrap();
    assert_eq!(facts.evicted_through_ms, None);
    assert_eq!(facts.opening_bursts, [700]);
    let native: Vec<_> = frame
        .projection
        .aggressions
        .iter()
        .filter(|mark| mark.live)
        .cloned()
        .collect();
    for openings in [&[][..], facts.opening_bursts.as_slice()] {
        let drawn = TapeDotMemory::default().project(
            &native,
            TapeDotView {
                now_ms: 20_700,
                window_ms: 30_000,
                dot_window_ms: 100,
                evicted_through_ms: facts.evicted_through_ms,
                prices: PriceWindow::new(Decimal::from(186_500), Decimal::from(187_300)).unwrap(),
                geometry: TapeDotGeometry {
                    left_x: 0.0,
                    right_x: 1.0,
                    width_px: 640.0,
                    height_px: 400.0,
                },
            },
            DotSizing {
                tape_column_px: 1.0,
                candle_column_px: 1.0,
                px_per_price: 1.0,
                typed_full: None,
            },
            &engine.config.bubbles,
            &engine.config.live_lane,
            openings,
        );
        assert_eq!(drawn.marks.iter().map(|mark| mark.quantity).sum::<Decimal>(), Decimal::from(78_296));
        assert_eq!(drawn.marks.iter().map(|mark| mark.buy_quantity).sum::<Decimal>(), Decimal::from(78_196));
        let mut ids: Vec<_> = drawn.marks.iter().flat_map(|mark| mark.agg_ids.iter().copied()).collect();
        ids.sort_unstable();
        assert_eq!(ids, [1, 2, 3]);
    }
}

#[test]
fn inferred_grid_preserves_an_existing_retention_horizon_without_advancing_it() {
    let mut engine = engine(2);
    for id in 1..=3 {
        engine.record_trade(&print(id, id as i64 * 100, 187_225, id as i64, Side::Buy));
    }
    let before: Vec<_> = engine.history.aggressions().cloned().collect();
    assert_eq!(engine.history.evicted_through_ms(), Some(100));
    let counters = engine.history.counters();
    engine.size_from_tape(Decimal::from(5), Some(Decimal::from(187_225)));
    assert_eq!(engine.history.aggressions().cloned().collect::<Vec<_>>(), before);
    assert_eq!(engine.history.evicted_through_ms(), Some(100));
    assert_eq!(engine.history.opening_bursts(), &[100]);
    assert_eq!(engine.history.counters(), counters);
}
