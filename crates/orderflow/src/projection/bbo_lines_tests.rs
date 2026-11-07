//! The best bid and best ask as stepped lines on the tape: golden step points
//! from a fixture book, broken at every gap, carried dimmed to the live edge.

use super::*;
use crate::history::bbo_fixture;
use crate::projection::{HeatmapProjection, project};
use crate::timeline::BarTimeline;
use crate::{LiquidityHistory, LiveEdge};
use rust_decimal::Decimal;

fn line(side: RestingSide, carried: bool, points: &[(i64, i64)]) -> BboLine {
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
