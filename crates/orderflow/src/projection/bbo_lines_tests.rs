//! The best bid and best ask as stepped lines on the tape: golden step points
//! from a fixture book, broken at every gap, carried dimmed to the live edge.

use super::*;
use crate::history::bbo_fixture;
use crate::projection::{HeatmapProjection, project};
use crate::timeline::BarTimeline;
use crate::{LiquidityHistory, LiveEdge};
use rust_decimal::Decimal;

fn line(side: RestingSide, carried: bool, points: &[(i64, i64)]) -> BboLine<StepPoint> {
    BboLine {
        side,
        carried,
        points: points
            .iter()
            .map(|&(time, price)| (time, Decimal::from(price)))
            .collect(),
    }
}

#[test]
fn the_top_of_the_book_steps_and_breaks_at_the_gap() {
    let history = bbo_fixture();
    let lines = bbo_lines(history.bbo(), 1_000, 1_700, Some(1_800));
    assert_eq!(
        lines,
        vec![
            line(
                RestingSide::Bid,
                false,
                &[(1_000, 950), (1_300, 950), (1_300, 955), (1_400, 955)]
            ),
            line(RestingSide::Bid, false, &[(1_600, 940), (1_700, 940)]),
            line(RestingSide::Bid, true, &[(1_700, 940), (1_800, 940)]),
            line(
                RestingSide::Ask,
                false,
                &[(1_000, 960), (1_200, 960), (1_200, 970), (1_400, 970)]
            ),
            line(RestingSide::Ask, false, &[(1_600, 950), (1_700, 950)]),
            line(RestingSide::Ask, true, &[(1_700, 950), (1_800, 950)]),
        ],
        "nothing is drawn across the gap from 1,400 to 1,600"
    );
}

#[test]
fn a_window_opening_mid_step_starts_the_line_at_its_edge() {
    let history = bbo_fixture();
    let lines = bbo_lines(history.bbo(), 1_250, 1_350, None);
    assert_eq!(
        lines,
        vec![
            line(
                RestingSide::Bid,
                false,
                &[(1_250, 950), (1_300, 950), (1_300, 955), (1_350, 955)]
            ),
            line(RestingSide::Ask, false, &[(1_250, 970), (1_350, 970)]),
        ],
        "no carry asked for, none drawn"
    );
}

#[test]
fn a_side_with_no_book_draws_no_line_and_a_bookless_tail_carries_nothing() {
    let history = bbo_fixture();
    assert!(
        bbo_lines(history.bbo(), 1_450, 1_550, Some(1_590)).is_empty(),
        "inside the gap there is no top to draw, observed or carried"
    );
    assert!(bbo_lines(history.bbo(), 1_700, 1_700, None).is_empty());
}

fn timeline(now_ms: i64) -> BarTimeline {
    let bar = quantick_engine::Bar {
        open_time: 1_000,
        close_time: 2_000,
        open: 950.into(),
        high: 960.into(),
        low: 940.into(),
        close: 950.into(),
        buy_volume: Decimal::ONE,
        sell_volume: Decimal::ONE,
        trade_count: 2,
    };
    BarTimeline::from_bars(
        0,
        &[bar],
        None,
        Some(LiveEdge {
            now_ms,
            window_ms: 1_000,
            reference_ms: 50,
            on_newest_bar: true,
        }),
    )
    .with_full_lane_coverage()
}

fn prices() -> PriceWindow {
    PriceWindow::new(930.into(), 980.into()).unwrap()
}

#[test]
fn the_lane_places_the_steps_on_its_own_clock_and_dims_the_carried_tail() {
    let history = bbo_fixture();
    let timeline = timeline(1_800);
    let (lane_start, lane_end) = timeline.lane_bounds_ms().unwrap();
    let carry = book_carry(&history, lane_end).expect("the book confirmed 100 ms ago");
    let expected: Vec<BboPath> = bbo_lines(
        history.bbo(),
        lane_start,
        carry.from_ms,
        Some(carry.until_ms),
    )
    .into_iter()
    .map(|line| BboPath {
        side: line.side,
        carried: line.carried,
        points: line
            .points
            .iter()
            .map(|&(time, price)| {
                [
                    timeline.locate_in_lane_clamped(time).unwrap().normalized,
                    prices().y_unclamped(price).unwrap(),
                ]
            })
            .collect(),
    })
    .collect();
    let paths = project_bbo_paths(&history, &timeline, prices());
    assert_eq!(paths, expected);
    assert_eq!(paths.iter().filter(|path| path.carried).count(), 2);
    assert!(
        paths.iter().all(|path| path.points.len() >= 2),
        "every path is a line, never a lone point"
    );
}

