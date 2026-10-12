//! The trades journal's sessions through the real gateway: list through the
//! ledger's filter, rename, tag, set the ledger's view, delete — behind its
//! own permission, the live session refused — and the ledger re-reading what
//! the call wrote, and only then.
use super::*;

use quantick_paper::report::session_edit::JournalEdit;
use quantick_paper::report::sessions::SessionKey;

const CAPABILITY: &str = "session.journal.set";
const DELETE: &str = "session.journal.delete";
/// The cockpit grant plus the one a delete needs.
const DELETE_SCOPES: &[&str] = &["cockpit", "cockpit.layout", "cockpit.journal.delete"];

fn journal_call(app: &mut QuantickApp, client: &mut LocalClient, input: Value) -> ResponseEnvelope {
    unkeyed_call(app, client, CAPABILITY, input).0
}

fn delete_call(app: &mut QuantickApp, client: &mut LocalClient, session: &str) -> ResponseEnvelope {
    unkeyed_call(app, client, DELETE, json!({ "session": session })).0
}

/// One replay session file the way the account writes it.
fn write_session(dir: &crate::scratch::ScratchDir, file: &str, closes: &[i64]) -> String {
    let mut text =
        quantick_sim::history::write_header("WINJ26", quantick_sim::history::SessionSource::Replay);
    for &closed_ms in closes {
        text.push_str(&format!(
            "{},{closed_ms},long,1,100,103,3,manual,,,,\n",
            closed_ms - 1_000
        ));
    }
    dir.write(format!("WINJ26/{file}"), &text);
    format!("WINJ26/{file}")
}

#[test]
fn sessions_are_listed_named_tagged_and_deleted_by_name() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let trades = crate::scratch::ScratchDir::new("journal-sessions-control");
    app.active_tab_mut()
        .paper
        .account_mut()
        .set_trades_dir(trades.path().to_path_buf());
    // 2026-03-16 13:00 UTC; the same tape traded twice.
    let close = 1_773_666_000_000_i64;
    let first = write_session(&trades, "20260316-130000.csv", &[close, close + 60_000]);
    let second = write_session(&trades, "20260316-130000.rerun-1.csv", &[close]);

    let directory = gateway_test_directory("journal-sessions-control");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );

    let listed = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({ "action": "list" }),
    ));
    assert_eq!(
        listed["session_count"], "2",
        "two runs of one tape are two sessions"
    );
    assert_eq!(listed["changed"], false);

    let renamed = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({ "action": "rename", "session": first, "name": "Opening drive" }),
    ));
    assert_eq!(renamed["session"]["name"], "Opening drive");
    let tagged = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({
            "action": "tag", "session": first,
            "opened_unix_ms": close - 1_000, "closed_unix_ms": close, "tags": ["#MSC"],
        }),
    ));
    assert_eq!(
        tagged["session"]["tagged_trades"][0]["tags"],
        json!(["msc"])
    );

    // `#msc` lists only the session holding the tagged trade, and counts
    // the trades it kept out.
    let msc = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({ "action": "list", "search": "#msc" }),
    ));
    assert_eq!(msc["session_count"], "1");
    assert_eq!(msc["sessions"][0]["session"], first);
    assert_eq!(msc["sessions"][0]["trade_count"], "1");
    assert_eq!(msc["hidden_trades"], "2");
    let named = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({ "action": "list", "search": "opening" }),
    ));
    assert_eq!(
        named["session_count"], "1",
        "words match the session's name"
    );

    // A delete is not this capability's, and the cockpit grant alone does
    // not reach the one that does it.
    let not_here = journal_call(
        &mut app,
        &mut client,
        json!({ "action": "delete", "session": second }),
    );
    assert_eq!(error_code(&not_here), Some(codes::INVALID_REQUEST));
    let ungranted = delete_call(&mut app, &mut client, &second);
    assert_eq!(error_code(&ungranted), Some(codes::PERMISSION_DENIED));
    assert!(trades.path().join(&second).is_file(), "nothing moved");

    // Every edit journaled one event a client can reconcile against.
    let events = readback(&mut app, &ctx, &mut client, CAPABILITY);
    assert_eq!(
        events.len(),
        2,
        "rename and tag; the listings journal nothing"
    );

    // The name and the tags are in the file: a fresh read — a restart —
    // sees them.
    let reread = quantick_paper::report::load_history(trades.path(), None, &[]);
    assert_eq!(
        reread
            .rows
            .iter()
            .filter(|row| !row.tags.is_empty())
            .count(),
        1
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn a_delete_needs_its_own_permission_and_a_repeat_removes_nothing() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let trades = crate::scratch::ScratchDir::new("journal-sessions-delete");
    app.active_tab_mut()
        .paper
        .account_mut()
        .set_trades_dir(trades.path().to_path_buf());
    let session = write_session(&trades, "20260316-130000.csv", &[1_773_666_000_000]);

    let directory = gateway_test_directory("journal-sessions-delete");
    grant_annotate_for_test(
        &mut app,
        "all-reads,cockpit,cockpit.layout,cockpit.journal.delete",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(&directory, &options("cockpit", DELETE_SCOPES));
    let deleted = success_result(&delete_call(&mut app, &mut client, &session));
    assert!(
        deleted["moved_to"]
            .as_str()
            .is_some_and(|to| to.starts_with(".deleted/WINJ26/")),
        "{deleted}"
    );
    let gone = delete_call(&mut app, &mut client, &session);
    assert_eq!(
        error_code(&gone),
        Some(codes::RESOURCE_GONE),
        "a repeat removes nothing"
    );
    assert_eq!(readback(&mut app, &ctx, &mut client, DELETE).len(), 1);
    assert!(
        quantick_paper::report::load_history(trades.path(), None, &[])
            .sessions
            .is_empty()
    );
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

#[test]
fn the_session_a_tab_is_writing_cannot_be_deleted() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let trades = crate::scratch::ScratchDir::new("journal-sessions-live");
    let paper = app.active_tab_mut().paper.account_mut();
    paper.set_trades_dir(trades.path().to_path_buf());
    paper.set_symbol("WINJ26");
    let print = |agg_id: u64| quantick_engine::Trade {
        agg_id,
        timestamp_ms: 1_773_666_000_000 + i64::try_from(agg_id).expect("small id") * 1_000,
        price: rust_decimal::Decimal::from(100),
        quantity: rust_decimal::Decimal::ONE,
        side: quantick_engine::Side::Buy,
    };
    paper.seed(&print(1));
    paper.apply_sim_command_for_tests(quantick_sim::Command::PlaceMarket {
        side: quantick_engine::Side::Buy,
        quantity: rust_decimal::Decimal::ONE,
        bracket: Default::default(),
    });
    paper.on_trade(&print(2));
    paper.reset_timeline();
    let live = paper
        .session_journal_paths()
        .first()
        .cloned()
        .expect("the close was journaled");
    let session = SessionKey::of(trades.path(), &live)
        .expect("a session key")
        .to_string();

    let directory = gateway_test_directory("journal-sessions-live");
    grant_annotate_for_test(
        &mut app,
        "all-reads,cockpit,cockpit.layout,cockpit.journal.delete",
    );
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(&directory, &options("cockpit", DELETE_SCOPES));
    let refused = delete_call(&mut app, &mut client, &session);
    assert_eq!(error_code(&refused), Some(codes::CAPABILITY_UNAVAILABLE));
    assert!(live.is_file(), "the live session is untouched");
    let listed = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({ "action": "list" }),
    ));
    assert_eq!(listed["sessions"][0]["live"], true);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// The filter row and a session's fold caret, by name: what the call sets is
