//! The journal's session acts through a fake window: what reaches the port,
//! what is journaled, and the refusal codes a caller branches on. The file
//! work itself is `quantick-paper`'s, tested there over real files.

use std::path::PathBuf;

use serde_json::json;

use super::*;
use crate::test_support::FakeWindow;

/// A window whose journal folder does not exist and whose edits answer what
/// the test set.
struct FakeJournal {
    edits: Vec<JournalEdit>,
    answer: Result<JournalEditOutcome, SessionEditError>,
}

impl FakeJournal {
    fn answering(answer: Result<JournalEditOutcome, SessionEditError>) -> Self {
        Self {
            edits: Vec::new(),
            answer,
        }
    }
}

impl JournalSessionsPort for FakeJournal {
    fn journal_scope(&mut self) -> JournalScope {
        JournalScope {
            dir: PathBuf::from("no-such-trades-folder"),
            live: Vec::new(),
        }
    }
    fn journal_timezone(&mut self) -> TzOffset {
        TzOffset::new(0)
    }
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError> {
        self.edits.push(edit.clone());
        self.answer.clone()
    }
}

#[derive(Default)]
struct Events(Vec<NewEvent>);

impl JournalEventAccess for Events {
    fn record_event(&mut self, event: NewEvent) {
        self.0.push(event);
    }
}

fn session() -> SessionKey {
    SessionKey::parse("WINJ26/20260316-130000.csv").expect("a session key")
}

fn call(
    window: &mut FakeJournal,
    events: &mut Events,
    input: Value,
) -> Result<Value, ControlError> {
    journal_sessions(window, events, &FakeWindow::assistant(), &input)
}

#[test]
fn a_rename_reaches_the_ledgers_edit_and_is_journaled_once() {
    let mut window = FakeJournal::answering(Ok(JournalEditOutcome::Renamed {
        session: session(),
        name: Some("Opening drive".to_owned()),
    }));
    let mut events = Events::default();
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
    assert_eq!(events.0.len(), 1);
    assert_eq!(events.0[0].kind.as_str(), JOURNAL_SESSIONS_EVENT_KIND);
    assert_eq!(events.0[0].payload["journal_session"]["action"], "rename");
}

#[test]
fn a_tag_passes_the_trade_times_and_tags_through() {
    let mut window = FakeJournal::answering(Ok(JournalEditOutcome::Tagged {
        session: session(),
        times: (1, 2),
        tags: vec!["msc".to_owned()],
    }));
    let mut events = Events::default();
    let input = json!({
        "action": "tag", "session": session().to_string(),
        "opened_unix_ms": 1, "closed_unix_ms": 2, "tags": ["#MSC"],
    });
    call(&mut window, &mut events, input).expect("tagged");
    assert_eq!(
        window.edits,
        vec![JournalEdit::Tag {
            session: session(),
            times: (1, 2),
            tags: vec!["#MSC".to_owned()]
        }]
    );
}

#[test]
fn deleting_the_live_session_is_refused_with_a_stable_code_and_journals_nothing() {
    let mut window = FakeJournal::answering(Err(SessionEditError::LiveSession(session())));
    let mut events = Events::default();
    let input = json!({ "action": "delete", "session": session().to_string() });
    let error = call(&mut window, &mut events, input).expect_err("refused");
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert!(!error.retryable);
    assert_eq!(
        error.context.details.expect("details")["code"],
        "live_session"
    );
    assert!(!error.context.next_steps.is_empty());
    assert!(events.0.is_empty());
}

#[test]
fn a_session_that_is_not_a_listed_file_is_refused_before_the_window_is_asked() {
    let mut window = FakeJournal::answering(Err(SessionEditError::Io("unreached".to_owned())));
    let mut events = Events::default();
    for session in ["../escape.csv", "/abs/x.csv", "WINJ26/x.txt"] {
        let input = json!({ "action": "delete", "session": session });
        let error = call(&mut window, &mut events, input).expect_err("refused");
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
        let error = call(&mut window, &mut Events::default(), input).expect_err("refused");
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
    let mut events = Events::default();
    let result = call(
        &mut window,
        &mut events,
        json!({ "action": "list", "source": "replay", "from": "2026-03-16", "search": "#msc" }),
    )
    .expect("listed");
    assert_eq!(result["changed"], false);
    assert_eq!(result["sessions"], json!([]));
    assert_eq!(result["session_count"], "0");
    assert!(window.edits.is_empty() && events.0.is_empty());

    let error = call(
        &mut window,
        &mut events,
        json!({ "action": "list", "to": "16/03/2026" }),
    )
    .expect_err("refused");
    assert_eq!(error.context.details.expect("details")["field"], "to");
}
