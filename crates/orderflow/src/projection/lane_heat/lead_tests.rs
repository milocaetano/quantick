//! The native tape's averaged book fills the lane edge to edge: older depth
//! in the lead left of the oldest dot, the live current book past NOW, both
//! on the clock the dots use, and neither moving the lane's own bands.

use super::*;
use crate::projection::{
    CellHitContext, DotRungMemory, FlowCellHit, MAX_LEAD_SHARE, PaneGeometry,
    TapeHorizontalGeometry,
};
use crate::{DisplayGrouping, HeatmapConfig, LiveEdge};
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

/// An averaged native tape whose largest bubble is `max_radius`.
fn config(max_radius: f32, max_visible_cells: usize) -> HeatmapConfig {
    let mut config = HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        max_visible_cells,
        ..HeatmapConfig::default()
    };
    config.live_lane.enabled = true;
    config.live_lane.tape_only = true;
    config.live_lane.depth_time_averaging = true;
    config.bubbles.max_radius = max_radius;
    config
}

/// A synchronized book captured from `from_ms`, confirming at 1,900 so it
/// is still the current book at NOW.
fn history_from(from_ms: i64, config: HeatmapConfig) -> LiquidityHistory {
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
        history
            .apply_delta(
                time,
                &BookDelta::new(id, id, vec![level(95, quantity)], vec![]),
            )
            .unwrap();
    }
    history
}

