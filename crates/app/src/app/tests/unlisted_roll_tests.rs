//! A contract the venue no longer lists rolls to the one its feed config names
//! next: offered on the notice popup, and taken by itself on a restored tab.
use super::*;
use quantick_feed::FeedNotice;

/// What the MetaTrader bridge's `BRIDGE_SYMBOL_NOT_FOUND` reaches a tab as.
fn unlisted(symbol: &str) -> FeedNotice {
    FeedNotice::attention(
        format!("MetaTrader does not list {symbol}"),
        "Add the contract to Market Watch.",
    )
    .with_unlisted(Some(symbol.to_owned()))
}

/// A tab on TESTUSDT whose feed config rolls it to ETHUSDT.
fn rolling_app() -> (QuantickApp, mpsc::Receiver<FeedCommand>) {
    let (mut app, commands) = app_with_history(4);
    app.config.feeds[0].contract_rolls =
        std::collections::BTreeMap::from([("TESTUSDT".to_owned(), "ETHUSDT".to_owned())]);
    (app, commands)
}

/// The popup open on the active tab, over `notice`, settled for a click.
fn popup_over(app: &mut QuantickApp, ctx: &egui::Context, notice: FeedNotice) -> egui::FullOutput {
    app.active_tab_mut().link.notice = notice;
    app.chrome.feed_popup_tab = Some(app.runtime.tabs.active_id());
    for _ in 0..3 {
        run_frame(app, ctx);
    }
    run_frame(app, ctx)
}

/// The active tab's notice as `feed.status` publishes it.
fn notice_view(app: &QuantickApp) -> serde_json::Value {
    let mut registry = crate::control::standard_registry().unwrap();
    let scopes = [observer_scope("feed.status")];
    let capture = registry
        .capture(app, &observer_instance(), &scopes)
        .unwrap()
        .into_serialized()
        .unwrap();
    capture.scopes[&scopes[0]].value["tabs"][0]["notice"].clone()
}

/// The status bar's lead cell for the active tab.
fn status_lead(app: &QuantickApp) -> crate::statusbar::StatusLead<'_> {
    crate::statusbar::StatusLead::of(app.active_tab(), None, 0, false)
}

fn click(at: egui::Pos2) -> Vec<egui::Event> {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    vec![egui::Event::PointerMoved(at), press(true), press(false)]
}

#[test]
fn the_popup_offers_the_roll_and_its_press_lands_on_the_next_contract() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    run_frame(&mut app, &ctx);
    let output = popup_over(&mut app, &ctx, unlisted("TESTUSDT"));
    assert_eq!(
        app.active_tab().active.1,
        "TESTUSDT",
        "a tab opened by hand waits for the press"
    );
    let at = painted_text_center(&output, "Roll to ETHUSDT").expect("the roll is offered first");
    assert!(
        painted_text_center(&output, "Reconnect").is_some()
            && painted_text_center(&output, "Reload").is_some(),
        "both acts stay reachable under it"
    );
    // Every surface offers the same roll: the chip beside the status word,
    // the empty chart's control, and the control plane.
    let roll = crate::feed_notice::NoticeAction::Roll("ETHUSDT".to_owned());
    assert_eq!(app.active_tab().link.quick_action(), roll);
    assert_eq!(status_lead(&app).action, roll);
    let view = notice_view(&app);
    assert_eq!(view["unlisted_symbol"], "TESTUSDT");
    assert_eq!(view["roll_to"], "ETHUSDT");
    assert!(view.get("roll_refused").is_none());
    run_frame_with_events(&mut app, &ctx, click(at));
    // The press rolls on the next frame's settle, beside the drawings.
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert_eq!(
        app.surfaces.toast.message(),
        Some("TESTUSDT is no longer listed; rolled to ETHUSDT")
    );
    assert_eq!(
        app.chrome.feed_popup_tab, None,
        "a pressed control closes it"
    );
    assert_eq!(
        status_lead(&app).note.as_deref(),
        Some("TESTUSDT is no longer listed; rolled to ETHUSDT"),
        "the explanation outlives the toast"
    );
}

/// The scene's feed-status actions, as listed.
fn feed_status_entries(app: &QuantickApp) -> Vec<serde_json::Value> {
    let scene = observer_scene(app);
    scene["controls"]
        .as_array()
        .expect("the scene reports a control list")
        .iter()
        .filter(|control| control["capability_id"].is_string())
        .filter(|control| control["owner"]["id"] == "feed_status")
        .cloned()
        .collect()
}

