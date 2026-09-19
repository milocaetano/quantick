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
    wait_for_bar_response(app, ctx, client, &request_id);
    let response = client.read().unwrap();
    assert_eq!(response.request_id, request_id);
    response
}

// A UI operation is complete before its worker necessarily serializes the reply.
// An extra frame here would be a new passive editor operation, not part of the call.
fn wait_for_bar_response(
    app: &mut QuantickApp,
    ctx: &egui::Context,
    client: &mut quantick_control_local::client::LocalClient,
    request_id: &quantick_control::id::RequestId,
) {
    app.control
        .control_access
        .as_mut()
        .unwrap()
        .arm_completion_for_test(request_id.clone());
    for _ in 0..400 {
        run_frame(app, ctx);
        if app
            .control
            .control_access
            .as_ref()
            .unwrap()
            .completed_for_test(request_id)
            || client.reply_pending(std::time::Duration::from_millis(5))
        {
            break;
        }
    }
}

/// Owns the only response gate; unwinding releases the worker as well.
struct BarResponseGate {
    target: std::sync::Arc<std::sync::Mutex<Option<quantick_control::id::RequestId>>>,
    reached: crossbeam_channel::Receiver<quantick_control::id::RequestId>,
    release: crossbeam_channel::Sender<()>,
}

impl BarResponseGate {
    fn install(app: &mut QuantickApp, ctx: &egui::Context, directory: &std::path::Path) -> Self {
        let target = std::sync::Arc::new(std::sync::Mutex::new(None));
        let observed_target = std::sync::Arc::clone(&target);
        let (reached_tx, reached) = crossbeam_channel::bounded(1);
        let (release, release_rx) = crossbeam_channel::bounded(1);
        let before_write =
            std::sync::Arc::new(move |request_id: &quantick_control::id::RequestId| {
                if observed_target.lock().unwrap().as_ref() != Some(request_id) {
                    return;
                }
                reached_tx.try_send(request_id.clone()).unwrap();
                release_rx
                    .recv_timeout(Self::window())
                    .expect("the test releases its response gate");
            });
        app.control
            .control_access
            .as_mut()
            .unwrap()
            .enable_before_write_for_test(ctx, directory.to_path_buf(), before_write);
        wait_for_test_gateway_descriptor(app, ctx);
        Self {
            target,
            reached,
            release,
        }
    }

    fn window() -> std::time::Duration {
        std::time::Duration::from_millis(quantick_control::limits::CONTROL_REQUEST_TIMEOUT_MS)
    }

    fn arm(&self, app: &mut QuantickApp, request_id: &quantick_control::id::RequestId) {
        *self.target.lock().unwrap() = Some(request_id.clone());
        app.control
            .control_access
            .as_mut()
            .unwrap()
            .arm_completion_for_test(request_id.clone());
    }

    fn wait_until_held(&self, request_id: &quantick_control::id::RequestId) {
        assert_eq!(
            &self.reached.recv_timeout(Self::window()).unwrap(),
            request_id
        );
    }
}

impl Drop for BarResponseGate {
    fn drop(&mut self) {
        let _ = self.release.try_send(());
    }
}

#[test]
fn delayed_bar_response_preserves_focused_operation_value() {
    delayed_bar_response_fixture(false);
    delayed_bar_response_fixture(true);
}

fn delayed_bar_response_fixture(unwind: bool) {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(50);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("bar-registry-delayed-response");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    let gate = BarResponseGate::install(&mut app, &ctx, &directory);
    let mut client =
        quantick_control_local::client::discover_in(&directory, &cockpit_test_options())
            .unwrap()
            .select(None)
            .unwrap();
    let config = BUILTIN_BARS.parse("dollar:500").unwrap();
    let request_id = client
        .send_versioned(
            "layout.pane.set_bar_spec",
            2,
            serde_json::json!({"pane":"0", "spec":"dollar:500"}),
        )
        .unwrap();
    gate.arm(&mut app, &request_id);
    wait_for_bar_response(&mut app, &ctx, &mut client, &request_id);
    assert!(
        app.control
            .control_access
            .as_ref()
            .unwrap()
            .completed_for_test(&request_id)
    );
    gate.wait_until_held(&request_id);
    assert_eq!(app.active_tab().flow_pane.state.spec(), &config);
    assert_eq!(
        app.active_tab().flow_pane.spec.pending(),
        Some(BUILTIN_BARS.parse("dollar:1000").unwrap())
    );
    assert!(!client.reply_pending(std::time::Duration::from_millis(5)));
    if unwind {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            let _owned_gate = gate;
            panic!("exercise response-gate cleanup during assertion unwinding");
        }));
        assert!(result.is_err());
    } else {
        drop(gate);
    }
    let response = client.read().unwrap();
    assert_eq!(response.request_id, request_id);
    assert!(matches!(
        response.outcome,
        quantick_control::wire::ResponseOutcome::Success { .. }
    ));
    assert_eq!(app.active_tab().flow_pane.state.spec(), &config);
    assert!(
        !app.control
            .control_access
            .as_ref()
            .unwrap()
            .completed_for_test(
                &quantick_control::id::RequestId::new("unrelated-request").unwrap()
            )
    );
    // A deliberate later idle frame still obeys the unchanged editor contract.
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().flow_pane.state.spec(),
        &BUILTIN_BARS.parse("dollar:1000").unwrap()
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).unwrap();
}