#[test]
fn the_switch_off_projects_no_line() {
    let mut history = bbo_fixture();
    let mut config = history.config().clone();
    config.live_lane.bbo_lines = false;
    history.update_config(config).unwrap();
    assert!(project_bbo_paths(&history, &timeline(1_800), prices()).is_empty());
}

/// The approved WIN tape draws exactly what it drew before the lines: the
/// switch adds one field to the frame and moves nothing else in it.
#[test]
fn the_lines_are_purely_additive_to_the_frame() {
    let frame = |on: bool| {
        let mut history: LiquidityHistory = bbo_fixture();
        let mut config = history.config().clone();
        config.live_lane.bbo_lines = on;
        history.update_config(config).unwrap();
        project(&history, &timeline(1_800), prices())
    };
    let (on, off) = (frame(true), frame(false));
    assert!(!on.bbo_lines.is_empty(), "on, the lines are projected");
    assert!(off.bbo_lines.is_empty(), "off, none are");
    assert_eq!(
        HeatmapProjection {
            bbo_lines: off.bbo_lines.clone(),
            ..on
        },
        off,
        "cells, marks, gaps and every count are untouched by the switch"
    );
}

#[test]
fn a_carried_line_is_as_faint_as_a_carried_band() {
    let path = |carried| BboPath {
        side: RestingSide::Ask,
        carried,
        points: Vec::new(),
    };
    assert_eq!(path(false).alpha(), 1.0);
    assert_eq!(path(true).alpha(), CARRIED_BOOK_ALPHA);
}

fn level(price: i64) -> quantick_orderbook::BookLevel {
    quantick_orderbook::BookLevel::new(price.into(), Decimal::ONE).unwrap()
}

fn snapshot(id: u64, bid: i64, ask: i64) -> quantick_orderbook::BookSnapshot {
    quantick_orderbook::BookSnapshot::new(
        id,
        vec![level(bid)],
        vec![level(ask), level(ask + 10)],
        quantick_orderbook::BookCoverage::Full,
    )
}

/// Bid 950 / ask 960 from 1,000; the book's last event, at 1,700, pulls
/// the ask to 970.
fn ends_on_a_move() -> LiquidityHistory {
    let mut history = LiquidityHistory::new(crate::HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        ..crate::HeatmapConfig::default()
    });
    history
        .install_snapshot(1_000, 1, snapshot(10, 950, 960))
        .unwrap();
    let pull = quantick_orderbook::BookLevel::new(960.into(), Decimal::ZERO).unwrap();
    history
        .apply_delta(
            1_700,
            &quantick_orderbook::BookDelta::new(11, 11, vec![], vec![pull]),
        )
        .unwrap();
    history
}

#[test]
fn a_top_moved_by_the_last_event_is_the_top_carried() {
    let history = ends_on_a_move();
    assert_eq!(
        bbo_lines(history.bbo(), 1_000, 1_700, Some(1_800)),
        vec![
            line(RestingSide::Bid, false, &[(1_000, 950), (1_700, 950)]),
            line(RestingSide::Bid, true, &[(1_700, 950), (1_800, 950)]),
            line(RestingSide::Ask, false, &[(1_000, 960), (1_700, 960)]),
            line(RestingSide::Ask, true, &[(1_700, 970), (1_800, 970)]),
        ],
        "the ask pulled at the last confirmation is carried at 970, not 960"
    );
    let carried_ask = project_bbo_paths(&history, &timeline(1_800), prices())
        .into_iter()
        .find(|path| path.carried && path.side == RestingSide::Ask)
        .expect("the carried ask");
    let y = prices().y_unclamped(970.into()).unwrap();
    assert!(carried_ask.points.iter().all(|point| point[1] == y));
}

