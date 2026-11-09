//! The configured source operation is reachable and leaves refusals untouched.
use super::*;

#[test]
fn configured_feed_selection_is_validated_retry_safe_and_reads_back() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    let directory = gateway_test_directory("feed-source");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.recover");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let input = json!({"tab_id":app.runtime.tabs.active_id().to_string(),
        "feed_id":app.active_tab().active.0, "symbol":app.active_tab().active.1});
    let generation = app.active_tab().link.feed_generation();
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
    assert_eq!(app.active_tab().link.feed_generation(), generation);
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
    let tab_id = app.runtime.tabs.active_id();
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
    let generation = app.active_tab().link.feed_generation();
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
    assert_eq!(app.active_tab().link.feed_generation(), generation + 1);
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
    assert_eq!(app.active_tab().link.feed_generation(), generation + 1);
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

/// The roll through the gateway: the tab lands on the configured contract
/// with its level still on the chart, the old contract keeps its copy, a
/// repeat rolls nothing, and an open paper position refuses the roll.
#[test]
fn a_roll_keeps_the_drawings_and_a_repeat_rolls_nothing() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = split_app(&ctx, 200);
    app.config.feeds[0].contract_rolls =
        std::collections::BTreeMap::from([("TESTUSDT".to_owned(), "ETHUSDT".to_owned())]);
    place_level(&mut app, PaneSide::Flow, 100.0);
    run_frame(&mut app, &ctx);
    let bars = format!("{:?}", app.active_tab().flow_pane.spec.spec().definition());

    let directory = gateway_test_directory("contract-roll");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.recover");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut observer = connect(&directory, &options("observer", &[]));
    let tab_id = app.runtime.tabs.active_id();
    let input = json!({"tab_id": tab_id.to_string(), "from_symbol": "TESTUSDT"});
    let (denied, _) = unkeyed_call(&mut app, &mut observer, "feed.contract.roll", input.clone());
    assert_eq!(error_code(&denied), Some(codes::PERMISSION_DENIED));

    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.recover"]),
    );
    let generation = app.active_tab().link.feed_generation();
    let (rolled, _) = unkeyed_call(&mut app, &mut cockpit, "feed.contract.roll", input.clone());
    let result = success_result(&rolled);
    assert_eq!(result["symbol"], "ETHUSDT");
    assert_eq!(result["changed"], true);
    assert_eq!(result["drawings_carried"], 1);
    assert_eq!(app.active_tab().link.feed_generation(), generation + 1);

    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert_eq!(drawings_on(&app, PaneSide::Flow), vec![100.0]);
    assert!(
        !app.active_tab().flow_pane.drawings.items()[0].foreign_market,
        "a rolled level is at home on the new contract"
    );
    assert_eq!(
        format!("{:?}", app.active_tab().flow_pane.spec.spec().definition()),
        bars,
        "the workspace stayed"
    );

    let (repeat, _) = unkeyed_call(&mut app, &mut cockpit, "feed.contract.roll", input);
    assert_eq!(success_result(&repeat)["changed"], false);
    assert_eq!(app.active_tab().link.feed_generation(), generation + 1);

    run_frame(&mut app, &ctx);
    app.active_tab_mut().symbol = "TESTUSDT".to_owned();
    run_frame(&mut app, &ctx);
    assert_eq!(
        drawings_on(&app, PaneSide::Flow),
        vec![100.0],
        "the old contract kept its copy"
    );
    disable_test_gateway(&mut app, &ctx);
}

/// What the three surfaces say the tab shows: the toolbar button's words,
/// the tab chip's label and the status bar's venue and symbol.
fn surfaces_read(app: &mut QuantickApp, ctx: &egui::Context, symbol: &str) -> [String; 3] {
    let output = run_frame(app, ctx);
    let status = app.status_model();
    let button =
        super::super::market_switch_tests::painted_starting(&output, &format!("{symbol} \u{b7} "));
    assert_eq!(
        button.len(),
        1,
        "one market button reads {symbol}: {button:?}"
    );
    [
        button[0]
            .trim_end_matches(egui_phosphor::regular::CARET_DOWN)
            .to_owned(),
        app.active_tab().chip_label().to_owned(),
        format!("{} \u{b7} {}", status.symbol, status.venue),
    ]
}

/// `tab.market.set` through the gateway moves the toolbar button, the tab
/// chip and the status cell together, and a repeat moves nothing.
#[test]
fn a_market_switch_moves_the_button_chip_and_status_cell_together() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let before = surfaces_read(&mut app, &ctx, "TESTUSDT");
    assert_eq!(before, ["TESTUSDT \u{b7} Binance"; 3].map(str::to_owned));

    let directory = gateway_test_directory("tab-market-set");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.recover");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut cockpit = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.recover"]),
    );
    let tab = app.runtime.tabs.active_id().to_string();
    let input = json!({"tab": tab, "feed": "binance", "symbol": "ETHUSDT"});
    let generation = app.active_tab().link.feed_generation();
    let (first, _) = unkeyed_call(&mut app, &mut cockpit, "tab.market.set", input.clone());
    let result = success_result(&first);
    assert_eq!(
        (
            &result["changed"],
            &result["from_symbol"],
            &result["symbol"]
        ),
        (&json!(true), &json!("TESTUSDT"), &json!("ETHUSDT"))
    );
    assert_eq!(app.active_tab().link.feed_generation(), generation + 1);
    let after = surfaces_read(&mut app, &ctx, "ETHUSDT");
    assert_eq!(after, ["ETHUSDT \u{b7} Binance"; 3].map(str::to_owned));

    let (repeat, _) = unkeyed_call(&mut app, &mut cockpit, "tab.market.set", input);
    assert_eq!(success_result(&repeat)["changed"], false);
    assert_eq!(app.active_tab().link.feed_generation(), generation + 1);
    let invalid = json!({"tab": tab, "feed": "binance", "symbol": "NOT_LISTED"});
    let (refused, _) = unkeyed_call(&mut app, &mut cockpit, "tab.market.set", invalid);
    assert_eq!(error_code(&refused), Some(codes::INVALID_REQUEST));
    disable_test_gateway(&mut app, &ctx);
    let _ = std::fs::remove_dir_all(&directory);
}