/// The scene's feed-status actions: id, label and the capability each calls.
fn feed_status_actions(app: &QuantickApp) -> Vec<(String, String, String)> {
    feed_status_entries(app)
        .iter()
        .map(|control| {
            let text = |key: &str| control[key].as_str().unwrap().to_owned();
            (text("control_id"), text("label"), text("capability_id"))
        })
        .collect()
}

/// While the chip offers the roll, the control plane names the roll, not
/// Reconnect; the popup lists it ahead of both recoveries.
#[test]
fn the_scene_names_the_roll_the_chip_offers() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    run_frame(&mut app, &ctx);
    app.active_tab_mut().link.notice = unlisted("TESTUSDT");
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    let roll = crate::feed_notice::NoticeAction::Roll("ETHUSDT".to_owned());
    assert_eq!(status_lead(&app).action, roll, "the chip offers the roll");
    let named = |id: &str, label: &str, capability: &str| {
        (id.to_owned(), label.to_owned(), capability.to_owned())
    };
    let roll = named("feed_status.roll", "Roll to ETHUSDT", "feed.contract.roll");
    assert_eq!(feed_status_actions(&app), std::slice::from_ref(&roll));

    popup_over(&mut app, &ctx, unlisted("TESTUSDT"));
    assert_eq!(
        feed_status_actions(&app),
        [
            roll,
            named("feed_status.reconnect", "Reconnect", "feed.reconnect"),
            named("feed_status.reload", "Reload", "feed.reload"),
        ]
    );
}

#[test]
fn no_successor_keeps_today_s_notice_and_its_reconnect() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let output = popup_over(&mut app, &ctx, unlisted("TESTUSDT"));
    assert!(painted_text_center(&output, "Reconnect").is_some());
    assert!(painted_text_center(&output, "Reload").is_some());
    assert!(
        !painted_text(&output)
            .iter()
            .any(|text| text.starts_with("Roll to")),
        "{:?}",
        painted_text(&output)
    );
}

#[test]
fn a_refused_roll_keeps_reconnect_and_says_why() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    let scratch = crate::scratch::ScratchDir::new("unlisted-roll-refused");
    app.active_tab_mut()
        .paper
        .redirect_history_dir(scratch.path().to_path_buf());
    run_frame(&mut app, &ctx);
    app.apply_toolbar_action(ToolbarAction::PaperBuy);
    assert!(!app.active_tab().paper.is_flat(), "a position is open");
    let output = popup_over(&mut app, &ctx, unlisted("TESTUSDT"));
    assert!(painted_text_center(&output, "Reconnect").is_some());
    assert!(
        painted_text(&output).iter().any(|text| text.contains(
            "Cannot roll to the next contract: Close the paper position and cancel working orders first."
        )),
        "{:?}",
        painted_text(&output)
    );
    assert!(
        !painted_text(&output)
            .iter()
            .any(|text| text.starts_with("Roll to"))
    );
    let reason = "Close the paper position and cancel working orders first";
    let lead = status_lead(&app);
    let hover = lead.report.and_then(|report| report.next_step_text());
    assert!(
        hover.is_some_and(|step| step.contains(reason)),
        "the status word's hover says why too"
    );
    assert_eq!(lead.action, crate::feed_notice::NoticeAction::Reconnect);
    assert_eq!(notice_view(&app)["roll_refused"], reason);

    // The scene lists the refused roll the popup explains, ahead of the pair,
    // as a roll that cannot run and why.
    let entries = feed_status_entries(&app);
    let ids: Vec<_> = entries.iter().map(|entry| &entry["control_id"]).collect();
    assert_eq!(
        ids,
        [
            "feed_status.roll",
            "feed_status.reconnect",
            "feed_status.reload"
        ]
    );
    assert_eq!(entries[0]["capability_id"], "feed.contract.roll");
    assert_eq!(entries[0]["availability"]["available"], false);
    assert_eq!(
        entries[0]["availability"]["reason"],
        "feed.contract.paper_flat"
    );
    assert_eq!(
        entries[0]["description"],
        format!("Cannot roll to the next contract: {reason}.")
    );
}

