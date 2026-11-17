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

#[test]
fn the_scope_lists_every_pane_tab_by_tab_with_its_layers_in_menu_order() {
    let mut window = window();
    let mut second = FakeTab::new(3, "WINZ26");
    second.panes.push(FakePane::with_trades(4, 50, []));
    window.tabs.push(second);
    let scope = snapshot(&window);
    assert_eq!(scope.omitted_panes.get(), 0);
    let panes: Vec<_> = scope
        .panes
        .iter()
        .map(|pane| (pane.tab_id.get(), pane.pane_id.get()))
        .collect();
    assert_eq!(panes, [(1, 2), (3, 4)]);
    let ids: Vec<_> = scope.panes[0]
        .layers
        .iter()
        .map(|layer| layer.id.as_str())
        .collect();
    assert_eq!(ids, ["grid", "tape"]);
    assert!(scope.panes[1].layers.is_empty());
}

#[test]
fn panes_past_the_bound_are_counted_rather_than_listed() {
    let mut tab = FakeTab::new(1, "BTCUSDT");
    for id in 0..MAX_SNAPSHOT_PANES as u64 + 3 {
        tab.panes.push(FakePane::with_trades(id, 50, []));
    }
    let scope = snapshot(&FakeWindow::new(vec![tab]));
    assert_eq!(scope.panes.len(), MAX_SNAPSHOT_PANES);
    assert_eq!(scope.omitted_panes.get(), 3);
}

#[test]
fn an_address_that_names_nothing_is_refused_in_the_calls_words() {
    let window = window();
    let refusal = |tab, pane, layer| {
        chart_layer(&window, WireU64::new(tab), WireU64::new(pane), layer)
            .unwrap_err()
            .message
    };
    assert_eq!(refusal(9, 2, "grid"), "no open tab has id 9");
    assert_eq!(
        refusal(1, 9, "grid"),
        "layer visibility names an unknown pane on the requested tab"
    );
    assert_eq!(
        refusal(1, 2, "volume"),
        "layer visibility names an unknown registered layer"
    );
    let read = chart_layer(&window, WireU64::new(1), WireU64::new(2), "tape").unwrap();
    assert_eq!(
        read.blocked.map(|blocked| blocked.code),
        Some("needs_order_flow")
    );
}

/// A pane the bounded scope leaves out is still addressed by id, and its
/// switch's journal entry names the request, the connection and the actor
/// beside the answer, so a client that lost the answer can reconcile.
#[test]
fn a_pane_past_the_bound_is_switched_and_its_journal_names_the_request() {
    let mut tab = FakeTab::new(1, "BTCUSDT");
    for id in 0..=MAX_SNAPSHOT_PANES as u64 {
        let mut pane = FakePane::with_trades(id, 50, []);
        pane.layers.push((layer("crosshair", false), None));
        tab.panes.push(pane);
    }
    let mut window = FakeWindow::new(vec![tab]);
    let last = MAX_SNAPSHOT_PANES as u64;
    let scope = snapshot(&window);
    assert_eq!(scope.omitted_panes.get(), 1);
    assert!(scope.panes.iter().all(|pane| pane.pane_id.get() != last));
    let (actor, mut access) = (FakeWindow::assistant(), FakeAccess::with_budget(0));
    let input = json!({"tab_id": "1", "pane_id": last.to_string(),
        "layer_id": "crosshair", "visible": true});
    let answer = set_visibility(&mut window, &mut access, &actor, &input).unwrap();
    assert_eq!(answer["changed"], true);
    assert_eq!(answer["pane_id"], last.to_string());
    let event = &access.events[0];
    assert_eq!(event.kind.as_str(), EVENT_KIND);
    assert_eq!(event.payload["request_id"], json!(actor.request_id));
    assert_eq!(event.payload["connection_id"], json!(actor.connection_id));
    assert_eq!(event.payload["result"], answer);
    let author = event.actor.as_ref().map(|actor| actor.client_name.as_str());
    assert_eq!(author, Some("fake assistant"));
    assert_eq!(
        window.layer_switches,
        [(last, "crosshair".to_owned(), true)]
    );
}
