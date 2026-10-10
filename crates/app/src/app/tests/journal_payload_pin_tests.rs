//! The journal events every hand-journaled action writes, pinned byte for
//! byte: module, kind, actor and payload as a client reads them. A refactor
//! of how an action journals must leave this file untouched.
use super::*;
use serde_json::{Value, json};

/// One event as a client reads it, less the sequence and the wall-clock
/// stamp, which no refactor is asked to keep.
fn pinned(event: &quantick_control_host::journal::SemanticEvent) -> String {
    let mut payload = event.payload.clone();
    // A connection id is drawn fresh per window; its presence is pinned,
    // not its value.
    if let Some(connection) = payload.get_mut("connection_id") {
        *connection = json!("<connection>");
    }
    serde_json::to_string(&json!({
        "module_id": event.module_id,
        "kind": event.kind,
        "actor": event.actor,
        "payload": payload,
    }))
    .unwrap()
}

fn journaled(app: &QuantickApp) -> Vec<String> {
    app.control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 22)
        .events
        .iter()
        .map(pinned)
        .collect()
}

fn act(app: &mut QuantickApp, id: &str, input: Value) -> Value {
    app.control_action(
        id,
        crate::control::trade::CAPABILITY_VERSION,
        crate::control::ActionOrigin::Human,
        input,
    )
    .unwrap_or_else(|error| panic!("{id} dispatches: {error:?}"))
}

#[test]
fn every_hand_journaled_action_writes_its_pinned_event() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(20);
    run_frame(&mut app, &ctx);
    let tab_id = app.tabs.active_id().to_string();
    let pane_id = app.active_tab().flow_pane.id.to_string();
    let anchor = {
        let pane = app.active_tab().drawing_pane();
        let slot = pane.slots().saturating_sub(1);
        json!({
            "time_unix_ms": pane.slot_open_time(slot).expect("the bar has a time"),
            "price": "100",
        })
    };

    act(&mut app, "attention.mark.create", json!({ "note": "pin" }));
    act(
        &mut app,
        "annotate.label.create",
        json!({ "anchors": [anchor], "text": "pin" }),
    );
    act(&mut app, "annotate.magnet.set", json!({ "enabled": true }));
    act(
        &mut app,
        "layers.visibility.set",
        json!({ "tab_id": tab_id, "pane_id": pane_id, "layer_id": "bubble_overlap_merge", "visible": true }),
    );
    act(
        &mut app,
        "indicator.script.attach",
        json!({ "name": "pin", "source": "//@version=5\nindicator(\"pin\")\nplot(close)\n" }),
    );
    app.apply_toolbar_action(ToolbarAction::AddNative("native.cvd"));
    settle_indicators(&mut app);
    let cvd = app.active_tab().flow_pane.indicators.all()[0].slot.0;
    act(
        &mut app,
        "indicator.mouse_vertical_line.set",
        json!({ "tab_id": tab_id, "pane_id": pane_id, "slot_id": cvd.to_string(), "enabled": true }),
    );
    act(
        &mut app,
        "trade.order.place",
        json!({ "side": "buy", "kind": "limit", "quantity": "1", "price": "1" }),
    );
    act(&mut app, "notify.toast", json!({ "message": "pin" }));

    let events = journaled(&app);
    for event in &events {
        eprintln!("PINNED_EVENT {event}");
    }
    assert_eq!(events, PINNED_EVENTS, "a journal payload changed");
}

const PINNED_EVENTS: [&str; 8] = [
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"attention.mark.created","module_id":"attention","payload":{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"note":"pin","target":{"active_tab_id":"0","focused_pane_id":"0","focused_pane_side":"flow","pointer":null,"pointer_availability":{"available":false,"reason":"pointer_is_not_over_a_painted_chart"},"semantic_scene":{"available":true}},"target_source":"pointer"}}"#,
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"annotate.object.created","module_id":"annotate","payload":{"annotation":{"anchors":[{"price":"100","slot":"19","time_unix_ms":3000}],"annotation_id":"1","author":{"actor_kind":"human_ui","client_name":"quantick-ui"},"label":"Text 1","pane_id":"0","pane_side":"flow","tab_id":"0","tool_id":"text"}}}"#,
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"annotate.magnet.changed","module_id":"annotate","payload":{"drawing_magnet":{"changed":true,"enabled":true}}}"#,
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"layers.visibility.set","module_id":"layers","payload":{"connection_id":"<connection>","request_id":"ui-4","result":{"changed":true,"layer":{"blocked_reason":null,"effective":true,"id":"bubble_overlap_merge","label":"volume dots (Bookmap style)","persistence":"orderflow_preset","requested":true,"scope":"flow_pane"},"pane_id":"0","tab_id":"0"}}}"#,
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"indicator.script.attached","module_id":"indicator","payload":{"script":{"declared_inputs":[],"name":"pin","pane_side":"flow","slot_id":"0","tab_id":"0"}}}"#,
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"indicator.mouse_vertical_line.changed","module_id":"indicator","payload":{"indicator_guide":{"enabled":true,"pane_id":"0","slot_id":"0","tab_id":"0"}}}"#,
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"trade.order.placed","module_id":"trade","payload":{"accepted":true,"asked":{"kind":"limit","price":"1","quantity":"1","side":"buy"},"order_id":1,"rejected_because":null,"simulated":true}}"#,
    r#"{"actor":{"client_name":"quantick-ui","kind":"human_ui"},"kind":"notify.raised","module_id":"notify","payload":{"channel":"toast","delivered":true,"message":"pin"}}"#,
];

/// The notification budget's refusal, pinned but for the seconds left, which
/// read a monotonic clock.
#[test]
fn a_spent_notification_budget_refuses_in_its_pinned_words() {
    let (mut app, _commands) = app_with_history(4);
    let refusal = (0..8)
        .find_map(|_| {
            app.control_action(
                "notify.toast",
                crate::control::trade::CAPABILITY_VERSION,
                crate::control::ActionOrigin::Human,
                json!({ "message": "pin" }),
            )
            .err()
        })
        .expect("the budget runs out");
    let text = serde_json::to_string(&refusal)
        .unwrap()
        .replace("about 10 second", "about N second")
        .replace("about 9 second", "about N second");
    eprintln!("PINNED_REFUSAL {text}");
    assert_eq!(text, PINNED_REFUSAL, "the refusal changed");
}

const PINNED_REFUSAL: &str = r#"{"code":"control.backpressure","message":"this client's notification budget is spent","retryable":true,"next_steps":["Notifications are limited to 6 per minute with a burst of 2; retry in about N second(s)."]}"#;
