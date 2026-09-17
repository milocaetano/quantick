//! Admitted control calls and UI commands converge at the headless owner.
use super::*;
use quantick_engine::bar_registry::BUILTIN_BARS;
use quantick_engine::bar_selection::{BarInputAvailability, SelectionCommand};

#[test]
fn subfloor_shared_bar_operation_applies_only_the_effective_configuration() {
    let (mut app, _commands) = app_with_history(50);
    for id in ["volume", "dollar"] {
        let raw = BUILTIN_BARS.parse(&format!("{id}:0.000000001")).unwrap();
        let effective = raw.clamped();
        let tab = app.active_tab_mut();
        assert!(tab.set_pane_bar_spec(0, raw).unwrap());
        assert_eq!(tab.flow_pane.state.spec(), &effective);
        assert_eq!(tab.flow_pane.spec.spec(), effective);
        let revision = tab.flow_pane.state.series_revision();
        tab.apply_spec_changes();
        tab.apply_spec_changes();
        assert_eq!(tab.flow_pane.spec.pending(), None);
        assert_eq!(
            tab.flow_pane.state.series_revision(),
            revision,
            "no deferred second rebuild"
        );
        assert!(!tab.set_pane_bar_spec(0, raw).unwrap());
        assert_eq!(
            tab.flow_pane.state.series_revision(),
            revision,
            "repeated request is a no-op"
        );
    }
}

#[test]
fn subfloor_admitted_bar_requests_are_idempotent_and_do_not_rebuild_again() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    app.active_tab_mut().set_layout(CanvasLayout::TimeAndFlow);
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    app.active_tab_mut().focus = PaneSide::Flow;
    let target = PaneSide::Time(0);
    assert!(app.active_tab().pane_at(1).is_some());
    // Target the nonfocused pane: the focused editor's legacy passive range
    // clamp is a separate UI edit, not part of this admitted operation.
    let directory = gateway_test_directory("bar-registry-subfloor");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &cockpit_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    for id in ["volume", "dollar"] {
        assert_eq!(app.active_tab().focused_side(), PaneSide::Flow);
        let text = format!("{id}:0.000000001");
        let effective = BUILTIN_BARS.parse(&text).unwrap().clamped();
        let response = remote_bar_call(
            &mut app,
            &ctx,
            &mut client,
            serde_json::json!({"pane":"1", "spec":text}),
        );
        assert_eq!(success_result(&response)["changed"], true);
        assert_eq!(app.active_tab().pane(target).state.spec(), &effective);
        assert_eq!(app.active_tab().pane(target).spec.spec(), effective);
        let revision = app.active_tab().pane(target).state.series_revision();
        for _ in 0..3 {
            run_frame(&mut app, &ctx);
        }
        assert_eq!(
            app.active_tab().pane(target).state.series_revision(),
            revision
        );
        let response = remote_bar_call(
            &mut app,
            &ctx,
            &mut client,
            serde_json::json!({"pane":"1", "spec":text}),
        );
        assert_eq!(success_result(&response)["changed"], false);
        assert_eq!(
            app.active_tab().pane(target).state.series_revision(),
            revision
        );
        assert_eq!(app.active_tab().pane(target).spec.pending(), None);
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn bar_registry_commands_match_admitted_remote_changes_and_refuse_missing_inputs() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("bar-registry-commands");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &cockpit_test_options())
            .unwrap()
            .select(None)
            .unwrap();

    for text in [
        "tick:3",
        "volume:5",
        "dollar:500",
        "time:1s",
        "imbalance:8",
        "imbalance:volume:8",
        "imbalance:dollar:8",
    ] {
        let config = BUILTIN_BARS.parse(text).unwrap();
        app.active_tab_mut()
            .flow_pane
            .spec
            .update(
                SelectionCommand::Replace(config),
                BarInputAvailability::PRINTS,
            )
            .unwrap();
        app.active_tab_mut().apply_spec_changes();
        app.active_tab_mut().apply_spec_changes();
        let expected =
            quantick_engine::fixture::write_bars(app.active_tab().flow_pane.state.bars());
        app.active_tab_mut().flow_pane.set_spec(BarSpec::Tick(50));
        let response = remote_bar_call(
            &mut app,
            &ctx,
            &mut client,
            serde_json::json!({"pane":"0", "spec":text}),
        );
        assert!(
            matches!(
                response.outcome,
                quantick_control::wire::ResponseOutcome::Success { .. }
            ),
            "{text}: {:?}",
            response.outcome
        );
        assert_eq!(app.active_tab().flow_pane.state.spec(), &config);
        assert_eq!(
            quantick_engine::fixture::write_bars(app.active_tab().flow_pane.state.bars()),
            expected
        );
    }
    let before = *app.active_tab().flow_pane.state.spec();
    for text in [
        "trades:10",
        "tick:0",
        "unknown:10",
        "time:99ms",
        "imbalance:unknown:8",
    ] {
        let response = remote_bar_call(
            &mut app,
            &ctx,
            &mut client,
            serde_json::json!({"pane":"0", "spec":text}),
        );
        assert_eq!(
            response_error(&response).code.as_str(),
            quantick_control::error::codes::INVALID_REQUEST
        );
        assert_eq!(
            app.active_tab().flow_pane.state.spec(),
            &before,
            "a refused command is atomic"
        );
    }
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn bar_registry_remote_changes_still_require_the_granted_layout_scope() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("bar-registry-denied");
    grant_annotate_for_test(&mut app, "all-reads");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &cockpit_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let before = *app.active_tab().flow_pane.state.spec();
    let response = remote_bar_call(
        &mut app,
        &ctx,
        &mut client,
        serde_json::json!({"pane":"0", "spec":"tick:3"}),
    );
    assert_eq!(
        response_error(&response).code.as_str(),
        quantick_control::error::codes::PERMISSION_DENIED
    );
    assert_eq!(app.active_tab().flow_pane.state.spec(), &before);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}

// V1 intentionally retains its historical float result and is refused by the
// wire codec. The registry change uses the admitted, encodable v2 contract.
fn remote_bar_call(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    payload: serde_json::Value,
) -> quantick_control::wire::ResponseEnvelope {
    let request_id = client
        .send_versioned("layout.pane.set_bar_spec", 2, payload)
        .unwrap();
    for _ in 0..400 {
        run_frame(app, ctx);
        if client.reply_pending(std::time::Duration::from_millis(5)) {
            break;
        }
    }
    let response = client.read().unwrap();
    assert_eq!(response.request_id, request_id);
    response
}
