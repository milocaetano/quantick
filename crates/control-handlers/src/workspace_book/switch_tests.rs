//! The switch that asks first, through the fake window: a row asks and each
//! answer does what it says, and the call refuses until its input decides.

use super::*;
use quantick_session::switch_prompt::SwitchAnswer;

/// Tab 1 on Scalp holding an edit of its own (a footprint style), tab 2 on
/// Default; autosave off, so the edit is held.
fn held_on_scalp() -> (FakeHost, FakeAccess, ActorContext) {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    edit(&mut host, 1);
    (host.writes, host.shown) = (0, Vec::new());
    access.events.clear();
    (host, access, actor)
}

fn row(
    host: &mut FakeHost,
    access: &mut FakeAccess,
    actor: &ActorContext,
    menu: &mut WorkspaceMenuState,
    command: WorkspaceCommand,
) -> Option<String> {
    apply_row_command(host, Some((access, actor)), menu, 1, "SYM1", command)
}

/// A row switching a tab that holds edits asks, naming the switch, and
/// moves nothing: no write, no journal row, the edit still held.
#[test]
fn a_row_switch_that_would_drop_edits_asks_first() {
    let (mut host, mut access, actor) = held_on_scalp();
    let before = host.book.clone();
    let mut menu = WorkspaceMenuState::default();
    let set = WorkspaceCommand::Set(DEFAULT_WORKSPACE.to_owned());
    assert_eq!(row(&mut host, &mut access, &actor, &mut menu, set), None);
    let asked = menu.switch.question().expect("a question is up");
    assert_eq!((asked.tab, asked.workspace.as_str()), (1, "Scalp"));
    assert_eq!(asked.target, DEFAULT_WORKSPACE);
    assert_eq!(host.book, before, "nothing moved");
    assert_eq!((host.writes, access.events.len()), (0, 0));
    let prompt = menu_prompt(&menu).expect("the window shows it");
    assert_eq!(prompt.title, text::switch_prompt_title("SYM1", "Default"));
    assert_eq!(
        prompt.buttons,
        [
            text::CLOSE_CANCEL.to_owned(),
            text::SWITCH_WITHOUT_SAVING.to_owned(),
            text::save_to_short_label("Scalp"),
        ]
    );
    for (at, answer) in SwitchAnswer::BUTTONS.into_iter().enumerate() {
        let command = Some(WorkspaceCommand::AnswerSwitch(answer));
        assert_eq!(prompt_answer(&menu, at), command, "button {at}");
    }
}

/// Cancel changes nothing and takes the question down.
#[test]
fn cancel_on_the_switch_prompt_changes_nothing() {
    let (mut host, mut access, actor) = held_on_scalp();
    let mut menu = WorkspaceMenuState::default();
    let set = WorkspaceCommand::Set(DEFAULT_WORKSPACE.to_owned());
    row(&mut host, &mut access, &actor, &mut menu, set);
    let before = host.book.clone();
    let cancel = WorkspaceCommand::AnswerSwitch(SwitchAnswer::Cancel);
    assert_eq!(row(&mut host, &mut access, &actor, &mut menu, cancel), None);
    assert_eq!(menu.switch.question(), None);
    assert_eq!(host.book, before);
    assert_eq!(
        (host.writes, access.events.len(), host.shown.len()),
        (0, 0, 0)
    );
}

/// Switch without saving drops the edit, says so, and leaves Scalp as saved.
#[test]
fn switch_without_saving_drops_the_edit_and_says_so() {
    let (mut host, mut access, actor) = held_on_scalp();
    let mut menu = WorkspaceMenuState::default();
    let set = WorkspaceCommand::Set(DEFAULT_WORKSPACE.to_owned());
    row(&mut host, &mut access, &actor, &mut menu, set);
    let go = WorkspaceCommand::AnswerSwitch(SwitchAnswer::SwitchWithoutSaving);
    let note = row(&mut host, &mut access, &actor, &mut menu, go);
    assert_eq!(note, Some(text::switched_toast("SYM1", "Default", true)));
    assert_eq!(host.book.tab_state(1).unwrap().workspace, DEFAULT_WORKSPACE);
    assert_eq!(scalp_style(&host), None, "Scalp as saved");
    assert_eq!(host.shown, [1]);
}

/// Save writes the edit into Scalp, then switches: nothing is dropped.
#[test]
fn save_on_the_switch_prompt_writes_the_edit_then_switches() {
    let (mut host, mut access, actor) = held_on_scalp();
    let mut menu = WorkspaceMenuState::default();
    let set = WorkspaceCommand::Set(DEFAULT_WORKSPACE.to_owned());
    row(&mut host, &mut access, &actor, &mut menu, set);
    let save = WorkspaceCommand::AnswerSwitch(SwitchAnswer::Save);
    let note = row(&mut host, &mut access, &actor, &mut menu, save);
    assert_eq!(note, Some(text::switched_toast("SYM1", "Default", false)));
    assert_eq!(scalp_style(&host).as_deref(), Some("heatmap"));
    assert_eq!(host.book.tab_state(1).unwrap().workspace, DEFAULT_WORKSPACE);
    let kinds: Vec<_> = rows(&access).into_iter().map(|row| row.1).collect();
    assert_eq!(
        kinds,
        [WORKSPACE_SAVED_EVENT_KIND, TAB_WORKSPACE_SET_EVENT_KIND]
    );
}

