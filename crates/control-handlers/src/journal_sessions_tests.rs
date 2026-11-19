//! The journal's session acts through a fake window: what reaches the port,
//! what is journaled, and the refusal codes a caller branches on. The file
//! work itself is `quantick-paper`'s, tested there over real files.

use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow, PaperDesk};
use crate::trade::{PaperOwner, TradeDesk};
use quantick_civil::TzOffset;
use quantick_paper::report::session_edit::JournalScope;
use quantick_session::paper::SharedChange;

/// A window whose journal folder does not exist and whose edits answer what
/// the test set.
struct FakeJournal {
    dir: PathBuf,
    edits: Vec<JournalEdit>,
    answer: Result<JournalEditOutcome, SessionEditError>,
    /// The active tab's desk and its ledger; `None` when no tab holds one.
    desk: Option<PaperDesk>,
}

impl FakeJournal {
    fn answering(answer: Result<JournalEditOutcome, SessionEditError>) -> Self {
        Self {
            dir: PathBuf::from("no-such-trades-folder"),
            edits: Vec::new(),
            answer,
            desk: Some(PaperDesk::new(
                quantick_paper::PaperAccount::with_trades_dir(PathBuf::from(
                    "no-such-trades-folder",
                )),
            )),
        }
    }
}

impl PaperOwner for FakeJournal {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        self.desk.as_ref().map(|desk| desk as &dyn TradeDesk)
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        self.desk.as_mut().map(|desk| desk as &mut dyn TradeDesk)
    }
    fn persist(&mut self, _change: SharedChange) {}
    fn journal_scope(&self) -> JournalScope {
        JournalScope {
            dir: self.dir.clone(),
            live: Vec::new(),
        }
    }
    fn journal_timezone(&self) -> TzOffset {
        TzOffset::new(0)
    }
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError> {
        self.edits.push(edit.clone());
        self.answer.clone()
    }
}

fn session() -> SessionKey {
    SessionKey::parse("WINJ26/20260316-130000.csv").expect("a session key")
}

fn events() -> FakeAccess {
    FakeAccess::with_budget(0)
}

fn call(
    window: &mut FakeJournal,
    events: &mut FakeAccess,
    input: Value,
) -> Result<Value, ControlError> {
    journal_sessions(window, events, &FakeWindow::assistant(), &input)
}

fn delete(
    window: &mut FakeJournal,
    events: &mut FakeAccess,
    input: Value,
) -> Result<Value, ControlError> {
    journal_delete(window, events, &FakeWindow::assistant(), &input)
}

#[test]
fn a_rename_reaches_the_ledgers_edit_and_is_journaled_once() {
    let mut window = FakeJournal::answering(Ok(JournalEditOutcome::Renamed {
        session: session(),
        name: Some("Opening drive".to_owned()),
    }));
    let mut events = events();
    let input =
        json!({ "action": "rename", "session": session().to_string(), "name": "Opening drive" });
    let result = call(&mut window, &mut events, input).expect("renamed");
    assert_eq!(
        window.edits,
        vec![JournalEdit::Rename {
            session: session(),
            name: "Opening drive".to_owned()
        }]
    );
    assert_eq!(result["action"], "rename");
    assert_eq!(result["changed"], true);
    assert_eq!(events.events.len(), 1);
    assert_eq!(events.events[0].kind.as_str(), JOURNAL_SESSIONS_EVENT_KIND);
    assert_eq!(
        events.events[0].payload["journal_session"]["action"],
        "rename"
    );
}

#[test]
fn a_tag_names_its_trade_by_times_and_which_of_them() {
    let trade = TradeRef {
        opened_ms: 1,
        closed_ms: 2,
        nth: 1,
    };
    let mut window = FakeJournal::answering(Ok(JournalEditOutcome::Tagged {
        session: session(),
        trade,
        tags: vec!["msc".to_owned()],
    }));
    let input = json!({
        "action": "tag", "session": session().to_string(),
        "opened_unix_ms": 1, "closed_unix_ms": 2, "nth": 1, "tags": ["#MSC"],
    });
    let result = call(&mut window, &mut events(), input).expect("tagged");
    assert_eq!(result["action"], "tag");
    let first = json!({
        "action": "tag", "session": session().to_string(),
        "opened_unix_ms": 1, "closed_unix_ms": 2, "tags": [],
    });
    call(&mut window, &mut events(), first).expect("tagged");
    assert_eq!(
        window.edits,
        vec![
            JournalEdit::Tag {
                session: session(),
                trade,
                tags: vec!["#MSC".to_owned()]
            },
            JournalEdit::Tag {
                session: session(),
                trade: TradeRef { nth: 0, ..trade },
                tags: Vec::new()
            }
        ],
        "nth left out is the first trade with those times"
    );
}