#[test]
fn a_resync_snapshot_as_the_last_event_is_carried_at_once() {
    let mut history = ends_on_a_move();
    history.mark_gap(1_750, "transport_closed").unwrap();
    history
        .install_snapshot(1_760, 2, snapshot(20, 940, 945))
        .unwrap();
    let lines = bbo_lines(history.bbo(), 1_000, 1_760, Some(1_800));
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.carried)
            .cloned()
            .collect::<Vec<_>>(),
        vec![
            line(RestingSide::Bid, true, &[(1_760, 940), (1_800, 940)]),
            line(RestingSide::Ask, true, &[(1_760, 945), (1_800, 945)]),
        ],
        "no delta needed after the snapshot"
    );
    assert!(
        lines
            .iter()
            .filter(|line| !line.carried)
            .all(|line| line.points.last().unwrap().0 <= 1_750),
        "nothing observed is drawn across the gap"
    );
}

#[test]
fn a_clipped_line_ends_its_crossing_legs_on_the_window() {
    let step = line(
        RestingSide::Bid,
        false,
        &[(1_000, 950), (1_300, 950), (1_300, 955), (1_400, 955)],
    );
    assert_eq!(
        step.clipped(1_100, 1_350),
        Some(line(
            RestingSide::Bid,
            false,
            &[(1_100, 950), (1_300, 950), (1_300, 955), (1_350, 955)]
        ))
    );
    assert_eq!(step.clipped(1_500, 1_600), None);
    assert_eq!(
        step.clipped(1_400, 1_600),
        None,
        "a single instant draws nothing"
    );
}

#[test]
fn a_crowded_pixel_column_keeps_its_ends_and_extremes() {
    let mut points = vec![[9.5, 50.0]];
    for index in 0..1_000 {
        let y = 40.0 + (index % 7) as f32;
        points.push([10.0 + index as f32 / 2_000.0, y]);
    }
    points.push([10.9, 44.0]);
    points.push([11.2, 44.0]);
    let kept = decimate_columns(points);
    let ys: Vec<f32> = kept.iter().map(|point| point[1]).collect();
    assert_eq!(ys, vec![50.0, 40.0, 46.0, 44.0, 44.0], "{kept:?}");
    assert_eq!(
        kept[1],
        [10.0, 40.0],
        "the column's first vertex is its lowest"
    );
    assert!(
        kept.windows(2).all(|pair| pair[0][0] <= pair[1][0]),
        "order kept"
    );
    let sparse = vec![[1.0, 1.0], [2.0, 1.0], [2.0, 3.0], [3.0, 3.0]];
    assert_eq!(
        decimate_columns(sparse.clone()),
        sparse,
        "a sparse line is untouched"
    );
}

#[test]
fn a_held_tape_places_its_own_lines_on_its_clock() {
    let history = bbo_fixture();
    let grouping =
        crate::EffectiveGrouping::resolve(crate::DisplayGrouping::Native, Decimal::ONE, 50.into());
    let heat = super::super::project_past_heat(&history, 1_000, 1_700, prices(), grouping);
    assert_eq!(heat.bbo_lines.len(), 4, "two sides, broken at the gap");
    let placed = heat.placed(1_350, 300, 2);
    assert_eq!(
        placed.bbo_lines.len(),
        2,
        "the gap and later are off the window"
    );
    for path in placed.bbo_lines.iter() {
        assert!(!path.carried);
        assert!(
            path.points
                .iter()
                .all(|point| (0.5..=1.0).contains(&point[0]))
        );
    }
}

