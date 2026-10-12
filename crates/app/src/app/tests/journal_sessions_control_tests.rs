//! The trades journal's sessions through the real gateway: list through the
//! ledger's filter, rename, tag, delete — the live session refused — and the
//! ledger re-reading what the call wrote.
use super::*;

const CAPABILITY: &str = "session.journal.set";

fn journal_call(app: &mut QuantickApp, client: &mut LocalClient, input: Value) -> ResponseEnvelope {
    unkeyed_call(app, client, CAPABILITY, input).0
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

    let deleted = success_result(&journal_call(
        &mut app,
        &mut client,
        json!({ "action": "delete", "session": second }),
    ));
    assert!(
        deleted["moved_to"]
            .as_str()
            .is_some_and(|to| to.starts_with(".deleted/WINJ26/")),
        "{deleted}"
    );
    let gone = journal_call(
        &mut app,
        &mut client,
        json!({ "action": "delete", "session": second }),
    );
    assert_eq!(
        error_code(&gone),
        Some(codes::RESOURCE_GONE),
        "a repeat removes nothing"
    );

    // The name and the tags are in the file: a fresh read — a restart —
    // sees them.
    let reread = quantick_paper::report::load_history(trades.path(), None, &[]);
    assert_eq!(reread.sessions.len(), 1);
    assert_eq!(
        reread
            .rows
            .iter()
            .filter(|row| !row.tags.is_empty())
            .count(),
        1
    );
    assert_eq!(
        reread
            .sessions
            .values()
            .next()
            .and_then(|info| info.name.as_deref()),
        Some("Opening drive")
    );

    // Every edit journaled one event a client can reconcile against.
    let events = readback(&mut app, &ctx, &mut client, CAPABILITY);
    assert_eq!(
        events.len(),
        3,
        "rename, tag and delete; the listings journal nothing"
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
    let session = quantick_paper::report::sessions::SessionKey::of(trades.path(), &live)
        .expect("a session key")
        .to_string();

    let directory = gateway_test_directory("journal-sessions-live");
    grant_annotate_for_test(&mut app, "all-reads,cockpit,cockpit.layout");
    enable_test_gateway(&mut app, &ctx, &directory, 4);
    let mut client = connect(
        &directory,
        &options("cockpit", &["cockpit", "cockpit.layout"]),
    );
    let refused = journal_call(
        &mut app,
        &mut client,
        json!({ "action": "delete", "session": session }),
    );
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