#[test]
fn a_delete_is_its_own_capability_and_set_refuses_one() {
    let mut window = FakeJournal::answering(Ok(JournalEditOutcome::Deleted {
        session: session(),
        moved_to: PathBuf::from("no-such-trades-folder/.deleted/WINJ26/x.csv"),
    }));
    let mut events = events();
    let input = json!({ "action": "delete", "session": session().to_string() });
    let error = call(&mut window, &mut events, input).expect_err("not this capability's");
    assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
    assert!(window.edits.is_empty());

    let result = delete(
        &mut window,
        &mut events,
        json!({ "session": session().to_string() }),
    )
    .expect("deleted");
    assert_eq!(result["action"], "delete");
    assert_eq!(result["moved_to"], ".deleted/WINJ26/x.csv");
    assert_eq!(
        window.edits,
        vec![JournalEdit::Delete { session: session() }]
    );
    assert_eq!(events.events.len(), 1);

    let descriptor = delete_descriptor();
    assert!(!descriptor.reversible, "nothing in the contract undoes it");
    assert!(
        descriptor
            .required_permissions
            .iter()
            .any(|permission| permission.as_str() == "cockpit.journal.delete")
    );
    assert!(
        descriptor
            .risk_flags
            .iter()
            .any(|flag| flag.as_str() == "journal_session_removed")
    );
}

#[test]
fn broker_delete_reports_an_origin_relative_destination_in_the_event_and_reply() {
    let scratch = crate::scratch::ScratchDir::new("broker-delete-readback");
    let dir = scratch.path().join("paper");
    let key = session().for_broker();
    let path = key.path(&dir);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "# quantick-trades 2\n").unwrap();
    let scope = JournalScope {
        dir: dir.clone(),
        live: vec![],
    };
    let edit = JournalEdit::Delete {
        session: key.clone(),
    };
    let outcome = quantick_paper::report::session_edit::apply_journal_edit(&scope, &edit).unwrap();
    let mut window = FakeJournal::answering(Ok(outcome));
    window.dir = dir;
    let mut events = events();
    let result = delete(&mut window, &mut events, json!({"session": key.as_str()})).unwrap();
    assert_eq!(result["moved_to"], format!("broker/.deleted/{}", session()));
    assert_eq!(events.events[0].payload["journal_session"], result);
}

#[test]
fn deleting_the_live_session_is_refused_with_a_stable_code_and_journals_nothing() {
    let mut window = FakeJournal::answering(Err(SessionEditError::LiveSession(session())));
    let mut events = events();
    let input = json!({ "session": session().to_string() });
    let error = delete(&mut window, &mut events, input).expect_err("refused");
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert!(!error.retryable);
    assert_eq!(
        error.context.details.expect("details")["code"],
        "live_session"
    );
    assert!(!error.context.next_steps.is_empty());
    assert!(events.events.is_empty());
}

#[test]
fn a_second_delete_of_a_moved_session_is_a_stable_not_found() {
    let mut window = FakeJournal::answering(Err(SessionEditError::NotFound(session())));
    let input = json!({ "session": session().to_string() });
    for _ in 0..2 {
        let error = delete(&mut window, &mut events(), input.clone()).expect_err("gone");
        assert_eq!(error.code.as_str(), codes::RESOURCE_GONE);
        assert!(!error.retryable, "nothing to retry for");
        assert_eq!(
            error.context.details.expect("details")["code"],
            "session_not_found"
        );
    }
}

#[test]
fn a_session_that_is_not_a_listed_file_is_refused_before_the_window_is_asked() {
    let mut window = FakeJournal::answering(Err(SessionEditError::Io("unreached".to_owned())));
    let mut events = events();
    for session in [
        "../escape.csv",
        "/abs/x.csv",
        "WINJ26/x.txt",
        "WINJ26 /x.csv",
    ] {
        let error =
            delete(&mut window, &mut events, json!({ "session": session })).expect_err("refused");
        assert_eq!(error.code.as_str(), codes::INVALID_REQUEST, "{session}");
        assert_eq!(
            error.context.details.expect("details")["code"],
            "invalid_session"
        );
    }
    assert!(window.edits.is_empty());
}