/// The production path: the view's rungs ask for the lead, the engine reads
/// it on its own window.
fn lead_ms(config: &HeatmapConfig) -> i64 {
    DotRungMemory::default()
        .choose(
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
        .tape_lead_ms(WINDOW_MS)
}

/// The geometry the painter draws the tape on.
fn geometry(config: &HeatmapConfig) -> TapeHorizontalGeometry {
    TapeHorizontalGeometry::from_config(config, LANE_WIDTH_PX, LANE_HEIGHT_PX).unwrap()
}

fn timeline(lead_ms: i64) -> BarTimeline {
    let bar = quantick_engine::Bar {
        open_time: 0,
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

fn project(history: &LiquidityHistory, timeline: &BarTimeline) -> Vec<HeatmapCell> {
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

/// The tape's book for `history`, its lead read through the production path.
fn lane(history: &LiquidityHistory) -> (Vec<HeatmapCell>, BarTimeline) {
    let timeline = timeline(lead_ms(history.config()));
    (project(history, &timeline), timeline)
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
    for max_radius in [4.0, 40.0] {
        let history = history_from(0, config(max_radius, 1_000));
        let geometry = geometry(history.config());
        assert!(
            geometry.inset_px > 0.0,
            "radius {max_radius} insets the dots"
        );
        let (cells, timeline) = lane(&history);
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
    let history = history_from(0, config(40.0, 1_000));
    let geometry = geometry(history.config());
    let (cells, timeline) = lane(&history);
    let regions = timeline.region_count();
    let spans = painted(geometry, &cells, regions, true);
    let change_x = geometry.x((LEAD_CHANGE_MS - LANE_START_MS) as f64 / WINDOW_MS as f64);
    assert!(change_x > 0.0 && change_x < geometry.inset_px);
    let wall = |quantity: i64, lead: bool| {
        cells
            .iter()
            .zip(&spans)
            .find(|(cell, _)| {
                cell.price_bucket == Decimal::from(95)
                    && cell.quantity == Decimal::from(quantity)
                    && cell.lead_span.is_some() == lead
            })
            .map(|(_, &span)| span)
            .unwrap()
    };
    let (from, until) = wall(100, true);
    assert!(from.abs() < 1e-3 && (until - change_x).abs() < 1e-3);
    let (from, until) = wall(300, true);
    assert!(
        (from - change_x).abs() < 1e-3,
        "it starts where it changed: {from}"
    );
    assert!(
        (until - geometry.inset_px).abs() < 1e-3,
        "and meets the lane's own band"
    );
    assert!((wall(300, false).0 - geometry.inset_px).abs() < 1e-3);
    let opening = (regions - 1) as f64 / regions as f64;
    for cell in cells.iter().filter(|cell| cell.lead_span.is_some()) {
        assert_eq!((cell.x0, cell.x1), (opening, opening), "{cell:?}");
    }
}

#[test]
fn the_lead_stays_empty_where_no_history_was_captured() {
    let history = history_from(LANE_START_MS, config(40.0, 1_000));
    let geometry = geometry(history.config());
    let (cells, timeline) = lane(&history);
    assert!(cells.iter().all(|cell| cell.lead_span.is_none()));
    let (left, _) = reach(&painted(geometry, &cells, timeline.region_count(), true));
    assert!(
        (left - geometry.inset_px).abs() < 1e-3,
        "nothing is invented before the capture: {left}"
    );
}

#[test]
fn a_lane_without_averaging_takes_no_lead() {
    let mut config = config(40.0, 1_000);
    let lead = lead_ms(&config);
    assert!(lead > 0);
    config.live_lane.depth_time_averaging = false;
    assert_eq!(lead_ms(&config), 0, "the view asks for none");
    let history = history_from(0, config);
    let cells = project(&history, &timeline(lead));
    assert!(cells.iter().all(|cell| cell.lead_span.is_none()));
}

#[test]
fn the_lead_is_the_painters_inset_and_never_more_than_its_cap() {
    let config = config(40.0, 1_000);
    let share = geometry(&config).lead_share();
    assert!(share > 0.0 && share <= MAX_LEAD_SHARE);
    assert_eq!(lead_ms(&config), (share * WINDOW_MS as f64).ceil() as i64);
    // A pane one diameter wide keeps the capped lead instead of dropping it.
    let bubbles = config.bubbles.clone();
    for width in [80.0, 81.0, 90.0, 120.0] {
        let tape = TapeHorizontalGeometry::resolve(width, LANE_HEIGHT_PX, &bubbles);
        assert!(
            tape.lead_share() <= MAX_LEAD_SHARE && tape.lead_share() > 0.0,
            "width {width}: {}",
            tape.lead_share()
        );
    }
    let closed = TapeHorizontalGeometry::resolve(80.0, LANE_HEIGHT_PX, &bubbles);
    assert_eq!(closed.span_px, 0.0);
    assert_eq!(closed.lead_share(), MAX_LEAD_SHARE);
}

/// A busy book: every level changes every 37 ms from the start, enough for
/// a small cap to average the lane.
fn busy_history(config: HeatmapConfig) -> LiquidityHistory {
    let mut history = LiquidityHistory::new(config);
    let levels = |step: i64| {
        (95..=99)
            .map(|price| level(price, 10 + (price * 7 + step * 13).rem_euclid(90)))
            .collect::<Vec<_>>()
    };
    history
        .install_snapshot(
            0,
            1,
            BookSnapshot::new(10, levels(0), vec![level(101, 5)], BookCoverage::Full),
        )
        .unwrap();
    for step in 1..=51_i64 {
        let id = 10 + step as u64;
        history
            .apply_delta(step * 37, &BookDelta::new(id, id, levels(step), vec![]))
            .unwrap();
    }
    history
}

#[test]
fn the_bubble_size_never_moves_the_lanes_own_bands() {
    let split = |max_radius: f32| {
        let (cells, _) = lane(&busy_history(config(max_radius, 24)));
        assert!(cells.len() <= 24, "one cap for both");
        cells
            .into_iter()
            .partition::<Vec<_>, _>(|cell| cell.lead_span.is_none())
    };
    let (small, small_lead) = split(4.0);
    let (large, large_lead) = split(40.0);
    assert!(
        small.iter().any(|cell| cell.time_averaged),
        "the cap averages"
    );
    assert!(large_lead.len() >= small_lead.len());
    assert_eq!(
        small, large,
        "the oldest-to-NOW book is the same at any radius"
    );
    let unpadded = project(&busy_history(config(4.0, 24)), &timeline(0));
    assert_eq!(unpadded, small, "and the same as with no lead at all");
}

#[test]
fn a_gap_inside_the_lead_is_marked_there() {
    let mut history = LiquidityHistory::new(config(40.0, 1_000));
    let snapshot = || {
        BookSnapshot::new(
            10,
            vec![level(95, 100)],
            vec![level(101, 1)],
            BookCoverage::Full,
        )
    };
    history.install_snapshot(0, 1, snapshot()).unwrap();
    history.mark_gap(1_450, "transport_closed").unwrap();
    history.install_snapshot(1_470, 2, snapshot()).unwrap();
    let timeline = timeline(lead_ms(history.config()));
    assert!(LANE_START_MS - timeline.lane_lead_ms() < 1_450);
    let gaps = lead_gaps(&history, &timeline);
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!((gaps[0].x0, gaps[0].x1), (0.5, 0.5), "at the opening");
    assert_eq!(gaps[0].lead_span, Some((-0.1, -0.06)));
    // Before the capture began, the lead says so rather than reading empty.
    let late = history_from(1_450, config(40.0, 1_000));
    let gaps = lead_gaps(&late, &timeline);
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert!(gaps[0].precedes_capture());
    assert_eq!(gaps[0].lead_span.unwrap().1, -0.1);
}

#[test]
fn a_lead_band_reports_on_the_lane() {
    let history = history_from(0, config(40.0, 1_000));
    let (cells, timeline) = lane(&history);
    let cell = cells.iter().find(|cell| cell.lead_span.is_some()).unwrap();
    let hit = FlowCellHit::from_cell(
        cell,
        Decimal::ONE,
        CellHitContext {
            first_bar_index: 7,
            slot_count: timeline.region_count(),
            has_lane: true,
            held_tape_end_ms: None,
            held: false,
            current_book_preview: false,
            current_book_source: None,
        },
    )
    .unwrap();
    assert!(hit.live_lane, "the tape's band, never a candle's");
    assert_eq!((hit.start_slot, hit.end_slot_exclusive), (8, 8));
}
