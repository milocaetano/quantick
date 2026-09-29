//! The optional tape pane retains execution coordinates and readable areas.

use super::*;
use crate::projection::{
    DotSizing, TapeDotGeometry, TapeHorizontalGeometry, merge_tape_dots, position_tape_at,
};

mod dense_window_performance;
mod frame_input;
mod hidden_slots;
mod opening_scale;
mod performance;
mod recorded_stability;
mod stability;

fn tape_config() -> HeatmapConfig {
    let mut config = dots_config();
    config.live_lane.tape_only = true;
    config.bubbles.min_radius = 6.0;
    config.bubbles.max_radius = 15.0;
    config
}

fn tape_dots(window_ms: i64, ticks: i64) -> VolumeDots {
    VolumeDots {
        native_tape: true,
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
    let frame = frame_at(&history, &timeline, prices("90", "110"), &tape_dots(250, 5));
    let marks = frame
        .aggressions
        .into_iter()
        .filter(|mark| mark.live)
        .collect();
    (config, timeline, marks)
}

#[test]
fn closed_tape_dots_keep_the_quantity_weighted_execution_coordinates() {
    let (_, timeline, marks) = fixture(3_400);
    assert_eq!(marks.len(), 1);
    let dot = &marks[0];
    assert_eq!(dot.quantity, dec("0.4"));
    assert_eq!(dot.buy_quantity, dec("0.1"));
    assert_eq!(dot.timestamp_quantity, dec("1216.7"));
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
    assert_eq!(
        forming[0].x,
        timeline.live_now_position().unwrap().normalized
    );
    let (_, early_timeline, early) = fixture(3_400);
    let (_, late_timeline, late) = fixture(3_420);
    let lane_fraction = |timeline: &BarTimeline, mark: &AggressionPrimitive| {
        let (from, _) = timeline.lane_bounds_ms().unwrap();
        let left = timeline.locate_in_lane_clamped(from).unwrap().normalized;
        (mark.x - left) / (1.0 - left)
    };
    let drift = lane_fraction(&early_timeline, &early[0]) - lane_fraction(&late_timeline, &late[0]);
    assert!((drift - 20.0 / 1_500.0).abs() < 1e-12);
}

#[test]
fn the_first_captured_print_appears_before_its_window_closes() {
    let history = tape(tape_config(), &[(1, 3_011, "100", "1", Side::Buy)]);
    let timeline = chart(3_011, 1_500, None);
    let frame = frame_at(&history, &timeline, prices("90", "110"), &tape_dots(100, 1));
    let live: Vec<_> = frame.aggressions.iter().filter(|mark| mark.live).collect();
    assert_eq!(live.len(), 1, "recording may start inside a forming window");
    assert_eq!(live[0].x, 1.0);
    assert_eq!(live[0].quantity, Decimal::ONE);
}

#[test]
fn a_stale_forming_projection_keeps_exact_time_while_the_tape_rolls() {
    let (_, timeline, mut marks) = fixture(3_060);
    let left = timeline.locate_in_lane_clamped(1_560).unwrap().normalized;
    let mut candle = marks[0].clone();
    candle.live = false;
    candle.x = 0.25;
    marks.push(candle.clone());
    position_tape_at(&mut marks, 3_070, 1_500, left, 250);
    assert_eq!(marks[0].x, 1.0, "the open dot still rides NOW");
    position_tape_at(&mut marks, 3_260, 1_500, left, 250);
    let expected = left + (1.0 - left) * ((3_041.75 - 1_760.0) / 1_500.0);
    assert!(
        (marks[0].x - expected).abs() < 1e-12,
        "the closed dot recovers its exact time"
    );
    let first = marks[0].x;
    position_tape_at(&mut marks, 3_290, 1_500, left, 250);
    assert!((first - marks[0].x - (1.0 - left) * 30.0 / 1_500.0).abs() < 1e-12);
    assert_eq!(marks[1], candle, "candle primitives stay unchanged");
    assert_eq!(marks[0].timestamp_quantity, dec("1216.7"));
    position_tape_at(&mut marks, 5_000, 1_500, left, 250);
    assert!(
        marks[0].x < left,
        "an aged-out dot leaves instead of piling at the left edge"
    );
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
        assert!(zoom.native_tape);
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

fn collision_fixture() -> (
    HeatmapConfig,
    DotSizing,
    TapeDotGeometry,
    Vec<AggressionPrimitive>,
) {
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
        mark.buy_share = (mark.buy_quantity / mark.quantity)
            .to_string()
            .parse()
            .unwrap();
        mark.price = dec(price);
        mark.price_bucket = mark.price;
        mark.price_span = Decimal::ONE;
        mark.x = x;
        mark.y = prices("90", "110").y(mark.price).unwrap();
        mark.first_timestamp_ms = id as i64 * 10;
        mark.last_timestamp_ms = mark.first_timestamp_ms;
        mark.timestamp_quantity = Decimal::from(mark.first_timestamp_ms) * mark.quantity;
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
    let moment: Decimal = merged.iter().map(|mark| mark.timestamp_quantity).sum();
    assert_eq!(moment, dec("16.5"));
    let dot = merged.iter().find(|mark| mark.agg_ids == [1, 2]).unwrap();
    assert_eq!(dot.price, dec("100.75"));
    assert!((dot.x - 0.1075).abs() < 1e-12, "no column snapping");
    assert!((dot.y - 0.4625).abs() < 1e-12, "no row snapping");
    assert_eq!(dot.buy_share, 0.25);
    assert_eq!(dot.trade_count, 2);
    let mut reversed = marks;
    reversed.reverse();
    assert_eq!(
        merged,
        merge_tape_dots(
            &reversed,
            sizing,
            &config.bubbles,
            &config.live_lane,
            geometry
        )
    );
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

#[test]
fn budget_folding_preserves_the_exact_execution_time_moment() {
    use crate::projection::fold::{FoldOrder, fold_to_budget};

    let (_, _, _, mut marks) = collision_fixture();
    fold_to_budget(&mut marks, 2, FoldOrder::OldestFirst, Decimal::ONE, None);
    assert!(marks.len() <= 2);
    let moment: Decimal = marks.iter().map(|mark| mark.timestamp_quantity).sum();
    assert_eq!(moment, dec("16.5"));
}

#[test]
fn dots_and_tape_clock_share_one_linear_padded_time_span() {
    let bubbles = BubbleStyle::default();
    let geometry = TapeHorizontalGeometry::resolve(300.0, 400.0, &bubbles);
    assert_eq!(geometry.max_radius, bubbles.max_radius);
    assert!(geometry.x(0.0) >= geometry.max_radius);
    assert!(geometry.x(1.0) + geometry.max_radius <= 300.0);
    assert_eq!(geometry.x(0.5), 150.0, "half the time is half the pane");
    let (now, window) = (20_000_i64, 15_000_i64);
    for instant in [5_000_i64, 8_750, 12_500, 16_250, 20_000] {
        let fraction = (instant - (now - window)) as f64 / window as f64;
        let dot_x = geometry.x(fraction);
        let clock_x = geometry.inset_px + fraction as f32 * geometry.span_px;
        assert_eq!(dot_x, clock_x, "one instant has one horizontal position");
    }
}

#[test]
fn a_narrow_tape_caps_all_radii_equally_and_keeps_now_whole() {
    let mut config = tape_config();
    config.bubbles.max_radius = 48.0;
    let (_, sizing, _, marks) = collision_fixture();
    for width in [300.0, 100.0, 96.0, 60.0, 10.0] {
        let geometry = TapeHorizontalGeometry::resolve(width, 400.0, &config.bubbles);
        assert_eq!(geometry.max_radius, 48.0_f32.min(width / 2.0));
        assert!(geometry.x(1.0) + geometry.max_radius <= width + 1e-5);
        assert!(geometry.x(0.0) - geometry.max_radius >= -1e-5);
        assert!(geometry.span_px >= 0.0);
        let mut fitted = config.bubbles.clone();
        fitted.max_radius = geometry.max_radius;
        let mut big = marks[0].clone();
        big.quantity = dec("4");
        let mut quarter = big.clone();
        quarter.quantity = Decimal::ONE;
        let big_radius = sizing.radius(&fitted, &config.live_lane, &big, dec("4"));
        let quarter_radius = sizing.radius(&fitted, &config.live_lane, &quarter, dec("4"));
        assert!((quarter_radius / big_radius - 0.5).abs() < 1e-6);
    }
    assert_eq!(
        config.bubbles.max_radius, 48.0,
        "fitting never changes the setting"
    );
}

#[test]
fn a_tape_with_only_room_for_one_diameter_still_resolves_collisions() {
    let (mut config, sizing, _, mut marks) = collision_fixture();
    config.bubbles.max_radius = 48.0;
    let horizontal = TapeHorizontalGeometry::resolve(60.0, 200.0, &config.bubbles);
    assert_eq!(horizontal.span_px, 0.0);
    config.bubbles.max_radius = horizontal.max_radius;
    marks.truncate(2);
    for mark in &mut marks {
        mark.y = 0.5;
    }
    let merged = merge_tape_dots(
        &marks,
        sizing,
        &config.bubbles,
        &config.live_lane,
        TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: horizontal.span_px,
            height_px: 200.0,
        },
    );
    assert_eq!(
        merged.len(),
        1,
        "coincident dots cannot hide behind each other"
    );
    assert_eq!(merged[0].quantity, dec("0.4"));
}
