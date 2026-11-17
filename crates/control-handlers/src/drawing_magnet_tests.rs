//! The magnet calls docked through the fake window: the rail's switch, and
//! the addressed pane's marks for one drawing's own switch and its scope.
use super::*;
use crate::test_support::{FakeAccess, FakePane, FakeTab, FakeWindow};
use quantick_control::error::codes;
use quantick_drawings::{ChartPoint, DRAWING_TOOLS, DrawingBand, DrawingStyle, NewDrawing};
use serde_json::json;

/// One tab (id 1) whose pane (id 2) holds one horizontal line.
fn window() -> (FakeWindow, u64) {
    window_with("horizontal-line")
}

/// One tab (id 1) whose pane (id 2) holds one drawing of tool `tool_id`.
fn window_with(tool_id: &str) -> (FakeWindow, u64) {
    let mut pane = FakePane::with_trades(2, 50, []);
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == tool_id)
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
    assert_eq!(scoped["pinned_bars"], "tick:50", "{scoped}");
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

#[test]
fn a_tool_that_snaps_by_its_own_rule_refuses_its_magnet_and_a_stale_flag_clears() {
    let ((mut window, id), actor) = (window_with("anchored-vwap"), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let mut input =
        json!({"tab_id": "1", "pane_id": "2", "drawing_id": id.to_string(), "enabled": true});
    let refused = object(&mut window, &mut access, &actor, &input).unwrap_err();
    assert_eq!(refused.code.as_str(), codes::INVALID_REQUEST);
    assert_eq!(
        refused.context.details,
        Some(json!({"reason": "tool_snaps_by_its_own_rule", "tool_id": "anchored-vwap"}))
    );
    assert!(access.events.is_empty(), "a refusal journals nothing");

    // A stale flag (an older build, a hand-edited layout) reads back off,
    // since no drag honours it, and turning it off is accepted and clears it.
    let mark = |window: &mut FakeWindow| window.tabs[0].panes[0].drawings.items_mut()[0].magnet;
    window.tabs[0].panes[0].drawings.items_mut()[0].magnet = true;
    assert!(!window.tabs[0].panes[0].drawings.items()[0].own_magnet());
    input["enabled"] = json!(false);
    let cleared = object(&mut window, &mut access, &actor, &input).unwrap();
    assert_eq!(cleared["changed"], true);
    assert!(!mark(&mut window));
    let again = object(&mut window, &mut access, &actor, &input).unwrap();
    assert_eq!(again["changed"], false);
}
