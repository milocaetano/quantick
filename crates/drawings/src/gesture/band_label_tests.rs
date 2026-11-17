//! The band an object lives on, as the chrome names it: nothing on the
//! candles, "all bands" for a time-only mark, and a parked mark named by the
//! indicator kind it waits for.

use super::*;
use crate::{ChartPoint, DRAWING_TOOLS, PaneKey};

type Marks = crate::Drawings<crate::DrawingTool>;

fn tool(id: &str) -> crate::DrawingTool {
    DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == id)
        .unwrap_or_else(|| panic!("the {id} tool is registered"))
}

fn placed(tool_id: &str, band: &DrawingBand) -> Mark {
    let mut marks = Marks::default();
    marks.place_on(tool(tool_id), band, ChartPoint::at(1.0, 0.0));
    marks.items()[0].clone()
}

/// A price mark says nothing, a time-only mark crosses every band, and a
/// mark whose indicator is off the chart is parked under its kind's name.
#[test]
fn the_label_names_the_band_or_the_indicator_it_waits_for() {
    let indicators = IndicatorViews::new();
    let price = label_for(&indicators, &placed("horizontal-line", &DrawingBand::Price));
    assert!(matches!(price, BandLabel::Price));
    assert_eq!(price.chip(), None);
    assert_eq!(price.hint(), None);

    let cvd = DrawingBand::Indicator(PaneKey {
        kind: "native.cvd".into(),
        ordinal: 0,
    });
    let time_only = label_for(&indicators, &placed("vertical-line", &cvd));
    assert!(matches!(time_only, BandLabel::AllBands));
    assert_eq!(time_only.chip().as_deref(), Some("all bands"));

    let parked = label_for(&indicators, &placed("horizontal-line", &cvd));
    assert!(matches!(parked, BandLabel::Parked(_)));
    assert_eq!(parked.chip().as_deref(), Some("not on chart - cvd"));
    assert!(parked.hint().unwrap().contains("not on the chart"));
}

/// A pane that is there but not painting names itself and says why.
#[test]
fn an_unpainted_band_names_its_pane_and_the_reason() {
    let unpainted = BandLabel::Unpainted("CVD".into(), "this pane is collapsed");
    assert_eq!(unpainted.chip().as_deref(), Some("CVD - not painted"));
    assert_eq!(unpainted.hint(), Some("this pane is collapsed"));
    let shown = BandLabel::Indicator("CVD".into());
    assert_eq!(shown.chip().as_deref(), Some("CVD"));
    assert_eq!(shown.hint(), None);
}
