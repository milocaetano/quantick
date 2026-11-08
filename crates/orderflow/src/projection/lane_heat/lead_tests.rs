//! The native tape's averaged book fills the lane edge to edge: older depth
//! in the lead left of the oldest dot, the live current book past NOW, both
//! on the clock the dots use.

use super::*;
use crate::projection::{DotRungMemory, PaneGeometry, TapeHorizontalGeometry};
use crate::{DisplayGrouping, LiveEdge};
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

const LANE_WIDTH_PX: f32 = 400.0;
const LANE_HEIGHT_PX: f32 = 600.0;
const NOW_MS: i64 = 2_000;
const WINDOW_MS: i64 = 500;
const LANE_START_MS: i64 = NOW_MS - WINDOW_MS;
/// A wall that changes inside every lead the tests use.
const LEAD_CHANGE_MS: i64 = LANE_START_MS - 20;

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(price.into(), quantity.into()).unwrap()
}

/// A synchronized averaged book captured from `from_ms`, confirming at
/// 1,900 so it is still the current book at NOW.
fn history_from(from_ms: i64) -> LiquidityHistory {
    let mut config = crate::HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        max_visible_cells: 1_000,
        ..crate::HeatmapConfig::default()
    };
    config.live_lane.depth_time_averaging = true;
    let mut history = LiquidityHistory::new(config);
    history
        .install_snapshot(
            from_ms,
            1,
            BookSnapshot::new(
                10,
                vec![level(95, 100), level(99, 1)],
                vec![level(101, 1)],
                BookCoverage::Full,
            ),
        )
        .unwrap();
    let changes = [(LEAD_CHANGE_MS, 300), (1_900, 600)];
    for (id, (time, quantity)) in
        (11..).zip(changes.into_iter().filter(|&(time, _)| time > from_ms))
    {
        {
            history
                .apply_delta(
                    time,
                    &BookDelta::new(id, id, vec![level(95, quantity)], vec![]),
                )
                .unwrap();
        }
    }
    history
}

fn bubbles(max_radius: f32) -> crate::config::BubbleStyle {
    crate::config::BubbleStyle {
        max_radius,
        ..crate::HeatmapConfig::default().bubbles
    }
}

fn geometry(max_radius: f32) -> TapeHorizontalGeometry {
    TapeHorizontalGeometry::resolve(LANE_WIDTH_PX, LANE_HEIGHT_PX, &bubbles(max_radius))
}

fn timeline(lead_ms: i64) -> BarTimeline {
    let bar = quantick_engine::Bar {
        open_time: 1_000,
        close_time: NOW_MS,
        open: 100.into(),
        high: 101.into(),
        low: 99.into(),
        close: 100.into(),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    };
    BarTimeline::from_bars(
        0,
        &[bar],
        None,
        Some(LiveEdge {
            now_ms: NOW_MS,
            window_ms: WINDOW_MS,
            reference_ms: WINDOW_MS,
            on_newest_bar: true,
        }),
    )
    .with_full_lane_coverage()
    .with_lane_lead_ms(lead_ms)
}

fn lane(history: &LiquidityHistory, timeline: &BarTimeline) -> Vec<HeatmapCell> {
    let grouping = EffectiveGrouping::resolve(DisplayGrouping::Native, Decimal::ONE, 8.into());
    project_lane_heat(
        history,
        timeline,
        PriceWindow::new(94.into(), 102.into()).unwrap(),
        grouping,
        130.into(),
    )
    .0
}

/// Pixels from the lane's left edge each cell is painted over: the tape's
/// own span where it gives one, the lane's opening-to-NOW clock otherwise.
fn painted(
    geometry: TapeHorizontalGeometry,
    cells: &[HeatmapCell],
    regions: usize,
    live: bool,
) -> Vec<(f32, f32)> {
    let fraction = |x: f64| x * regions as f64 - (regions - 1) as f64;
    cells
        .iter()
        .map(|cell| {
            geometry
                .book_span_px(cell, regions, live)
                .unwrap_or_else(|| (geometry.x(fraction(cell.x0)), geometry.x(fraction(cell.x1))))
        })
        .collect()
}

fn reach(spans: &[(f32, f32)]) -> (f32, f32) {
    spans.iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(low, high), &(x0, x1)| (low.min(x0), high.max(x1)),
    )
}

