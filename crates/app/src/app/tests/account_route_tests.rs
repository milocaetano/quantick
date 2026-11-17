//! The account route reaches its owner from the window: the toolbar notice
//! and a named call. What each decides is `quantick_paper::route`'s, tested
//! there.

use serde_json::json;

use super::*;

/// Broker routes on, on an ephemeral port, launching no bridge process.
fn with_broker_routes(app: &mut QuantickApp) {
    let mt5 = &mut app.config.broker.metatrader;
    mt5.enabled = true;
    mt5.exec_port = 0;
    mt5.exec_bridge_command = Vec::new();
    // The session reads the config once, as the window builds it.
    app.runtime.broker = crate::paper_trading::account_route::session(&app.config);
}

#[test]
fn the_toolbar_notice_opens_the_account_list_and_builds_the_venue() {
    let (mut app, _commands) = app_with_history(4);
    with_broker_routes(&mut app);
    app.apply_toolbar_action(crate::toolbar::ToolbarAction::OpenAccountList);
    let sheet = app
        .runtime
        .broker
        .sheet()
        .expect("broker routes are offered");
    assert!(sheet.open);
    assert!(app.runtime.broker.has_venue());
}

#[test]
fn a_named_route_call_reaches_the_route() {
    let (mut app, _commands) = app_with_history(4);
    with_broker_routes(&mut app);
    let origin = crate::control::ActionOrigin::Human;
    let read = app
        .control_action("trade.account.read", 1, origin, json!({}))
        .expect("the read dispatches");
    assert_eq!(read["route"], "sim");
    assert_eq!(read["broker_enabled"], true);
    let origin = crate::control::ActionOrigin::Human;
    let refused = app
        .control_action(
            "trade.account.select",
            1,
            origin,
            json!({ "route": "real" }),
        )
        .expect_err("REAL is never a named call");
    let code = refused
        .context
        .details
        .as_ref()
        .map(|d| d["route_code"].clone());
    assert_eq!(code, Some(json!("route.real_needs_hold")));
}
