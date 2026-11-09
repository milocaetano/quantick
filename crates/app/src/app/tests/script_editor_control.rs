//! Editor actions through the admitted local gateway.
use super::*;
use quantick_stores::script_library::ScriptLibrary;

#[test]
fn script_library_control_discovery_permissions_paging_and_copy_retry() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let sources = crate::scratch::ScratchDir::new("library-control-sources");
    app.runtime.indicators.script_library = ScriptLibrary::open(&sources);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("library-control");
    grant_annotate_for_test(
        &mut app,
        "all-reads,cockpit,cockpit.layout,observe.user_text",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let capability = "indicator.library.edit";
    assert!(
        crate::control::registered_action_versions()
            .iter()
            .any(|(id, _)| id == capability)
    );
    let mut denied = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let refusal = unkeyed_call(
        &mut app,
        &mut denied,
        capability,
        json!({"operation":{"action":"open","id":"ema.pine"}}),
    )
    .0;
    assert_eq!(error_code(&refusal), Some(codes::PERMISSION_DENIED));
    assert!(app.surfaces.script_editor.state.document.is_none());
    let mut client = connect(
        &directory,
        &options(
            "cockpit",
            &["cockpit", "cockpit.layout", "observe.user_text"],
        ),
    );
    let list = success_result(
        &unkeyed_call(
            &mut app,
            &mut client,
            capability,
            json!({"operation":{"action":"list"},"limit":1}),
        )
        .0,
    );
    assert_eq!(list["entries"].as_array().unwrap().len(), 1);
    assert_eq!(list["next_offset"], 1);
    let bad = unkeyed_call(
        &mut app,
        &mut client,
        capability,
        json!({"operation":{"action":"list"},"limit":65}),
    )
    .0;
    assert_eq!(error_code(&bad), Some(codes::INVALID_REQUEST));
    let payload =
        json!({"operation":{"action":"copy","id":"ema.pine","new_id":"control-copy.pine"}});
    let first = keyed_call(
        &mut app,
        &mut client,
        "copy-first",
        capability,
        payload.clone(),
        "library-copy",
    )
    .0;
    let retry = keyed_call(
        &mut app,
        &mut client,
        "copy-retry",
        capability,
        payload,
        "library-copy",
    )
    .0;
    assert_eq!(success_result(&first), success_result(&retry));
    assert_eq!(
        app.runtime
            .indicators
            .script_library
            .entries()
            .iter()
            .filter(|entry| entry.name == "control-copy.pine")
            .count(),
        1
    );
    assert!(sources.join("control-copy.pine").exists());
    let private_source = "plot(private_signal)";
    let _ = unkeyed_call(
        &mut app,
        &mut client,
        capability,
        json!({"operation":{"action":"replace_source","source": private_source}}),
    );
    let events = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 22);
    let editor_events: Vec<_> = events
        .events
        .iter()
        .filter(|event| event.kind.as_str() == "indicator.library.changed")
        .collect();
    assert!(!editor_events.is_empty());
    for event in editor_events {
        let payload = event.payload.to_string();
        assert!(!payload.contains(private_source));
        assert!(!payload.contains("control-copy"));
        assert!(!payload.contains("diagnostics"));
    }
    disable_test_gateway(&mut app, &ctx);
}
