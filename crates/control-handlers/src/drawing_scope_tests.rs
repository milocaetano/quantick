use super::*;
use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingStyle, NewDrawing};

fn level(drawings: &mut Drawings<DrawingTool>, time_ms: Option<i64>) -> DrawingId {
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .unwrap();
    let point = ChartPoint::at_time(3.0, 100.0, time_ms);
    let fresh = |tool: DrawingTool| NewDrawing {
        style: DrawingStyle::default(),
        payload: tool.default_payload(),
    };
    assert!(drawings.place_with(tool, &DrawingBand::Price, point, fresh));
    drawings.items().last().unwrap().id
}

fn input(id: DrawingId, scope: ScopeName) -> DrawingScopeInput {
    DrawingScopeInput {
        tab_id: WireU64::new(1),
        pane_id: WireU64::new(2),
        drawing_id: WireU64::new(id.0),
        scope,
    }
}

#[test]
fn this_chart_pins_to_the_bars_and_a_repeat_changes_nothing() {
    let mut drawings = Drawings::default();
    let id = level(&mut drawings, Some(1_700_000_000_000));
    let ask = input(id, ScopeName::ThisChart);

    let set = set_scope(&mut drawings, &ask, "time:15m").unwrap();
    assert!(set.changed);
    assert_eq!(set.pinned_bars.as_deref(), Some("time:15m"));
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);

    let revision = drawings.revision();
    let again = set_scope(&mut drawings, &ask, "time:15m").unwrap();
    assert!(!again.changed, "naming the state it stands in is a no-op");
    assert_eq!(again.pinned_bars.as_deref(), Some("time:15m"));
    assert_eq!(drawings.revision(), revision, "nothing to write");

    let back = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "time:15m").unwrap();
    assert!(back.changed && back.pinned_bars.is_none());
    assert!(drawings.undo(), "one undo step per change");
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
}

#[test]
fn all_charts_on_a_drawing_without_market_time_is_refused_with_a_reason() {
    let mut drawings = Drawings::default();
    let id = level(&mut drawings, None);
    let error = set_scope(&mut drawings, &input(id, ScopeName::AllCharts), "tick:100").unwrap_err();
    assert_eq!(
        error.code,
        quantick_control::error::ControlError::invalid_request("").code
    );
    let details = error.context.details.unwrap();
    assert_eq!(details["reason"], "anchor_without_market_time");
    assert_eq!(drawings.items()[0].scope, DrawingScope::ThisChart);
}

#[test]
fn an_id_the_pane_does_not_hold_is_refused() {
    let mut drawings = Drawings::default();
    let error = set_scope(
        &mut drawings,
        &input(DrawingId(9), ScopeName::ThisChart),
        "tick:100",
    )
    .unwrap_err();
    assert!(error.message.contains("holds no drawing with id 9"));
}
