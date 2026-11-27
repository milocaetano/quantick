//! A change of bar space reaches a pane's marks and the body drag holding a
//! copy of their anchors together.

use super::*;
use crate::DRAWING_TOOLS;
use quantick_chart::state::{BarSpec, ChartState, SpecSelector};

/// One print a second, from print `from` up to but not including `to`.
fn tape(spec: BarSpec, from: i64, to: i64) -> ChartState {
    let mut state = ChartState::new(spec);
    for print in from..to {
        state.ingest_live(&quantick_engine::Trade {
            agg_id: print.unsigned_abs(),
            timestamp_ms: print * 1_000,
            price: 50.into(),
            quantity: 1.into(),
            side: quantick_engine::Side::Buy,
        });
    }
    state
}

/// A horizontal line at `point`, and a body drag in flight on it.
fn grabbed_line(point: ChartPoint) -> (Marks, PaneGestures) {
    let line = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .expect("the horizontal line is registered");
    let mut drawings = Marks::default();
    drawings.place(line, point);
    let gestures = PaneGestures {
        translate_from: Some(TranslateFrom {
            index: 0,
            last: Px { x: 0.0, y: 0.0 },
            free: vec![point],
        }),
        ..PaneGestures::default()
    };
    (drawings, gestures)
}

fn marks<'a>(
    drawings: &'a mut Marks,
    gestures: &'a mut PaneGestures,
    state: &'a ChartState,
    spec: &'a SpecSelector,
) -> PaneMarksMut<'a> {
    PaneMarksMut {
        drawings,
        gestures,
        series: PaneSeriesRead {
            history_prefix: &[],
            state,
            spec,
        },
    }
}

fn dragged_bar(gestures: &PaneGestures) -> f32 {
    gestures
        .translate_from
        .as_ref()
        .expect("still grabbed")
        .free[0]
        .bar
}

/// A re-cut to twice the bar size puts print 12 on bar 6, for the store and
/// for the anchors the hand is moving alike.
#[test]
fn a_recut_moves_a_body_drag_with_the_store() {
    let spec = SpecSelector::new(BarSpec::Tick(2));
    let state = tape(BarSpec::Tick(2), 0, 20);
    let (mut drawings, mut gestures) = grabbed_line(ChartPoint::at_time(12.5, 50.0, Some(12_000)));

    marks(&mut drawings, &mut gestures, &state, &spec).reanchor(20);

    assert_eq!(drawings.items()[0].points[0].bar, 6.5);
    assert_eq!(drawings.items()[0].points[0].time_ms, Some(12_000));
    assert!(!drawings.items()[0].off_series, "still on its own instant");
    assert_eq!(dragged_bar(&gestures), 6.5);
}

/// Ten older bars arriving move the body drag by ten, and put a mark the
/// shorter tape could not reach back on its own bar.
#[test]
fn older_bars_shift_the_drag_and_resettle_an_off_series_mark() {
    let spec = SpecSelector::new(BarSpec::Tick(1));
    let short = tape(BarSpec::Tick(1), 10, 20);
    let (mut drawings, mut gestures) = grabbed_line(ChartPoint::at_time(0.5, 50.0, Some(5_000)));
    marks(&mut drawings, &mut gestures, &short, &spec).reanchor(10);
    assert!(
        drawings.items()[0].off_series,
        "the short tape misses print 5"
    );
    let before = dragged_bar(&gestures);

    let long = tape(BarSpec::Tick(1), 0, 20);
    marks(&mut drawings, &mut gestures, &long, &spec).shift_bars(10);

    assert!(!drawings.items()[0].off_series);
    assert_eq!(drawings.items()[0].points[0].bar, 5.5);
    assert_eq!(dragged_bar(&gestures), before + 10.0);
}

/// A projected anchor past the newest bar of a tick chart, re-cut: it keeps
/// its two bars past the end, the drag's copy too, whatever the new cut's
/// pace would make of its instant — never parked on the newest bar. Once
/// the tape has reached its time, or on a time chart's clock, it is put on
/// its instant.
#[test]
fn a_recut_keeps_a_projected_anchor_past_the_end_of_a_tick_chart() {
    let projected = |time_ms| ChartPoint {
        time_projected: true,
        ..ChartPoint::at_time(21.5, 50.0, Some(time_ms))
    };
    for (spec, time_ms, what) in [
        (BarSpec::Tick(2), 19_500, "inside the newest bar's step"),
        (BarSpec::Tick(2), 21_000, "a step past it at the new pace"),
        (BarSpec::Tick(2), 13_000, "a time the tape has reached"),
        (BarSpec::Time(2_000), 21_000, "a time chart's clock"),
    ] {
        let state = tape(spec, 0, 20);
        let spec = SpecSelector::new(spec);
        let (mut drawings, mut gestures) = grabbed_line(projected(time_ms));
        let series = PaneSeriesRead {
            history_prefix: &[],
            state: &state,
            spec: &spec,
        };
        let bar = if time_ms <= series.projected_dated_until() {
            series.slot_of_time(time_ms).expect("the series reaches it")
        } else {
            11.5
        };

        marks(&mut drawings, &mut gestures, &state, &spec).reanchor(20);

        assert_eq!(drawings.items()[0].points[0].bar, bar, "{what}");
        assert_eq!(dragged_bar(&gestures), bar, "{what}");
        assert_eq!(drawings.items()[0].points[0].time_ms, Some(time_ms));
    }
}
