//! The workspace calls through a fake window: changes reach the book and the
//! charts once, a repeat answers no change, a failed write leaves nothing.

use quantick_control_schema::workspace_book::READBACKS;
use quantick_stores::workspaces::DEFAULT_WORKSPACE;
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};

struct FakeHost {
    book: WorkspaceBook,
    tabs: Vec<u64>,
    writes: usize,
    write_fails: bool,
    shown: Vec<u64>,
}

impl TabDirectory for FakeHost {
    fn active_tab_index(&self) -> usize {
        0
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tabs.iter().position(|tab| *tab == tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tabs[index]
    }
}

impl WorkspaceBookHost for FakeHost {
    fn workspace_book(&mut self) -> Option<&mut WorkspaceBook> {
        Some(&mut self.book)
    }
    fn persist_workspace_book(&mut self) -> Result<(), String> {
        if self.write_fails {
            return Err("saving is off".to_owned());
        }
        self.writes += 1;
        Ok(())
    }
    fn show_tab_recipe(&mut self, tab: u64, _recipe: &WorkspaceRecipe) {
        self.shown.push(tab);
    }
}

/// Two open tabs the book has adopted.
fn parts() -> (FakeHost, FakeAccess, ActorContext) {
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    book.open_tab(2);
    let host = FakeHost {
        book,
        tabs: vec![1, 2],
        writes: 0,
        write_fails: false,
        shown: Vec::new(),
    };
    (host, FakeAccess::with_budget(0), FakeWindow::assistant())
}

fn edit(host: &mut FakeHost, tab: u64) {
    let edited = WorkspaceRecipe {
        context_bars: vec!["time:1m".to_owned()],
        ..WorkspaceRecipe::default()
    };
    host.book.observe(tab, &edited).unwrap();
}

/// A tab the window opened but the book has not adopted: its panes are still
/// being built, and a workspace put on it now would be undone by the desk.
#[test]
fn a_tab_not_yet_adopted_is_refused_retryably_and_nothing_changes() {
    let (mut host, mut access, actor) = parts();
    host.tabs.push(3);
    let before = host.book.clone();
    let value = json!({ "tab_id": "3", "name": "Default" });
    let error = set(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert!(error.retryable);
    assert_eq!(detail(&error), TAB_NOT_READY_CODE);
    let value = json!({ "tab_id": "3", "name": "Scalp" });
    save_as(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(host.book, before, "the book never met the tab");
    assert!(host.book.tab_state(3).is_none());
    assert_eq!((host.writes, access.events.len()), (0, 0));
}

/// Tab ids travel as strings in results, so a journal reader gets the same.
#[test]
fn journal_rows_carry_tab_ids_as_strings() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let value = json!({ "tab_id": "2", "name": "Scalp" });
    set(&mut host, &mut access, &actor, &value).unwrap();
    edit(&mut host, 1);
    save_to(&mut host, &mut access, &actor, &json!({})).unwrap();
    host.book.observe(1, &WorkspaceRecipe::default()).unwrap();
    revert(&mut host, &mut access, &actor, &json!({})).unwrap();
    delete(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let field = |kind: &str, name: &str| {
        access
            .events
            .iter()
            .find(|event| event.kind.as_str() == kind)
            .map(|event| event.payload[name].clone())
            .unwrap()
    };
    assert_eq!(field(TAB_WORKSPACE_SET_EVENT_KIND, "tab_id"), json!("2"));
    assert_eq!(field(WORKSPACE_REVERTED_EVENT_KIND, "tab_id"), json!("1"));
    assert_eq!(
        field(WORKSPACE_DELETED_EVENT_KIND, "moved"),
        json!(["1", "2"])
    );
    let saves: Vec<_> = access
        .events
        .iter()
        .filter(|event| event.kind.as_str() == WORKSPACE_SAVED_EVENT_KIND)
        .map(|event| event.payload["tabs_changed"].clone())
        .collect();
    assert_eq!(saves, [json!([]), json!(["2"])]);
}

/// The dialog, the call and the toast read one cleaned name; a rename to the
/// same name closes the dialog and says nothing.
#[test]
fn the_rename_dialog_toasts_the_stored_name_and_nothing_for_a_no_op() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let mut menu = WorkspaceMenuState::default();
    menu.open_rename("Scalp");
    menu.dialog.as_mut().unwrap().name = "  The   open ".to_owned();
    let note = apply_row_command(
        &mut host,
        Some((&mut access, &actor)),
        &mut menu,
        1,
        "WINV26",
        WorkspaceCommand::SubmitDialog,
    );
    assert_eq!(note, Some(text::renamed_toast("Scalp", "The open")));
    assert!(menu.dialog.is_none());
    menu.open_rename("The open");
    menu.dialog.as_mut().unwrap().name = " The open".to_owned();
    let note = apply_row_command(
        &mut host,
        Some((&mut access, &actor)),
        &mut menu,
        1,
        "WINV26",
        WorkspaceCommand::SubmitDialog,
    );
    assert_eq!(note, None, "no toast for a rename that moved nothing");
    assert!(menu.dialog.is_none());
}

/// The journal rows a run left, as module, kind and payload.
fn rows(access: &FakeAccess) -> Vec<(String, String, Value)> {
    let row = |event: &NewEvent| {
        let module = event.module_id.as_str().to_owned();
        (
            module,
            event.kind.as_str().to_owned(),
            event.payload.clone(),
        )
    };
    access.events.iter().map(row).collect()
}

/// A hand on the `⋮` rows and a script on the calls leave one journal: a
/// menu switch is `tab.workspace.set`'s row and a menu delete
/// `workspace.delete`'s, payload for payload.
#[test]
fn a_menu_set_and_delete_journal_the_rows_the_calls_do() {
    let (mut by_call, mut call_access, actor) = parts();
    save_as(
        &mut by_call,
        &mut call_access,
        &actor,
        &json!({ "name": "Scalp" }),
    )
    .unwrap();
    let value = json!({ "tab_id": "2", "name": "Scalp" });
    set(&mut by_call, &mut call_access, &actor, &value).unwrap();
    delete(
        &mut by_call,
        &mut call_access,
        &actor,
        &json!({ "name": "Scalp" }),
    )
    .unwrap();

    let (mut by_hand, mut hand_access, hand) = parts();
    save_as(
        &mut by_hand,
        &mut hand_access,
        &hand,
        &json!({ "name": "Scalp" }),
    )
    .unwrap();
    let mut menu = WorkspaceMenuState::default();
    for command in [
        WorkspaceCommand::Set("Scalp".to_owned()),
        WorkspaceCommand::Delete("Scalp".to_owned()),
    ] {
        let journal = Some((&mut hand_access, &hand));
        let note = apply_row_command(&mut by_hand, journal, &mut menu, 2, "WDOV26", command);
        assert!(note.is_some(), "each row changed something and says so");
    }

    let kinds: Vec<_> = rows(&call_access).into_iter().map(|row| row.1).collect();
    assert_eq!(
        kinds,
        [
            WORKSPACE_SAVED_EVENT_KIND,
            TAB_WORKSPACE_SET_EVENT_KIND,
            WORKSPACE_DELETED_EVENT_KIND
        ]
    );
    assert_eq!(rows(&hand_access), rows(&call_access));
    assert_eq!(by_hand.book, by_call.book, "and the same book after");
}

/// A window without a control identity has no journal; the row still lands.
#[test]
fn a_menu_row_without_a_journal_still_changes_the_book() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let mut menu = WorkspaceMenuState::default();
    let none: Journal<'_, FakeAccess> = None;
    let command = WorkspaceCommand::Set("Scalp".to_owned());
    apply_row_command(&mut host, none, &mut menu, 2, "WDOV26", command);
    assert_eq!(host.book.tab_state(2).unwrap().workspace, "Scalp");
    assert_eq!(access.events.len(), 1, "only the call's save");
}

fn detail(error: &ControlError) -> String {
    error.context.details.as_ref().unwrap()["code"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn save_as_then_set_moves_the_tabs_and_a_repeat_is_no_change() {
    let (mut host, mut access, actor) = parts();
    let value = json!({ "name": "Scalp" });
    let saved = save_as(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(saved["workspace"], "Scalp");
    assert_eq!(saved["changed"], true);
    assert_eq!(host.writes, 1);
    assert_eq!(access.events.len(), 1);
    let again = save_as(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(again["changed"], false);
    assert_eq!((host.writes, access.events.len()), (1, 1));

    let value = json!({ "tab_id": "2", "name": "Scalp" });
    let moved = set(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(moved["changed"], true);
    assert_eq!(host.shown, [2]);
    let again = set(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(again["changed"], false);
    assert_eq!(host.shown, [2], "a repeat redraws nothing");
    assert_eq!(host.writes, 1, "tab state lives in the desk");
}

#[test]
fn save_to_answers_the_tabs_it_changed_and_they_are_redrawn() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let value = json!({ "tab_id": "2", "name": "Scalp" });
    set(&mut host, &mut access, &actor, &value).unwrap();
    host.shown.clear();

    edit(&mut host, 1);
    let saved = save_to(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(saved["workspace"], "Scalp");
    assert_eq!(saved["tabs_changed"], json!(["2"]));
    assert_eq!(host.shown, [2]);
    let again = save_to(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(again["changed"], false);
    assert_eq!(again["tabs_changed"], json!([]));
}

#[test]
fn a_failed_write_restores_the_book_journals_nothing_and_says_why() {
    let (mut host, mut access, actor) = parts();
    host.write_fails = true;
    let value = json!({ "name": "Scalp" });
    let error = save_as(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert_eq!(detail(&error), WRITE_FAILED_CODE);
    assert!(error.message.contains("saving is off"));
    assert!(access.events.is_empty());
    assert!(
        host.book.recipe("Scalp").is_none(),
        "memory matches the file"
    );

    host.write_fails = false;
    let retried = save_as(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(retried["changed"], true);
}

#[test]
fn bad_names_are_refused_with_a_stable_code_and_the_reason() {
    let (mut host, mut access, actor) = parts();
    let empty = save_as(&mut host, &mut access, &actor, &json!({ "name": " " })).unwrap_err();
    assert_eq!(empty.code.as_str(), codes::INVALID_REQUEST);
    assert_eq!(detail(&empty), "workspace.name_empty");
    assert_eq!(empty.message, "a workspace needs a name");
    let taken = save_as(
        &mut host,
        &mut access,
        &actor,
        &json!({ "name": "default" }),
    )
    .unwrap_err();
    assert_eq!(detail(&taken), "workspace.name_taken");
    let unknown = set(&mut host, &mut access, &actor, &json!({ "name": "Nope" })).unwrap_err();
    assert_eq!(detail(&unknown), "workspace.unknown");
    let value = json!({ "tab_id": "9", "name": "Default" });
    let stranger = set(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(stranger.code.as_str(), codes::INVALID_REQUEST);
    assert_eq!(host.writes, 0);
    assert!(access.events.is_empty());
}

#[test]
fn default_cannot_be_deleted_or_renamed() {
    let (mut host, mut access, actor) = parts();
    let value = json!({ "name": DEFAULT_WORKSPACE });
    let error = delete(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(detail(&error), "workspace.default_protected");
    assert_eq!(error.message, "Default cannot be deleted or renamed");
    let value = json!({ "name": DEFAULT_WORKSPACE, "to": "Other" });
    let error = rename(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(detail(&error), "workspace.default_protected");
}

#[test]
fn delete_moves_tabs_to_default_keeps_edits_and_a_repeat_is_no_change() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let value = json!({ "tab_id": "2", "name": "Scalp" });
    set(&mut host, &mut access, &actor, &value).unwrap();
    edit(&mut host, 2);
    host.shown.clear();

    let value = json!({ "name": "Scalp" });
    let gone = delete(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(gone["moved"], json!(["1", "2"]));
    assert_eq!(gone["kept_edits"], json!(["2"]));
    assert_eq!(host.shown, [1], "the tab with edits stays as it is");
    let edited = host.book.tab_state(2).unwrap();
    assert_eq!((edited.workspace, edited.modified), ("Default", true));
    let again = delete(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(again["changed"], false);
}

#[test]
fn rename_answers_the_cleaned_name_and_revert_drops_edits_once() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let value = json!({ "name": "Scalp", "to": " The  open " });
    let moved = rename(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(moved["workspace"], "The open");
    assert_eq!(moved["changed"], true);
    let value = json!({ "name": "The open", "to": "The open" });
    let same = rename(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(same["changed"], false);

    edit(&mut host, 1);
    let reverted = revert(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(reverted["changed"], true);
    assert_eq!(reverted["workspace"], "The open");
    assert_eq!(host.shown, [1]);
    let again = revert(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(again["changed"], false);
}

#[test]
fn list_names_every_workspace_with_its_tabs() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let rows = list(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(
        rows["workspaces"],
        json!([
            { "name": "Default", "tabs": ["2"] },
            { "name": "Scalp", "tabs": ["1"] },
        ])
    );
}

#[test]
fn every_journal_row_reads_a_field_the_event_carries() {
    let (mut host, mut access, actor) = parts();
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let value = json!({ "tab_id": "2", "name": "Scalp" });
    set(&mut host, &mut access, &actor, &value).unwrap();
    edit(&mut host, 1);
    save_to(&mut host, &mut access, &actor, &json!({})).unwrap();
    host.book.observe(2, &WorkspaceRecipe::default()).unwrap();
    let value = json!({ "tab_id": "2" });
    revert(&mut host, &mut access, &actor, &value).unwrap();
    let value = json!({ "name": "Scalp", "to": "Open" });
    rename(&mut host, &mut access, &actor, &value).unwrap();
    delete(&mut host, &mut access, &actor, &json!({ "name": "Open" })).unwrap();
    for row in READBACKS {
        let kind = row.event.unwrap();
        assert!(
            access
                .events
                .iter()
                .filter(|event| event.kind.as_str() == kind)
                .all(|event| event.payload.get("workspace").is_some()),
            "{kind}"
        );
        assert!(
            access
                .events
                .iter()
                .any(|event| event.kind.as_str() == kind),
            "{kind} was never journaled"
        );
    }
}