#[test]
fn a_gone_session_and_a_failed_write_say_whether_a_retry_can_help() {
    for (answer, code, retryable) in [
        (
            SessionEditError::NotFound(session()),
            codes::RESOURCE_GONE,
            false,
        ),
        (
            SessionEditError::Io("disk full".to_owned()),
            codes::CAPABILITY_UNAVAILABLE,
            true,
        ),
        (
            SessionEditError::InvalidTag("bad".to_owned()),
            codes::INVALID_REQUEST,
            false,
        ),
    ] {
        let mut window = FakeJournal::answering(Err(answer.clone()));
        let input = json!({ "action": "rename", "session": session().to_string(), "name": "x" });
        let error = call(&mut window, &mut events(), input).expect_err("refused");
        assert_eq!(error.code.as_str(), code, "{answer:?}");
        assert_eq!(error.retryable, retryable, "{answer:?}");
        assert_eq!(
            error.context.details.expect("details")["code"],
            answer.code()
        );
    }
}

#[test]
fn a_listing_changes_nothing_and_a_bad_date_names_its_field() {
    let mut window = FakeJournal::answering(Err(SessionEditError::Io("unreached".to_owned())));
    let mut events = events();
    let result = call(
        &mut window,
        &mut events,
        json!({ "action": "list", "source": "replay", "from": "2026-03-16", "search": "#msc" }),
    )
    .expect("listed");
    assert_eq!(result["changed"], false);
    assert_eq!(result["sessions"], json!([]));
    assert_eq!(result["session_count"], "0");
    assert!(window.edits.is_empty() && events.events.is_empty());

    let error = call(
        &mut window,
        &mut events,
        json!({ "action": "list", "to": "16/03/2026" }),
    )
    .expect_err("refused");
    assert_eq!(error.context.details.expect("details")["field"], "to");
}

/// The filter row and the fold carets, by name: set, read back, journaled
/// only when something moved, and refused when no tab holds a ledger.
#[test]
fn the_ledger_view_is_set_and_read_by_name() {
    let mut window = FakeJournal::answering(Err(SessionEditError::Io("unreached".to_owned())));
    let mut events = events();
    let input = json!({
        "action": "view", "source": "replay", "from": "2026-03-16", "to": "2026-03-17",
        "search": "#msc", "fold": [{ "session": session().to_string(), "folded": true }],
    });
    let result = call(&mut window, &mut events, input.clone()).expect("set");
    assert_eq!(result["changed"], true);
    assert_eq!(
        result["view"],
        json!({
            "source": "replay", "from": "2026-03-16", "to": "2026-03-17", "search": "#msc",
            "folded_sessions": [session().to_string()],
        })
    );
    assert_eq!(events.events.len(), 1);

    let again = call(&mut window, &mut events, input).expect("set again");
    assert_eq!(again["changed"], false, "a retry moves nothing");
    let read = call(&mut window, &mut events, json!({ "action": "view" })).expect("read");
    assert_eq!(read["view"], result["view"]);
    assert_eq!(events.events.len(), 1, "neither journals");

    let cleared = call(
        &mut window,
        &mut events,
        json!({ "action": "view", "clear": true }),
    )
    .expect("cleared");
    assert_eq!(cleared["view"]["source"], "both");
    assert!(cleared["view"].get("from").is_none());
    assert_eq!(
        cleared["view"]["folded_sessions"],
        json!([session().to_string()]),
        "clear drops the filter, not the folds"
    );

    window.desk = None;
    let error = call(&mut window, &mut events, json!({ "action": "view" })).expect_err("none");
    assert_eq!(error.context.details.expect("details")["code"], "no_ledger");
    let error = call(
        &mut window,
        &mut events,
        json!({ "action": "view", "fold": [{ "session": "../x.csv", "folded": true }] }),
    )
    .expect_err("refused");
    assert_eq!(
        error.context.details.expect("details")["code"],
        "invalid_session"
    );
}

/// Both journal calls act on the trader's own trades journal, which no tape
/// owns: a replay's control trace never records them, and a trace that holds
/// one (written by a build from before the rule) never runs it.
#[test]
fn journal_edits_are_never_recorded_into_or_replayed_from_a_control_trace() {
    let mut registry =
        quantick_control_host::actions::ActionRegistry::<FakeJournal, FakeAccess>::new();
    register(&mut registry).unwrap();
    for capability in [
        quantick_control_schema::journal_sessions::JOURNAL_SESSIONS_CAPABILITY_ID,
        quantick_control_schema::journal_sessions::JOURNAL_DELETE_CAPABILITY_ID,
    ] {
        let action = registry.lookup(capability, 1).expect("the call is docked");
        assert!(!action.traced(false).unwrap(), "{capability}");
        let error = action.traced(true).unwrap_err();
        assert_eq!(
            error.code.as_str(),
            codes::CAPABILITY_UNAVAILABLE,
            "{capability}"
        );
    }
}