#[test]
fn the_book_reaches_both_edges_at_any_bubble_size() {
    let history = history_from(1_000);
    for max_radius in [4.0, 40.0] {
        let geometry = geometry(max_radius);
        assert!(
            geometry.inset_px > 0.0,
            "radius {max_radius} insets the dots"
        );
        let timeline = timeline(geometry.lead_ms(WINDOW_MS));
        let cells = lane(&history, &timeline);
        let regions = timeline.region_count();
        let (left, right) = reach(&painted(geometry, &cells, regions, true));
        assert!(
            left.abs() < 1e-3,
            "radius {max_radius}: the lead starts at {left}"
        );
        assert!(
            (right - LANE_WIDTH_PX).abs() < 1e-3,
            "radius {max_radius}: the book ends at {right}"
        );
        let (_, at_now) = reach(&painted(geometry, &cells, regions, false));
        assert!(
            (at_now - geometry.x(1.0)).abs() < 1e-3,
            "radius {max_radius}: a book that is not live stops at NOW, not {at_now}"
        );
    }
}

#[test]
fn the_lead_continues_the_dots_clock_and_never_stretches_the_oldest_column() {
    let history = history_from(1_000);
    let geometry = geometry(40.0);
    let timeline = timeline(geometry.lead_ms(WINDOW_MS));
    let cells = lane(&history, &timeline);
    let regions = timeline.region_count();
    let spans = painted(geometry, &cells, regions, true);
    let change_x = geometry.x((LEAD_CHANGE_MS - LANE_START_MS) as f64 / WINDOW_MS as f64);
    assert!(change_x > 0.0 && change_x < geometry.inset_px);
    let wall = |quantity: i64| {
        cells
            .iter()
            .zip(&spans)
            .find(|(cell, _)| {
                cell.price_bucket == Decimal::from(95) && cell.quantity == Decimal::from(quantity)
            })
            .map(|(cell, &span)| (cell, span))
            .unwrap()
    };
    let (before, (from, until)) = wall(100);
    assert!(before.lead_span.is_some(), "older depth is the lead's");
    assert!(
        from.abs() < 1e-3 && (until - change_x).abs() < 1e-3,
        "{before:?} paints {from}..{until}, the change is at {change_x}"
    );
    let (after, (from, _)) = wall(300);
    assert!(
        after.lead_span.is_some(),
        "a run across the opening starts in the lead"
    );
    assert!(
        (from - change_x).abs() < 1e-3,
        "it starts where it changed: {from}"
    );
    for cell in &cells {
        let opening = (regions - 1) as f64 / regions as f64;
        assert!(
            cell.x0 >= opening,
            "a lead cell still reads as the lane's: {cell:?} against {opening}"
        );
    }
}

#[test]
fn the_lead_stays_empty_where_no_history_was_captured() {
    let history = history_from(LANE_START_MS);
    let geometry = geometry(40.0);
    let timeline = timeline(geometry.lead_ms(WINDOW_MS));
    let cells = lane(&history, &timeline);
    assert!(cells.iter().all(|cell| cell.lead_span.is_none()));
    let (left, _) = reach(&painted(geometry, &cells, timeline.region_count(), true));
    assert!(
        (left - geometry.inset_px).abs() < 1e-3,
        "nothing is invented before the capture: {left}"
    );
}

#[test]
fn a_lane_without_averaging_takes_no_lead() {
    let mut history = history_from(1_000);
    let mut config = history.config().clone();
    config.live_lane.depth_time_averaging = false;
    history.update_config(config).unwrap();
    let timeline = timeline(geometry(40.0).lead_ms(WINDOW_MS));
    assert!(
        lane(&history, &timeline)
            .iter()
            .all(|cell| cell.lead_span.is_none())
    );
}

#[test]
fn the_view_asks_for_the_lead_only_of_an_averaged_native_tape() {
    let mut config = crate::HeatmapConfig {
        enabled: true,
        ..crate::HeatmapConfig::default()
    };
    config.live_lane.enabled = true;
    config.live_lane.tape_only = true;
    config.bubbles = bubbles(40.0);
    let zoom = |config: &crate::HeatmapConfig| {
        DotRungMemory::default().choose(
            PaneGeometry {
                px_per_bar: 8.0,
                lane_width_px: LANE_WIDTH_PX,
                lane_window_ms: WINDOW_MS,
                height_px: LANE_HEIGHT_PX,
                lane_bars: Vec::new(),
            },
            config,
            (94.0, 102.0),
            None,
        )
    };
    config.live_lane.depth_time_averaging = true;
    assert!(config.native_tape());
    let lead_ms = zoom(&config).tape_lead_ms(WINDOW_MS);
    assert_eq!(lead_ms, geometry(40.0).lead_ms(WINDOW_MS));
    assert!(lead_ms > 0);
    config.live_lane.depth_time_averaging = false;
    assert_eq!(zoom(&config).tape_lead_ms(WINDOW_MS), 0);
}