/// what the ledger holds, read back by a second call.
#[test]
fn the_ledgers_filter_and_folds_are_set_and_read_by_name() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let directory = gateway_test_directory("journal-sessions-view");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let set = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({
            "action": "view", "source": "replay", "from": "2026-03-16", "search": "#msc",
            "fold": [{ "session": "WINJ26/20260316-130000.csv", "folded": true }],
        }),
    ));
    assert_eq!(set["changed"], true);
    let read = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({ "action": "view" }),
    ));
    assert_eq!(read["changed"], false);
    assert_eq!(
        read["view"],
        json!({
            "source": "replay", "from": "2026-03-16", "to": "2026-03-16", "search": "#msc",
            "folded_sessions": ["WINJ26/20260316-130000.csv"],
        })
    );
    assert_eq!(readback(&mut app, &ctx, &mut client, CAPABILITY).len(), 1);
    disable_test_gateway(&mut app, &ctx);
    std::fs::remove_dir_all(directory).ok();
}

/// A refused edit changed nothing on disk, so no ledger throws away its read
/// of the folder; a done one has every ledger read it again.
#[test]
fn only_an_edit_that_happened_makes_the_ledgers_read_again() {
    let ctx = egui::Context::default();
    let (mut app, _commands) = app_with_history(4);
    run_frame(&mut app, &ctx);
    let trades = crate::scratch::ScratchDir::new("journal-sessions-refused");
    app.active_tab_mut()
        .paper
        .account_mut()
        .set_trades_dir(trades.path().to_path_buf());
    let session = write_session(&trades, "20260316-130000.csv", &[1_773_666_000_000]);
    let load = |app: &mut QuantickApp| {
        let (report, env) = app.active_tab_mut().paper.account_mut().report_parts();
        report.reload_ledger(&env);
    };
    let loaded = |app: &QuantickApp| app.active_tab().paper.report_state().ledger_loaded();
    load(&mut app);
    let missing = JournalEdit::Rename {
        session: SessionKey::parse("WINJ26/missing.csv").expect("a key"),
        name: "x".to_owned(),
    };
    let refused = crate::app::paper_wiring::apply_journal_edit_across(&mut app.tabs, &missing);
    assert!(refused.is_err());
    assert!(loaded(&app), "nothing changed, nothing to re-read");
    let rename = JournalEdit::Rename {
        session: SessionKey::parse(&session).expect("a key"),
        name: "x".to_owned(),
    };
    let done = crate::app::paper_wiring::apply_journal_edit_across(&mut app.tabs, &rename);
    assert!(done.is_ok());
    assert!(
        !loaded(&app),
        "the file changed: the next frame re-reads it"
    );
}

