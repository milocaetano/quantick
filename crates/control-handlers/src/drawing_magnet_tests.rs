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

/// The rail's switch answers the value and whether it moved, journals that
/// answer on every call, and refuses malformed input before it switches.
#[test]
fn the_rail_call_answers_what_moved_and_refuses_malformed_input() {
    let ((mut window, _), actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    for invalid in [
        json!({"enabled": "yes"}),
        json!({}),
        json!({"enabled": true, "x": 1}),
    ] {
        let refused = rail(&mut window, &mut access, &actor, &invalid).unwrap_err();
        assert_eq!(refused.code.as_str(), codes::INVALID_REQUEST, "{invalid}");
    }
    assert!(!window.rail_magnet, "a refused call changes nothing");
    assert!(access.events.is_empty(), "nor journals");
    for (enabled, changed) in [(true, true), (true, false), (false, true)] {
        let input = json!({ "enabled": enabled });
        let answer = rail(&mut window, &mut access, &actor, &input).unwrap();
        assert_eq!(answer, json!({"enabled": enabled, "changed": changed}));
        assert_eq!(window.rail_magnet, enabled);
    }
    let journaled: Vec<_> = access
        .events
        .iter()
        .map(|event| event.payload["drawing_magnet"]["enabled"].clone())
        .collect();
    assert_eq!(journaled, [json!(true), json!(true), json!(false)]);
}

/// One drawing's switch answers its full address, journals every admitted
/// call, a repeat included, never moves the rail's magnet, and refuses an
/// address or input that names nothing before anything moves.
#[test]
fn one_drawings_call_answers_its_address_and_refuses_what_names_nothing() {
    let ((mut window, id), actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let id = id.to_string();
    let call = |enabled: Value| json!({"tab_id": "1", "pane_id": "2", "drawing_id": id, "enabled": enabled});
    for invalid in [
        call(json!("yes")),
        json!({"tab_id": "1", "pane_id": "2", "enabled": true}),
        json!({"tab_id": "1", "pane_id": "2", "drawing_id": id}),
        json!({"pane_id": "2", "drawing_id": id, "enabled": true}),
        json!({"tab_id": "1", "drawing_id": id, "enabled": true}),
        json!({"tab_id": "1", "pane_id": "2", "drawing_id": id, "enabled": true, "x": 1}),
        json!({"tab_id": "1", "pane_id": "2", "drawing_id": "999999", "enabled": true}),
        json!({"tab_id": "1", "pane_id": "999999", "drawing_id": id, "enabled": true}),
        json!({"tab_id": "999999", "pane_id": "2", "drawing_id": id, "enabled": true}),
    ] {
        let refused = object(&mut window, &mut access, &actor, &invalid).unwrap_err();
        assert_eq!(refused.code.as_str(), codes::INVALID_REQUEST, "{invalid}");
    }
    let own = |window: &FakeWindow| window.tabs[0].panes[0].drawings.items()[0].magnet;
    assert!(!own(&window), "a refused call changes nothing");
    assert!(access.events.is_empty(), "nor journals");
    for (enabled, changed) in [(true, true), (true, false), (false, true)] {
        let answer = object(&mut window, &mut access, &actor, &call(json!(enabled))).unwrap();
        let expected = json!({"drawing_id": id, "tab_id": "1", "pane_id": "2",
            "enabled": enabled, "changed": changed});
        assert_eq!(answer, expected);
        assert_eq!(own(&window), enabled);
        assert!(!window.rail_magnet, "the rail's magnet is another switch");
    }
    let journaled: Vec<_> = access
        .events
        .iter()
        .map(|event| event.payload["drawing_magnet"]["enabled"].clone())
        .collect();
    assert_eq!(journaled, [json!(true), json!(true), json!(false)]);
}

/// A drawing id is unique only within its pane: with the same id on two
/// panes of a tab, a call moves exactly the one on the pane it names.
#[test]
fn a_drawing_call_moves_only_the_named_panes_drawing_when_ids_collide() {
    let ((mut window, id), actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let tool = DRAWING_TOOLS
        .into_iter()
        .find(|tool| tool.id() == "horizontal-line")
        .unwrap();
    let mut other = FakePane::with_trades(3, 50, []);
    let fresh = |tool: DrawingTool| NewDrawing {
        style: DrawingStyle::default(),
        payload: tool.default_payload(),
    };
    let point = ChartPoint::at_time(5.0, 100.0, Some(1_000));
    assert!(
        other
            .drawings
            .place_with(tool, &DrawingBand::Price, point, fresh)
    );
    other.drawings.items_mut()[0].id = DrawingId(id);
    window.tabs[0].panes.push(other);
    let own = |window: &FakeWindow| {
        let panes = &window.tabs[0].panes;
        (
            panes[0].drawings.items()[0].magnet,
            panes[1].drawings.items()[0].magnet,
        )
    };
    for (pane, expected) in [("3", (false, true)), ("2", (true, true))] {
        let input =
            json!({"tab_id": "1", "pane_id": pane, "drawing_id": id.to_string(), "enabled": true});
        let answer = object(&mut window, &mut access, &actor, &input).unwrap();
        assert_eq!(answer["pane_id"], pane);
        assert_eq!(
            own(&window),
            expected,
            "only the named pane's drawing moved"
        );
    }
}

/// The docked scope call answers a repeat as no change and journals it too.
#[test]
fn the_scope_call_answers_a_repeat_as_no_change() {
    let ((mut window, id), actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let mut input =
        json!({"tab_id": "1", "pane_id": "2", "drawing_id": id.to_string(), "scope": "all_charts"});
    let wide = scope(&mut window, &mut access, &actor, &input).unwrap();
    assert_eq!(wide["changed"], true, "a new drawing starts on this chart");
    input["scope"] = json!("this_chart");
    let first = scope(&mut window, &mut access, &actor, &input).unwrap();
    assert_eq!(first["scope"], "this_chart");
    assert_eq!(first["changed"], true);
    let again = scope(&mut window, &mut access, &actor, &input).unwrap();
    assert_eq!(again["changed"], false, "a repeat");
    assert_eq!(again["pinned_bars"], "tick:50");
    assert_eq!(access.events.len(), 3);
}
