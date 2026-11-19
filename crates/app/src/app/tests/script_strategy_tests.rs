//! The script-strategy calls reach the active tab's own strategies through
//! the tab's lend. What arming, listing and disarming decide is
//! `quantick_session::script_strategies`' and the handler's, tested there.

use serde_json::json;

use super::*;

#[test]
fn the_script_strategy_calls_reach_the_tab_s_strategies() {
    let (mut app, _commands) = app_with_history(4);
    let flow = app.active_tab().flow_pane.id;
    let origin = || crate::control::ActionOrigin::Human;
    let input = json!({
        "script": "renko_poc_flip.pine",
        "side": "buy",
        "placement": { "kind": "anywhere" },
    });
    let armed = app
        .control_action("strategy.script.arm", 1, origin(), input)
        .expect("the bundled flip arms from the default library");
    let id = armed["strategy"]["id"].clone();
    assert_eq!(
        app.active_tab().market.strategies.script_readings().len(),
        1,
        "the arm landed on the tab's own strategies"
    );

    let listed = app
        .control_action("strategy.script.list", 1, origin(), json!({}))
        .expect("the list dispatches");
    assert_eq!(
        listed["library"]["folder"], "strategies",
        "no launch folder: the default"
    );
    assert_eq!(listed["armed"][0]["id"], id);
    assert_eq!(listed["armed"][0]["pane_id"], flow.to_string());

    let input = json!({ "id": id });
    let gone = app
        .control_action("strategy.script.disarm", 1, origin(), input)
        .expect("the disarm dispatches");
    assert_eq!(gone["disarmed"], true);
    assert!(
        app.active_tab()
            .market
            .strategies
            .script_readings()
            .is_empty()
    );
}
