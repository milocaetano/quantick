//! The optional tape pane retains execution coordinates and readable areas.

use super::*;
use crate::projection::{DotSizing, TapeDotGeometry, merge_tape_dots};

fn tape_config() -> HeatmapConfig {
    let mut config = dots_config();
    config.live_lane.tape_only = true;
    config.bubbles.min_radius = 6.0;
    config.bubbles.max_radius = 15.0;
    config
}

fn tape_dots(window_ms: i64, ticks: i64) -> VolumeDots {
    VolumeDots {
        tape_only: true,
        ..coarse(window_ms, ticks)
    }
}

fn fixture(now_ms: i64) -> (HeatmapConfig, BarTimeline, Vec<AggressionPrimitive>) {
    let config = tape_config();
    let history = recorded(
        config.clone(),
        &[
            (1, 3_011, "100", "0.1", Side::Buy),
            (2, 3_052, "101", "0.3", Side::Sell),
        ],
    );
    let timeline = chart(now_ms, 1_500, None);
    let frame = frame_at(
        &history,
        &timeline,
        prices("90", "110"),
        &tape_dots(250, 5),
    );
    let marks = frame.aggressions.into_iter().filter(|mark| mark.live).collect();
    (config, timeline, marks)
}

#[test]
fn closed_tape_dots_keep_the_quantity_weighted_execution_coordinates() {
    let (_, timeline, marks) = fixture(3_400);
    assert_eq!(marks.len(), 1);
    let dot = &marks[0];
    assert_eq!(dot.quantity, dec("0.4"));
    assert_eq!(dot.buy_quantity, dec("0.1"));
    assert_eq!(dot.price, dec("100.75"), "no tick or level-centre snap");
    assert_eq!(dot.y, prices("90", "110").y(dec("100.75")).unwrap());
    let lower = timeline.locate_in_lane_clamped(3_041).unwrap().normalized;
    let upper = timeline.locate_in_lane_clamped(3_042).unwrap().normalized;
    let expected = lower + (upper - lower) * 0.75;
    assert!((dot.x - expected).abs() < 1e-12, "quantity-weighted time");
}

#[test]
fn the_open_tape_window_rides_now_and_then_moves_by_elapsed_time() {
    let (_, timeline, forming) = fixture(3_060);
    assert_eq!(forming.len(), 1, "the latest print appears immediately");
    assert_eq!(forming[0].x, timeline.live_now_position().unwrap().normalized);
    let (_, early_timeline, early) = fixture(3_400);
    let (_, late_timeline, late) = fixture(3_420);
    let lane_fraction = |timeline: &BarTimeline, mark: &AggressionPrimitive| {
        let (from, _) = timeline.lane_bounds_ms().unwrap();
        let left = timeline.locate_in_lane_clamped(from).unwrap().normalized;
        (mark.x - left) / (1.0 - left)
    };
    let drift = lane_fraction(&early_timeline, &early[0])
        - lane_fraction(&late_timeline, &late[0]);
    assert!((drift - 20.0 / 1_500.0).abs() < 1e-12);
}

#[test]
fn the_first_captured_print_appears_before_its_window_closes() {
    let history = tape(tape_config(), &[(1, 3_011, "100", "1", Side::Buy)]);
    let timeline = chart(3_011, 1_500, None);
    let frame = frame_at(
        &history,
        &timeline,
        prices("90", "110"),
        &tape_dots(100, 1),
    );
    let live: Vec<_> = frame.aggressions.iter().filter(|mark| mark.live).collect();
    assert_eq!(live.len(), 1, "recording may start inside a forming window");
    assert_eq!(live[0].x, 1.0);
    assert_eq!(live[0].quantity, Decimal::ONE);
}

#[test]
fn tape_only_keeps_short_native_price_windows_at_every_tape_zoom() {
    let config = tape_config();
    for lane_window_ms in [15_000, 60_000, 300_000, 900_000] {
        let geometry = PaneGeometry {
            px_per_bar: 3.0,
            lane_width_px: 300.0,
            lane_window_ms,
            height_px: 400.0,
            lane_bars: Vec::new(),
        };
        let zoom = DotRungMemory::default().choose(geometry, &config, (0.0, 1_000.0), Some(80.0));
        assert!(zoom.tape_window_ms <= 250, "no coarse gaps: {zoom:?}");
        assert_eq!(zoom.tape_level_ticks, 1, "native prices are the base keys");
        assert!(zoom.tape_only);
    }
}