/// Restore (or import) a saved workspace of one TESTUSDT tab.
fn restore_testusdt(app: &mut QuantickApp) {
    app.arrangement_adapter()
        .restore_workspace(ui_state::Workspace::new(
            true,
            None,
            0,
            vec![ui_state::SavedTab {
                candle_styles: Vec::new(),
                feed: "binance".to_owned(),
                symbol: "TESTUSDT".to_owned(),
                layout: crate::config::DeclaredLayout::TimeAndFlow,
                split_fraction: Some(0.4),
                context_collapsed: false,
                flow_collapsed: false,
                context_heights: vec![],
                context_panes_collapsed: vec![],
                focus: Some(ui_state::SavedFocus::Flow),
                focus_slot: 0,
                context_bars: vec![],
                flow_layout: None,
                context_layouts: vec![],
                flow_bars: "dollar:250000".to_owned(),
                time_bars: Some("time:5m".to_owned()),
                flow_legend_collapsed: false,
                time_legend_collapsed: false,
                workspace: None,
                workspace_copy: None,
                orderflow_panel: Default::default(),
            }],
            None,
        ));
}

#[test]
fn a_restored_tab_rolls_itself_once_and_says_so() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    restore_testusdt(&mut app);
    assert!(app.active_tab().link.auto_roll.armed_for("TESTUSDT"));
    app.active_tab_mut().link.notice = unlisted("TESTUSDT");
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert_eq!(
        app.surfaces.toast.message(),
        Some("TESTUSDT is no longer listed; rolled to ETHUSDT")
    );
    // The explanation stays on the tab, not alarming, and is readable on the
    // control plane until a later notice replaces it.
    assert_eq!(
        status_lead(&app).note.as_deref(),
        Some("TESTUSDT is no longer listed; rolled to ETHUSDT")
    );
    let view = notice_view(&app);
    assert_eq!(view["kind"], "clear", "the new feed's notice is its own");
    assert_eq!(view["rolled_from"], "TESTUSDT");
    assert!(
        view.get("unlisted_symbol").is_none(),
        "nothing to roll from"
    );
    run_frame(&mut app, &ctx);
    assert!(status_lead(&app).note.is_some(), "it is no toast");

    // The same contract reported again rolls nothing further.
    app.config.feeds[0]
        .contract_rolls
        .insert("ETHUSDT".to_owned(), "TESTUSDT".to_owned());
    app.active_tab_mut().link.notice = unlisted("ETHUSDT");
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "TESTUSDT", "one step per report");
    app.active_tab_mut().link.notice = unlisted("TESTUSDT");
    run_frame(&mut app, &ctx);
    assert_eq!(
        app.active_tab().active.1,
        "TESTUSDT",
        "a configured cycle does not loop: TESTUSDT was decided already"
    );
}

/// The roll's note and the feed's notice are two slots: the new feed's
/// progress and trouble reach the status bar and `feed.status` while the
/// note stands beside them, until the next switch takes it away.
#[test]
fn the_roll_note_stands_beside_the_notice_until_the_next_switch() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    restore_testusdt(&mut app);
    app.active_tab_mut().link.notice = unlisted("TESTUSDT");
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    let note = Some("TESTUSDT is no longer listed; rolled to ETHUSDT");
    for (notice, kind) in [
        (
            FeedNotice::working("waiting for a MetaTrader bridge on 127.0.0.1:5555"),
            "working",
        ),
        (FeedNotice::Clear, "clear"),
        (FeedNotice::working("loading ETHUSDT history"), "working"),
        (
            FeedNotice::attention("MetaTrader 5 is not running", "Start it."),
            "attention",
        ),
    ] {
        app.active_tab_mut().link.notice = notice.clone();
        run_frame(&mut app, &ctx);
        assert_eq!(
            app.active_tab().link.notice,
            notice,
            "the notice is the feed's"
        );
        assert_eq!(status_lead(&app).note.as_deref(), note, "the note stands");
        let view = notice_view(&app);
        assert_eq!(
            (view["kind"].as_str(), &view["rolled_from"]),
            (Some(kind), &serde_json::json!("TESTUSDT"))
        );
    }
    let lead = status_lead(&app);
    assert!(
        lead.report.is_some(),
        "the attention is said beside the note"
    );

    app.active_tab_mut().link.notice = FeedNotice::Clear;
    app.active_tab_mut().symbol = "TESTUSDT".to_owned();
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "TESTUSDT", "a hand took it back");
    assert_eq!(
        status_lead(&app).note,
        None,
        "the next switch takes it away"
    );
    assert!(notice_view(&app).get("rolled_from").is_none());
}

