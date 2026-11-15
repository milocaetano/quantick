//! Which band a mark belongs to: a value mark to its own, a time-only mark
//! to every band as one object, a parked mark to none, and a pick never
//! crosses from one band to another.

use super::*;
use crate::gesture::{DrawingProjection, PaneSeriesRead};
use crate::{ChartPoint, DRAWING_TOOLS};
use quantick_chart::state::{BarSpec, ChartState, SpecSelector};
use quantick_chart_interaction::viewport::Viewport;

type Marks = crate::Drawings<crate::DrawingTool>;

fn tool(id: &str) -> crate::DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .unwrap_or_else(|| panic!("the {id} tool is registered"))
}

fn cvd() -> DrawingBand {
    DrawingBand::Indicator(PaneKey {
        kind: "native.cvd".into(),
        ordinal: 0,
    })
}

/// The price band on top and a CVD band under it, both over -10 … 10, so one
/// value sits at the same height inside either band.
fn price_and_cvd() -> [Band; 2] {
    let band = |key: DrawingBand, top: f32| Band {
        key,
        rect: PxRect::from_min_max(crate::pos2(0.0, top), crate::pos2(400.0, top + 200.0)),
        scale: Some(PriceScale::from_range(-10.0, 10.0, top, top + 200.0)),
        label: "band".into(),
        refusal: None,
    };
    [band(DrawingBand::Price, 0.0), band(cvd(), 200.0)]
}

/// A vertical line marks an instant: placed while pointing at the CVD band,
/// it is still one object, owned by no band and painted through all of them.
#[test]
fn a_time_only_mark_is_one_object_every_band_paints() {
    let bands = price_and_cvd();
    let mut marks = Marks::default();
    marks.place_on(tool("vertical-line"), &cvd(), ChartPoint::at(1.0, 0.0));
    assert_eq!(marks.items().len(), 1, "one object, not one per band");
    assert_eq!(marks.items()[0].band, DrawingBand::AllBands);
    for band in &bands {
        assert!(drawing_in_band(&marks.items()[0], band));
    }
}

/// A mark whose indicator is off the chart is parked: kept, but no band on
/// screen owns it, and it never falls back onto the price band.
#[test]
fn a_parked_mark_belongs_to_no_band_on_screen() {
    let [price, _] = price_and_cvd();
    let mut marks = Marks::default();
    marks.place_on(tool("horizontal-line"), &cvd(), ChartPoint::at(1.0, 0.0));
    let carved = [price];
    assert!(band_of(&carved, &marks.items()[0]).is_none());
    assert!(!drawing_in_band(&marks.items()[0], &carved[0]));
}

/// A CVD level and a price level can be one pixel apart and mean unrelated
/// things. The level below is asked for at the very pixel it would paint at
/// on the price band, so only the band rule can rule it out there.
#[test]
fn a_pick_never_crosses_bands() {
    let [price, cvd_band] = price_and_cvd();
    let mut marks = Marks::default();
    marks.place_on(tool("horizontal-line"), &cvd(), ChartPoint::at(1.0, 0.0));
    let state = ChartState::new(BarSpec::Tick(10));
    let spec = SpecSelector::new(BarSpec::Tick(10));
    let (viewport, indicators) = (Viewport::new(), IndicatorViews::new());
    let projection = DrawingProjection {
        series: PaneSeriesRead {
            history_prefix: &[],
            state: &state,
            spec: &spec,
        },
        viewport: &viewport,
        indicators: &indicators,
    };
    let at_zero = |band: &Band| crate::pos2(200.0, band.scale.as_ref().unwrap().y(0.0));
    assert_eq!(
        projection.drawing_at(&marks, at_zero(&cvd_band), &cvd_band, 400.0, 3),
        Some(0),
        "found on its own band"
    );
    assert_eq!(
        projection.drawing_at(&marks, at_zero(&price), &price, 400.0, 3),
        None,
        "a CVD level is not a price level"
    );
}