#[test]
fn tape_area_is_proportional_without_a_cell_size_or_minimum_radius_floor() {
    let (config, _, mut marks) = fixture(3_400);
    let sizing = DotSizing {
        tape_column_px: 1.0,
        candle_column_px: 1.0,
        px_per_price: 0.1,
        typed_full: None,
    };
    let biggest = marks[0].quantity;
    let big = sizing.radius(&config.bubbles, &config.live_lane, &marks[0], biggest);
    marks[0].quantity /= Decimal::from(4);
    let quarter = sizing.radius(&config.bubbles, &config.live_lane, &marks[0], biggest);
    assert_eq!(big, 15.0, "the cell never makes the tape tiny");
    assert!((quarter / big - 0.5).abs() < 1e-6, "area follows quantity");
    let mut legacy = config.live_lane;
    legacy.tape_only = false;
    assert!(sizing.radius(&config.bubbles, &legacy, &marks[0], biggest) <= 0.5);
}

fn collision_fixture() -> (HeatmapConfig, DotSizing, TapeDotGeometry, Vec<AggressionPrimitive>) {
    let (config, _, original) = fixture(3_400);
    let mut marks = Vec::new();
    for (id, x, quantity, buy_quantity, price) in [
        (1, 0.10, "0.1", "0.1", "100"),
        (2, 0.11, "0.3", "0", "101"),
        (3, 0.55, "0.1", "0.1", "105"),
        (4, 0.56, "0.1", "0", "105"),
        (5, 0.90, "0.05", "0.05", "103"),
    ] {
        let mut mark = original[0].clone();
        mark.agg_id = id;
        mark.agg_ids = vec![id];
        mark.quantity = dec(quantity);
        mark.buy_quantity = dec(buy_quantity);
        mark.buy_share = (mark.buy_quantity / mark.quantity).to_string().parse().unwrap();
        mark.price = dec(price);
        mark.price_bucket = mark.price;
        mark.price_span = Decimal::ONE;
        mark.x = x;
        mark.y = prices("90", "110").y(mark.price).unwrap();
        mark.first_timestamp_ms = id as i64 * 10;
        mark.last_timestamp_ms = mark.first_timestamp_ms;
        mark.trade_count = 1;
        marks.push(mark);
    }
    let sizing = DotSizing {
        tape_column_px: 1.0,
        candle_column_px: 10.0,
        px_per_price: 10.0,
        typed_full: None,
    };
    let geometry = TapeDotGeometry {
        left_x: 0.0,
        right_x: 1.0,
        width_px: 300.0,
        height_px: 200.0,
    };
    (config, sizing, geometry, marks)
}

#[test]
fn colliding_tape_dots_merge_at_the_weighted_coordinates_with_exact_sums() {
    let (config, sizing, geometry, marks) = collision_fixture();
    let merged = merge_tape_dots(&marks, sizing, &config.bubbles, &config.live_lane, geometry);
    let total: Decimal = merged.iter().map(|mark| mark.quantity).sum();
    let buys: Decimal = merged.iter().map(|mark| mark.buy_quantity).sum();
    assert_eq!(total, dec("0.65"));
    assert_eq!(buys, dec("0.25"));
    let dot = merged.iter().find(|mark| mark.agg_ids == [1, 2]).unwrap();
    assert_eq!(dot.price, dec("100.75"));
    assert!((dot.x - 0.1075).abs() < 1e-12, "no column snapping");
    assert!((dot.y - 0.4625).abs() < 1e-12, "no row snapping");
    assert_eq!(dot.buy_share, 0.25);
    assert_eq!(dot.trade_count, 2);
    let mut reversed = marks;
    reversed.reverse();
    assert_eq!(merged, merge_tape_dots(&reversed, sizing, &config.bubbles, &config.live_lane, geometry));
    let full = sizing.full_quantity(&merged, true);
    for (index, a) in merged.iter().enumerate() {
        for b in &merged[index + 1..] {
            let distance = ((a.x - b.x) * 300.0).hypot((a.y - b.y) * 200.0);
            let a_radius = sizing.radius(&config.bubbles, &config.live_lane, a, full);
            let b_radius = sizing.radius(&config.bubbles, &config.live_lane, b, full);
            assert!(distance >= f64::from(a_radius + b_radius) * 0.9 - 1e-5);
        }
    }
}

#[test]
fn a_collision_at_now_keeps_the_forming_dot_whole_at_the_edge() {
    let (config, sizing, geometry, mut marks) = collision_fixture();
    marks.truncate(2);
    marks[0].x = 1.0;
    marks[1].x = 0.999;
    let merged = merge_tape_dots(&marks, sizing, &config.bubbles, &config.live_lane, geometry);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].x, 1.0, "a forming merge remains at now");
    assert_eq!(merged[0].quantity, dec("0.4"));
}
