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