/// A tab holding nothing a switch drops switches at once, unasked.
#[test]
fn a_clean_tab_switches_without_a_question() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let mut menu = WorkspaceMenuState::default();
    let set = WorkspaceCommand::Set(DEFAULT_WORKSPACE.to_owned());
    let note = row(&mut host, &mut access, &actor, &mut menu, set);
    assert_eq!(note, Some(text::switched_toast("SYM1", "Default", false)));
    assert_eq!(menu.switch.question(), None);
    assert_eq!(menu_prompt(&menu), None);
}

/// A close's question stands over a switch's.
#[test]
fn a_close_question_comes_before_a_switch_question() {
    let (mut host, mut access, actor) = held_on_scalp();
    let mut menu = WorkspaceMenuState::default();
    let set = WorkspaceCommand::Set(DEFAULT_WORKSPACE.to_owned());
    row(&mut host, &mut access, &actor, &mut menu, set);
    let close = WorkspaceCommand::Close(CloseScope::Tabs, vec![1]);
    row(&mut host, &mut access, &actor, &mut menu, close);
    let prompt = menu_prompt(&menu).expect("a question is up");
    assert_eq!(prompt.title, text::close_prompt_title("SYM1"));
    let answer = Some(WorkspaceCommand::AnswerClose(CloseAnswer::Cancel));
    assert_eq!(prompt_answer(&menu, 0), answer);
}

/// `tab.workspace.set` never drops edits unasked and never writes the book:
/// without `edits` it refuses, naming the workspace and the target, and
/// changes nothing; `discard` drops them; `save` is no answer it takes, as
/// writing the book is `workspace.save_to`'s, behind the desk's consent.
#[test]
fn the_call_refuses_until_told_to_discard_and_never_saves() {
    let (mut host, mut access, actor) = held_on_scalp();
    let before = host.book.clone();
    for value in [
        json!({ "tab_id": "1", "name": DEFAULT_WORKSPACE }),
        json!({ "tab_id": "1", "name": DEFAULT_WORKSPACE, "edits": "save" }),
    ] {
        let refused = set(&mut host, &mut access, &actor, &value).unwrap_err();
        assert_eq!(host.book, before, "nothing changed: {value}");
        assert_eq!((host.writes, access.events.len()), (0, 0));
        if value.get("edits").is_none() {
            assert_eq!(detail(&refused), UNSAVED_EDITS_CODE);
            let details = refused.context.details.as_ref().unwrap();
            assert_eq!(details["workspace"], "Scalp");
            assert_eq!(details["target"], DEFAULT_WORKSPACE);
        }
    }
    let value = json!({ "tab_id": "1", "name": DEFAULT_WORKSPACE, "edits": "discard" });
    let moved = set(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(moved["dropped_edits"], true);
    assert_eq!(scalp_style(&host), None, "Scalp as saved");
    assert_eq!(host.book.tab_state(1).unwrap().workspace, DEFAULT_WORKSPACE);
    assert_eq!(host.writes, 0, "the call wrote nothing");
}

/// A question gone stale carries out nothing and says so: the tab left the
/// workspace it named by another route, so a Save would write a workspace
/// it never named; or the target is gone, so the switch could not follow.
#[test]
fn an_answer_to_a_stale_question_does_nothing_and_says_so() {
    let stale = Some(text::SWITCH_QUESTION_STALE.to_owned());
    let moved = |host: &mut FakeHost| {
        host.book.save_as(1, "Moved").unwrap();
    };
    let gone = |host: &mut FakeHost| {
        host.book.delete("Swing").unwrap();
    };
    for (target, change) in [
        (DEFAULT_WORKSPACE, &moved as &dyn Fn(&mut FakeHost)),
        ("Swing", &gone),
    ] {
        let (mut host, mut access, actor) = held_on_scalp();
        host.book.save_as(2, "Swing").unwrap();
        let mut menu = WorkspaceMenuState::default();
        let set = WorkspaceCommand::Set(target.to_owned());
        row(&mut host, &mut access, &actor, &mut menu, set);
        assert!(menu.switch.question().is_some(), "{target}: asked");
        change(&mut host);
        let before = host.book.clone();
        let save = WorkspaceCommand::AnswerSwitch(SwitchAnswer::Save);
        let note = row(&mut host, &mut access, &actor, &mut menu, save);
        assert_eq!(note, stale, "{target}");
        assert_eq!(menu.switch.question(), None, "the question is down");
        assert_eq!(host.book, before, "{target}: nothing saved or switched");
        assert_eq!((host.writes, access.events.len()), (0, 0));
    }
}
