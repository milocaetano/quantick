//! Where a mark from another pane lands on this one, and when it says it
//! could not be placed exactly.

use super::*;
use crate::DRAWING_TOOLS;
use crate::gesture::PaneSeriesRead;
use quantick_chart::indicator_views::IndicatorViews;
use quantick_chart::state::{BarSpec, ChartState, SpecSelector};
use quantick_chart_interaction::viewport::Viewport;

/// Two prints to a bar, a gap of 200 ms between bars: bar 0 is 1000 … 1100,
/// bar 1 is 1300 … 1400, bar 2 is 1600 … 1700.
const PRINTS_MS: [i64; 6] = [1_000, 1_100, 1_300, 1_400, 1_600, 1_700];

fn tape(prints: &[i64]) -> ChartState {
    let mut state = ChartState::new(BarSpec::Tick(2));
    for (agg_id, timestamp_ms) in (0..).zip(prints) {
        state.ingest_live(&quantick_engine::Trade {
            agg_id,
            timestamp_ms: *timestamp_ms,
            price: 50.into(),
            quantity: 1.into(),
            side: quantick_engine::Side::Buy,
        });
    }
    state
}

/// A shared rectangle from `from_ms` to `to_ms`, as another pane placed it.
fn rectangle(from_ms: i64, to_ms: i64) -> Mark {
    let rectangle = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "rectangle")
        .expect("the rectangle is registered");
    let mut drawings = Marks::default();
    drawings.place(rectangle, ChartPoint::at_time(0.5, 40.0, Some(from_ms)));
    drawings.place(rectangle, ChartPoint::at_time(1.5, 60.0, Some(to_ms)));
    drawings.items()[0].clone()
}

fn reprojected(state: &ChartState, drawing: &Mark) -> (Vec<f32>, bool) {
    let spec = SpecSelector::new(BarSpec::Tick(2));
    let viewport = Viewport::new();
    let indicators = IndicatorViews::new();
    let projection = DrawingProjection {
        series: PaneSeriesRead {
            history_prefix: &[],
            state,
            spec: &spec,
        },
        viewport: &viewport,
        indicators: &indicators,
    };
    let (anchors, clamped) = projection.reproject(drawing).expect("the series has bars");
    (anchors.iter().map(|anchor| anchor.bar).collect(), clamped)
}

/// The trader's report: a rectangle shown on all charts arrived on the
/// other chart as faded borders with no fill. Its corner fell in the quiet
/// between two of that chart's bars — after one bar's last print, before
/// the next one opened — which is inside the series, not off its end.
#[test]
fn an_instant_between_two_bars_is_on_the_series() {
    let state = tape(&PRINTS_MS);
    let (bars, clamped) = reprojected(&state, &rectangle(1_200, 1_650));

    assert!(
        !clamped,
        "a gap between two bars is not the end of the tape"
    );
    assert_eq!(
        bars,
        vec![0.5, 2.5],
        "each corner sits on the bar before it"
    );
}

/// The fade the gap fix must keep: an instant after the newest bar closed
/// really is past what this chart has seen.
#[test]
fn an_instant_past_the_newest_closed_bar_still_fades() {
    let state = tape(&PRINTS_MS[..4]);
    assert!(state.partial().is_none(), "the newest bar has closed");

    let (_, clamped) = reprojected(&state, &rectangle(1_000, 5_000));

    assert!(clamped);
}

/// A magnetic mark grabbed by its body on a mirror pane is dragged by its
/// one anchor, so it lands on the bar's extreme under the hand as on its own
/// pane; a free mark, or any other body, still translates.
#[test]
fn a_magnetic_mark_body_drag_on_a_mirror_moves_its_anchor() {
    let pick = |snap| SharedPick {
        owner: 1,
        index: 3,
        anchor: None,
        locked: false,
        magnet: false,
        snap,
    };
    for snap in [AnchorSnap::BarLow, AnchorSnap::BarHigh] {
        assert_eq!(
            shared_drag_of(&pick(snap), true),
            SharedDrag::Anchor {
                index: 3,
                anchor: 0
            }
        );
        assert_eq!(
            shared_drag_of(&pick(snap), false),
            SharedDrag::Body { index: 3 }
        );
    }
    assert_eq!(
        shared_drag_of(&pick(AnchorSnap::Pointer), true),
        SharedDrag::Body { index: 3 }
    );
    let handle = SharedPick {
        anchor: Some(1),
        ..pick(AnchorSnap::Pointer)
    };
    assert_eq!(
        shared_drag_of(&handle, false),
        SharedDrag::Anchor {
            index: 3,
            anchor: 1
        }
    );
}