/// A bundle imported mid-session adopts a tab that is already streaming: the
/// contract is listed, so a later report offers the roll and waits for a hand.
#[test]
fn an_import_does_not_license_a_tab_that_already_connected() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    run_frame(&mut app, &ctx);
    app.active_tab_mut()
        .link
        .set_connection(quantick_feed::FeedConnectionState::Connected, 0);
    restore_testusdt(&mut app);
    assert!(!app.active_tab().link.auto_roll.is_armed());
    app.active_tab_mut().link.notice = unlisted("TESTUSDT");
    run_frame(&mut app, &ctx);
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "TESTUSDT", "no roll by itself");
    assert_eq!(
        app.active_tab().link.quick_action(),
        crate::feed_notice::NoticeAction::Roll("ETHUSDT".to_owned()),
        "the roll is offered"
    );
}

#[test]
fn a_feed_that_connected_waits_for_a_hand() {
    let (mut app, notices, _feed_ends) = test_app_with_notices();
    app.active_tab_mut().link.auto_roll.arm("TESTUSDT");
    notices.blocking_send(FeedNotice::Connected).unwrap();
    app.active_tab_mut().link.drain_notices_at(0);
    assert!(
        !app.active_tab().link.auto_roll.is_armed(),
        "a contract that streamed is listed; a later report gets the popup, not a roll"
    );
}

#[test]
fn a_hand_switch_takes_a_restored_tab_over() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    run_frame(&mut app, &ctx);
    app.active_tab_mut().link.auto_roll.arm("TESTUSDT");
    app.active_tab_mut().symbol = "ETHUSDT".to_owned();
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert!(
        !app.active_tab().link.auto_roll.is_armed(),
        "the trader picked the contract; it waits for a press from here on"
    );
}

/// `feed.contract.roll` as an agent calls it, and the journal it left.
fn roll_by_agent(app: &mut QuantickApp) -> (serde_json::Value, Vec<serde_json::Value>) {
    let mut access = crate::control::ControlAccess::new();
    let actor = crate::app::control_host::tests::fake::FakeWindow::assistant();
    let input = serde_json::json!({
        "tab_id": app.runtime.tabs.active_id().to_string(),
        "from_symbol": "TESTUSDT",
    });
    let result =
        crate::control::invoke_action(app, &mut access, &actor, "feed.contract.roll", input)
            .expect("the roll runs");
    let journal = access.journal().read(1, 256, 1 << 20).events;
    let payloads = journal.into_iter().map(|event| event.payload).collect();
    (result, payloads)
}

/// An agent's roll off the contract the feed reports unlisted — the scene's
/// `feed_status.roll` — says what the notice's Roll says: the status bar's
/// note, `feed.status`'s `rolled_from`, and a journaled `trigger`. A roll off
/// a listed contract is the plain calendar roll, with neither.
#[test]
fn an_agent_roll_off_an_unlisted_contract_leaves_the_notice_s_note() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = rolling_app();
    run_frame(&mut app, &ctx);
    app.active_tab_mut().link.notice = unlisted("TESTUSDT");
    run_frame(&mut app, &ctx);
    let (result, journal) = roll_by_agent(&mut app);
    assert_eq!(result["symbol"], "ETHUSDT");
    run_frame(&mut app, &ctx);
    assert_eq!(app.active_tab().active.1, "ETHUSDT");
    assert_eq!(
        status_lead(&app).note.as_deref(),
        Some("TESTUSDT is no longer listed; rolled to ETHUSDT")
    );
    assert_eq!(notice_view(&app)["rolled_from"], "TESTUSDT");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0]["trigger"], "capability");
    assert_eq!(journal[0]["contract_roll"], result);

    let (mut app, _commands) = rolling_app();
    run_frame(&mut app, &ctx);
    let (result, journal) = roll_by_agent(&mut app);
    assert_eq!(result["symbol"], "ETHUSDT");
    run_frame(&mut app, &ctx);
    assert_eq!(
        status_lead(&app).note,
        None,
        "a listed contract says nothing"
    );
    assert!(notice_view(&app).get("rolled_from").is_none());
    assert!(journal[0].get("trigger").is_none());
}
