//! The configured source operation is reachable and leaves refusals untouched.
use super::*;

#[test]
fn configured_feed_selection_is_validated_retry_safe_and_reads_back() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("feed-source");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.recover");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let input = json!({"tab_id":app.tabs.active_id().to_string(),
        "feed_id":app.active_tab().active.0, "symbol":app.active_tab().active.1});
    let generation = app.active_tab().feed_generation();
    let mut observer = connect(&directory, &options("observer", &[]));
    let (denied, _) = unkeyed_call(&mut app, &mut observer, "feed.source.set", input.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.recover"]),
    );
    let mut invalid = input.clone();
    invalid["symbol"] = json!("NOT_CONFIGURED");
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, "feed.source.set", invalid);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "source-first",
        "feed.source.set",
        input.clone(),
        "source-key",
    );
    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "source-retry",
        "feed.source.set",
        input.clone(),
        "source-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(success_result(&first)["changed"], false);
    assert_eq!(app.active_tab().feed_generation(), generation);
    assert_eq!(app.active_tab().flow_pane.state.bars().len(), 4);
    let (read, _) = unkeyed_call(
        &mut app,
        &mut cockpit,
        "snapshot.read",
        json!({"scopes":["feed.status"]}),
    );
    let feed = &success_result(&read)["scopes"]["feed.status"]["value"];
    assert_eq!(feed["tabs"][0]["active_symbol"], input["symbol"]);
    assert!(
        feed["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|source| source["feed_id"] == input["feed_id"])
    );
    disable_test_gateway(&mut app, &ctx);
}