/// The lines are a layer of their own: with the tape's book and bubbles and
/// the candles' both off, they alone still make a projected frame.
#[test]
fn the_lines_alone_keep_the_frame_projected() {
    let mut history = bbo_fixture();
    let mut config = history.config().clone();
    config.show_depth = false;
    config.show_aggressions = false;
    config.live_lane.show_depth = false;
    config.live_lane.show_aggressions = false;
    history.update_config(config.clone()).unwrap();
    assert!(config.any_layer_enabled(), "the lines ask for a projection");
    let frame = project(&history, &timeline(1_800), prices());
    assert!(frame.enabled);
    assert!(!frame.bbo_lines.is_empty(), "the lines are drawn");
    assert!(frame.cells.is_empty() && frame.aggressions.is_empty());

    config.live_lane.bbo_lines = false;
    assert!(
        !config.any_layer_enabled(),
        "nothing left to draw, nothing projected"
    );
}

/// The `(top, bottom)` screen band a side's stroke covers at `true_y`.
fn band(side: RestingSide, true_y: f32, stroke: f32, high_price_up: bool) -> (f32, f32) {
    let centre = true_y + bbo_outward_offset_px(side, stroke, high_price_up);
    (centre - stroke / 2.0, centre + stroke / 2.0)
}

#[test]
fn the_two_sides_never_overlap_on_screen() {
    let stroke = 3.0;
    // Screen y grows downwards; the ask is the higher price, so its true y is
    // at or above the bid's.
    for (ask_y, bid_y) in [
        (100.0, 100.0),
        (100.0, 100.4),
        (100.0, 101.0),
        (100.0, 160.0),
    ] {
        let ask = band(RestingSide::Ask, ask_y, stroke, true);
        let bid = band(RestingSide::Bid, bid_y, stroke, true);
        assert!(ask.1 <= bid.0, "ask {ask:?} stays above bid {bid:?}");
        assert_eq!(ask.1, ask_y, "the ask never crosses its price inward");
        assert_eq!(bid.0, bid_y, "nor does the bid");
        assert_eq!(
            (ask.1 - ask.0, bid.1 - bid.0),
            (stroke, stroke),
            "both whole"
        );
    }
    // A wide spread is unchanged but for the outward half stroke.
    let (ask, bid) = (
        band(RestingSide::Ask, 100.0, stroke, true),
        band(RestingSide::Bid, 160.0, stroke, true),
    );
    assert_eq!((ask.0 + ask.1) / 2.0, 100.0 - stroke / 2.0);
    assert_eq!((bid.0 + bid.1) / 2.0, 160.0 + stroke / 2.0);
}

#[test]
fn an_inverted_axis_keeps_each_side_outward() {
    let stroke = 3.0;
    // Inverted, the ask is drawn below the bid.
    let ask = band(RestingSide::Ask, 100.3, stroke, false);
    let bid = band(RestingSide::Bid, 100.0, stroke, false);
    assert!(bid.1 <= ask.0, "bid {bid:?} above ask {ask:?}, apart");
    assert_eq!((bid.1, ask.0), (100.0, 100.3));
}

#[test]
fn every_halo_is_laid_before_any_line_and_each_line_sits_outward() {
    use crate::config::painting::{BBO_HALO_PX, BBO_LINE_WIDTH_PX};
    let path = |side, y| BboPath {
        side,
        carried: false,
        points: vec![[0.0, y], [1.0, y]],
    };
    let paths = [path(RestingSide::Ask, 0.5), path(RestingSide::Bid, 0.5)];
    let strokes = bbo_strokes(&paths, |[x, y]| [x as f32 * 100.0, y as f32 * 100.0], true);
    let sides: Vec<_> = strokes.iter().map(|stroke| stroke.side).collect();
    assert_eq!(
        sides,
        vec![None, None, Some(RestingSide::Ask), Some(RestingSide::Bid)]
    );
    assert_eq!(strokes[0].width, BBO_LINE_WIDTH_PX + 2.0 * BBO_HALO_PX);
    assert_eq!(strokes[2].width, BBO_LINE_WIDTH_PX);
    let half = BBO_LINE_WIDTH_PX / 2.0;
    assert_eq!(
        strokes[2].points[0][1],
        50.0 - half,
        "the ask above its price"
    );
    assert_eq!(
        strokes[3].points[0][1],
        50.0 + half,
        "the bid below its price"
    );
    assert!(
        strokes.iter().all(|stroke| stroke.alpha == 1.0),
        "full opacity"
    );
}
