//! The magnet calls docked through the fake window: the rail's switch, and
//! the addressed pane's marks for one drawing's own switch and its scope.
use super::*;
use crate::test_support::{FakeAccess, FakePane, FakeTab, FakeWindow};
use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingStyle, NewDrawing};
use serde_json::json;

/// One tab (id 1) whose pane (id 2) holds one horizontal line.
fn window() -> (FakeWindow, u64) {
    let mut pane = FakePane::with_trades(2, 50, []);
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .unwrap();
    let fresh = |tool: DrawingTool| NewDrawing {
        style: DrawingStyle::default(),
        payload: tool.default_payload(),
    };
    let point = ChartPoint::at_time(3.0, 100.0, Some(1_000));
    assert!(
        pane.drawings
            .place_with(tool, &DrawingBand::Price, point, fresh)
    );
    let id = pane.drawings.items()[0].id.0;
    let mut tab = FakeTab::new(1, "BTCUSDT");
    tab.panes.push(pane);
    (FakeWindow::new(vec![tab]), id)
}

#[test]
fn the_rail_call_flips_the_rail_and_journals_under_annotate() {
    let ((mut window, _), actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let on = json!({"enabled": true});
    assert_eq!(
        rail(&mut window, &mut access, &actor, &on).unwrap()["changed"],
        true
    );
    assert_eq!(
        rail(&mut window, &mut access, &actor, &on).unwrap()["changed"],
        false
    );
    assert!(window.rail_magnet);
    let event = &access.events[0];
    assert_eq!(event.module_id.as_str(), ANNOTATE_MODULE_ID);
    assert_eq!(event.kind.as_str(), DRAWING_MAGNET_EVENT_KIND);
}

#[test]
fn one_drawing_calls_reach_the_addressed_panes_marks() {
    let ((mut window, id), actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let address =
        |pane: &str| json!({"tab_id": "1", "pane_id": pane, "drawing_id": id.to_string()});
    let mut magnet = address("2");
    magnet["enabled"] = json!(false);
    let answer = object(&mut window, &mut access, &actor, &magnet);
    assert!(answer.is_ok(), "{answer:?}");
    let mut scope_input = address("2");
    scope_input["scope"] = json!("this_chart");
    let scoped = scope(&mut window, &mut access, &actor, &scope_input).unwrap();
    assert_eq!(scoped["pinned_bars"], "50t", "{scoped}");
    let kinds: Vec<_> = access
        .events
        .iter()
        .map(|event| event.kind.as_str().to_owned())
        .collect();
    assert_eq!(
        kinds,
        [
            OBJECT_MAGNET_EVENT_KIND,
            drawing_scope::DRAWING_SCOPE_EVENT_KIND
        ]
    );
    let mut elsewhere = address("9");
    elsewhere["enabled"] = json!(true);
    assert!(object(&mut window, &mut access, &actor, &elsewhere).is_err());
    assert_eq!(access.events.len(), 2);
}