/// Del with the pointer on the dock is the dock's — the Trades ledger deletes
/// a session with it — so the selected drawing survives it, on the very
/// frame the pointer arrives; with the pointer back on the chart, Del deletes
/// the drawing again. Nothing is claimed from one frame for the next.
#[test]
fn delete_over_the_dock_never_deletes_the_selected_drawing() {
    let (mut app, _commands) = app_with_history(200);
    let ctx = egui::Context::default();
    app.dock.open_tab(crate::dock::DockTab::Trades);
    run_frame(&mut app, &ctx);
    app.toolrail
        .arm(Tool::Drawing(drawing_tool("horizontal-line")));
    let chart = egui::pos2(500.0, 300.0);
    click_chart(&mut app, &ctx, chart);
    assert!(app.active_tab().flow_pane.drawings.selected().is_some());
    // Inside the Trades tab's body, which hugs the window's right edge.
    let dock = egui::pos2(TEST_WINDOW.x - 120.0, 450.0);
    let press_at = |app: &mut QuantickApp, at: egui::Pos2| {
        let events = vec![egui::Event::PointerMoved(at), key_press(egui::Key::Delete)];
        run_frame_with_events(app, &ctx, events);
    };
    press_at(&mut app, dock);
    assert!(app.dock.holds_pointer(&ctx), "the press landed on the dock");
    assert_eq!(
        app.active_tab().flow_pane.drawings.items().len(),
        1,
        "Del on the dock left the drawing alone"
    );
    assert!(app.active_tab().flow_pane.drawings.selected().is_some());
    press_at(&mut app, chart);
    assert!(
        app.active_tab().flow_pane.drawings.items().is_empty(),
        "Del on the chart is the chart's"
    );
}

/// One trace line as the action door writes it: the intent, or with
/// `done` the result that closes it.
fn trace_line(capability: &str, input: Value, done: bool) -> String {
    let entry = quantick_control_host::trace::TraceEntry {
        trace_version: quantick_control_host::trace::TRACE_VERSION,
        replay_elapsed_ms: 0,
        sequence: quantick_control::wire::WireU64::new(1),
        actor_kind: quantick_control::wire::ActorKind::HumanUi,
        client_name: "quantick-ui".to_owned(),
        capability_id: quantick_control::id::CapabilityId::new(capability).expect("an id"),
        capability_version: 1,
        canonical_input: input,
        expected_revisions: Vec::new(),
        result_code: done.then(|| quantick_control::id::ErrorCode::new("control.ok").unwrap()),
        result_digest: None,
    };
    format!(
        "{}\n",
        serde_json::to_string(&entry).expect("an entry encodes")
    )
}

/// A journal edit made while a replay plays is the trader's, not the tape's:
/// the replay's control trace never records it, and a trace that holds one —
/// written before this rule — never runs it. Replaying a tape renames, tags
/// and deletes nothing by itself.
#[test]
fn journal_edits_are_never_recorded_into_or_replayed_from_a_control_trace() {
    use quantick_feed::replay::test_support as replay_test_support;
    let ctx = egui::Context::default();
    let tape = crate::scratch::ScratchDir::new("journal-trace-tape");
    let trades = crate::scratch::ScratchDir::new("journal-trace-trades");
    let session = write_session(&trades, "20260316-130000.csv", &[1_773_666_000_000]);
    let replaying = |app: &mut QuantickApp| {
        let recording = recording_at(tape.path());
        app.active_tab_mut().replay = Some(replay_test_support::detached_link(recording));
        app.active_tab_mut()
            .paper
            .account_mut()
            .set_trades_dir(trades.path().to_path_buf());
    };
    let sidecar = crate::control::replay_trace_path_for(&recording_at(tape.path()).path);

    let (mut app, _commands) = app_with_history(12);
    replaying(&mut app);
    run_frame(&mut app, &ctx);
    let rename = json!({ "action": "rename", "session": session, "name": "Mine" });
    let human = crate::control::ActionOrigin::Human;
    app.control_action(CAPABILITY, 1, human, rename)
        .expect("a rename during a replay is done");
    let recorded = std::fs::read_to_string(&sidecar).unwrap_or_default();
    assert!(
        !recorded.contains(CAPABILITY),
        "the trace recorded a journal edit: {recorded}"
    );
    drop(app);

    // A trace an older build wrote, holding a delete of that session.
    let delete = json!({ "session": session });
    let lines = trace_line(DELETE, delete.clone(), false) + &trace_line(DELETE, delete, true);
    std::fs::write(&sidecar, lines).expect("the sidecar is written");
    let (mut app, _commands) = app_with_history(12);
    replaying(&mut app);
    for _ in 0..3 {
        run_frame(&mut app, &ctx);
    }
    assert!(
        trades.path().join(&session).is_file(),
        "replaying the tape deleted the trader's session"
    );
}