/// A one-trade-per-bar tape, `count` prints a second apart from 1 000 s:
/// a market instant and a slot are the same statement on it.
fn seconds(count: i64) -> ChartState {
    let mut state = ChartState::new(BarSpec::Tick(1));
    for print in 0..count {
        state.ingest_live(&quantick_engine::Trade {
            agg_id: print.unsigned_abs(),
            timestamp_ms: 1_000_000 + print * 1_000,
            price: 50.into(),
            quantity: 1.into(),
            side: quantick_engine::Side::Buy,
        });
    }
    state
}

/// A horizontal line on bar 4 of [`seconds`], at 100.
fn line_on_bar_four() -> Marks {
    let line = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .expect("the horizontal line is registered");
    let mut drawings = Marks::default();
    drawings.place(line, ChartPoint::at_time(4.5, 100.0, Some(1_004_000)));
    drawings
}

/// Apply `edit`, made on another pane, to `drawings` on the [`seconds`] tape.
fn apply(drawings: &mut Marks, edit: SharedEdit) {
    let state = seconds(20);
    let spec = SpecSelector::new(BarSpec::Tick(1));
    let viewport = Viewport::new();
    let indicators = IndicatorViews::new();
    SharedMarksMut {
        projection: DrawingProjection {
            series: PaneSeriesRead {
                history_prefix: &[],
                state: &state,
                spec: &spec,
            },
            viewport: &viewport,
            indicators: &indicators,
        },
        drawings,
    }
    .apply_edit(edit);
}

/// The other chart says where it put the handle in market time; this pane
/// is the one that knows which of *its* bars that is.
#[test]
fn an_anchor_moved_on_another_pane_lands_on_this_panes_own_bar() {
    let mut drawings = line_on_bar_four();
    apply(
        &mut drawings,
        SharedEdit::MoveAnchor {
            index: 0,
            anchor: 0,
            time_ms: 1_011_000,
            price: 101.5,
        },
    );
    let point = drawings.items()[0].points[0];
    assert_eq!(point.time_ms, Some(1_011_000));
    assert_eq!(point.bar, 11.5, "resolved into this pane's bar 11");
    assert!((point.price - 101.5).abs() < f64::EPSILON);
}

/// A body drag is said in market time: three seconds is three bars on a
/// one-trade-per-bar cut.
#[test]
fn a_body_drag_on_another_pane_moves_every_anchor_by_the_same_time() {
    let mut drawings = line_on_bar_four();
    apply(
        &mut drawings,
        SharedEdit::Translate {
            index: 0,
            delta_ms: 3_000,
            delta_price: 2.0,
        },
    );
    let point = drawings.items()[0].points[0];
    assert_eq!(point.time_ms, Some(1_007_000));
    assert_eq!(point.bar, 7.5);
    assert!((point.price - 102.0).abs() < f64::EPSILON);
}

/// A lock protects the geometry from both charts, and a drag back past the
/// first bar this pane holds moves nothing at all rather than half a shape.
#[test]
fn a_locked_mark_or_an_unplaceable_drag_moves_nothing() {
    let mut locked = line_on_bar_four();
    locked.items_mut()[0].locked = true;
    let before = locked.items()[0].points.clone();
    apply(
        &mut locked,
        SharedEdit::Translate {
            index: 0,
            delta_ms: 3_000,
            delta_price: 2.0,
        },
    );
    apply(
        &mut locked,
        SharedEdit::MoveAnchor {
            index: 0,
            anchor: 0,
            time_ms: 1_011_000,
            price: 101.5,
        },
    );
    assert_eq!(locked.items()[0].points, before, "the lock holds");

    let mut free = line_on_bar_four();
    apply(
        &mut free,
        SharedEdit::Translate {
            index: 0,
            delta_ms: -600_000,
            delta_price: 0.0,
        },
    );
    assert_eq!(free.items()[0].points, before, "no slot, no move");
}
