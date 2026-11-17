//! The layer switch through the fake window: a blocked layer is refused
//! before anything switches, and every admitted switch is journaled.
use super::*;
use crate::chart_layer::LayerBlocked;
use crate::test_support::{FakeAccess, FakePane, FakeTab, FakeWindow};
use quantick_control_schema::layers::LayerSnapshot;

fn layer(id: &str, requested: bool) -> LayerSnapshot {
    LayerSnapshot {
        id: id.to_owned(),
        label: id.to_owned(),
        scope: "pane".to_owned(),
        persistence: "chart_layers".to_owned(),
        requested,
        effective: requested,
        blocked_reason: None,
    }
}

/// One tab (id 1) whose pane (id 2) has a free grid and a blocked tape.
fn window() -> FakeWindow {
    let mut pane = FakePane::with_trades(2, 50, []);
    pane.layers.push((layer("grid", false), None));
    let blocked = LayerBlocked {
        code: "needs_order_flow",
        explanation: "the pane draws no order flow",
    };
    pane.layers.push((layer("tape", false), Some(blocked)));
    let mut tab = FakeTab::new(1, "BTCUSDT");
    tab.panes.push(pane);
    FakeWindow::new(vec![tab])
}

fn input(layer: &str, visible: bool) -> Value {
    json!({"tab_id": "1", "pane_id": "2", "layer_id": layer, "visible": visible})
}

#[test]
fn a_switch_answers_what_changed_and_journals_every_admitted_application() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let answer = set_visibility(&mut window, &mut access, &actor, &input("grid", true)).unwrap();
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["layer"]["requested"], true);
    let again = set_visibility(&mut window, &mut access, &actor, &input("grid", true)).unwrap();
    assert_eq!(again["changed"], false);
    assert_eq!(access.events.len(), 2);
    assert_eq!(access.events[1].payload["result"], again);
    assert_eq!(window.layer_switches.len(), 2);
}

#[test]
fn a_blocked_layer_is_refused_with_its_code_and_nothing_switches() {
    let (mut window, actor) = (window(), FakeWindow::assistant());
    let mut access = FakeAccess::with_budget(0);
    let error = set_visibility(&mut window, &mut access, &actor, &input("tape", true)).unwrap_err();
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert!(!error.retryable);
    assert_eq!(
        error.context.details,
        Some(json!({"reason": "needs_order_flow"}))
    );
    let unknown = set_visibility(&mut window, &mut access, &actor, &input("volume", true));
    assert!(unknown.is_err());
    assert!(window.layer_switches.is_empty());
    assert!(access.events.is_empty());
}
