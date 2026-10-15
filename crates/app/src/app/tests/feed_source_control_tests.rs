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

/// A real pair switch through the gateway, as the SOURCE combos make one:
/// the generation advances by exactly one, the tab reads back the new pair,
/// the paper position on the old market is flattened and journaled under its
/// symbol, and the switch is journaled under the caller's name. A keyed retry
/// replays the answer without switching again.
#[test]
fn a_pair_switch_advances_the_generation_once_and_flattens_the_paper_position() {
    let ctx = egui::Context::default();
    let (mut app, evt_tx, _commands, _book) = test_app();
    let dir = crate::scratch::ScratchDir::new("feed-source-switch");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(dir.path().to_path_buf());
    let tab_id = app.tabs.active_id();
    evt_tx
        .try_send(FeedEvent::Backfilled(vec![trade(2)]))
        .unwrap();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    evt_tx.try_send(FeedEvent::Live(trade(4))).unwrap();
    app.active_tab_mut().drain_feed_with_clock(tab_id, || 0);
    assert!(!app.active_tab().paper.is_flat(), "a position is open");

    let directory = gateway_test_directory("feed-source-switch");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.recover");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.recover"]),
    );
    let generation = app.active_tab().feed_generation();
    let input = json!({"tab_id": tab_id.to_string(), "feed_id": "binance", "symbol": "ETHUSDT"});
    let (first, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "switch-first",
        "feed.source.set",
        input.clone(),
        "switch-key",
    );
    let result = success_result(&first);
    assert_eq!(result["changed"], true);
    assert_eq!(result["timeline_kept"], false);
    assert_eq!(
        (&result["from_feed_id"], &result["from_symbol"]),
        (&json!("binance"), &json!("TESTUSDT"))
    );
    assert_eq!(app.active_tab().feed_generation(), generation + 1);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert!(
        app.active_tab().paper.is_flat(),
        "the old market's position is flattened"
    );
    let flattened = std::fs::read_dir(dir.join("TESTUSDT"))
        .expect("the flatten was journaled under the old symbol")
        .count();
    assert_eq!(flattened, 1);

    let (retry, _) = keyed_call(
        &mut app,
        &mut cockpit,
        "switch-retry",
        "feed.source.set",
        input,
        "switch-key",
    );
    assert_eq!(first.outcome, retry.outcome);
    assert_eq!(app.active_tab().feed_generation(), generation + 1);
    let (read, _) = unkeyed_call(
        &mut app,
        &mut cockpit,
        "snapshot.read",
        json!({"scopes":["feed.status"]}),
    );
    let tab = &success_result(&read)["scopes"]["feed.status"]["value"]["tabs"][0];
    assert_eq!(tab["active_symbol"], "ETHUSDT");
    assert_eq!(tab["feed_generation"], (generation + 1).to_string());

    let switches: Vec<_> = app
        .control
        .control_access
        .as_ref()
        .unwrap()
        .journal()
        .read(1, 256, 1 << 22)
        .events
        .into_iter()
        .filter(|event| event.kind.as_str() == "feed.source.changed")
        .collect();
    assert_eq!(switches.len(), 1, "one switch, one record");
    let actor = switches[0]
        .actor
        .as_ref()
        .expect("the switch names its actor");
    assert_eq!(actor.client_name, CLIENT_NAME);
    assert_eq!(
        switches[0].payload["feed_source"]["from_symbol"],
        "TESTUSDT"
    );
    disable_test_gateway(&mut app, &ctx);
    let _ = std::fs::remove_dir_all(&directory);
}
