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
    desk_writes: Vec<bool>,
    closed: Vec<u64>,
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
    fn tab_symbols(&self, tabs: &[u64]) -> Vec<String> {
        tabs.iter().map(|tab| format!("SYM{tab}")).collect()
    }
    fn persist_workspace_autosave(&mut self, on: bool) -> Result<(), String> {
        if self.write_fails {
            return Err("saving is off".to_owned());
        }
        self.desk_writes.push(on);
        Ok(())
    }
    fn close_market_tab(&mut self, tab: u64) -> bool {
        if self.tabs.len() < 2 {
            return false;
        }
        self.tabs.retain(|open| *open != tab);
        self.closed.push(tab);
        true
    }
}

/// Two open tabs the book has adopted, autosave off so an edit is held.
fn parts() -> (FakeHost, FakeAccess, ActorContext) {
    let (mut host, access, actor) = autosaving();
    host.book.set_autosave(false);
    (host, access, actor)
}

/// Two open tabs on a book with autosave on, as a fresh desk opens.
fn autosaving() -> (FakeHost, FakeAccess, ActorContext) {
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    book.open_tab(2);
    let host = FakeHost {
        book,
        tabs: vec![1, 2],
        writes: 0,
        write_fails: false,
        shown: Vec::new(),
        desk_writes: Vec::new(),
        closed: Vec::new(),
    };
    (host, FakeAccess::with_budget(0), FakeWindow::assistant())
}

fn edit(host: &mut FakeHost, tab: u64) {
    let edited = WorkspaceRecipe {
        footprint_style: Some("heatmap".to_owned()),
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
    menu.open_rename(1, "Scalp");
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
    menu.open_rename(1, "The open");
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
    assert_eq!(detail(&stranger), "workspace.unknown_tab");
    assert_eq!(stranger.message, "no open tab has id 9");
    let value = json!({ "tab_id": "9" });
    let stranger = save_to(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(detail(&stranger), "workspace.unknown_tab");
    let gone = delete(&mut host, &mut access, &actor, &json!({ "name": "Nope" })).unwrap_err();
    assert_eq!(gone.code.as_str(), codes::INVALID_REQUEST);
    assert_eq!(detail(&gone), "workspace.unknown");
    assert_eq!(gone.message, "no workspace is called Nope");
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
fn delete_moves_tabs_to_default_keeps_edits_and_a_repeat_is_refused() {
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
    let journaled = access.events.len();
    let again = delete(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(detail(&again), "workspace.unknown");
    assert_eq!(access.events.len(), journaled, "a refusal journals nothing");
}

#[test]
fn rename_answers_the_cleaned_name_and_revert_is_refused() {
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
    let before = host.book.clone();
    let journaled = access.events.len();
    let refused = revert(&mut host, &mut access, &actor, &json!({})).unwrap_err();
    assert_eq!(detail(&refused), "workspace.revert_removed");
    assert!(
        refused.message.contains("workspace.save_to"),
        "{}",
        refused.message
    );
    assert!(
        refused.message.contains("workspace.autosave.set"),
        "{}",
        refused.message
    );
    assert_eq!(host.book, before, "the edit stays held");
    assert_eq!(access.events.len(), journaled);
}

#[test]
fn list_names_every_workspace_with_its_summary_and_tabs() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let rows = list(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(
        rows["workspaces"],
        json!([
            { "name": "Default", "summary": "as the feed opens", "tabs": ["2"] },
            { "name": "Scalp", "summary": "Flow", "tabs": ["1"] },
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
    edit(&mut host, 2);
    let value = json!({ "tab_id": "2", "save": false });
    close(&mut host, &mut access, &actor, &value).unwrap();
    autosave_set(&mut host, &mut access, &actor, &json!({ "on": true })).unwrap();
    let value = json!({ "name": "Scalp", "to": "Open" });
    rename(&mut host, &mut access, &actor, &value).unwrap();
    delete(&mut host, &mut access, &actor, &json!({ "name": "Open" })).unwrap();
    edit(&mut host, 1);
    new(&mut host, &mut access, &actor, &json!({ "name": "Blank" })).unwrap();
    // Revert only refuses now: its row names an event nothing journals.
    for row in READBACKS
        .iter()
        .filter(|row| row.capability != "workspace.revert")
    {
        let kind = row.event.unwrap();
        let field = row.field.strip_prefix("payload.").unwrap();
        assert!(
            access
                .events
                .iter()
                .filter(|event| event.kind.as_str() == kind)
                .all(|event| event.payload.get(field).is_some()),
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

/// New workspace asks the name first: the row opens the dialog and nothing
/// else, and Esc closes it with the book, the disk, the charts and the journal
/// as they were.
#[test]
fn new_workspace_esc_leaves_the_chart_untouched() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    let before = host.book.clone();
    let mut menu = WorkspaceMenuState::default();
    let note = apply_row_command(
        &mut host,
        Some((&mut access, &actor)),
        &mut menu,
        1,
        "WINV26",
        WorkspaceCommand::BeginNew,
    );
    assert_eq!(note, None);
    assert_eq!(menu.dialog.as_ref().unwrap().mode, NameDialogMode::New);
    menu.dialog.as_mut().unwrap().name = "Blank".to_owned();
    menu.close_dialog();
    assert_eq!(host.book, before, "the tab keeps its edits");
    assert!(host.shown.is_empty(), "no chart was redrawn");
    assert_eq!((host.writes, access.events.len()), (0, 0));
}

/// Enter makes the empty recipe, moves only that tab and says so; a refused
/// name keeps the dialog and changes nothing.
#[test]
fn new_workspace_applies_on_enter_and_a_taken_name_stays_in_the_dialog() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Scalp" })).unwrap();
    let mut menu = WorkspaceMenuState::default();
    let mut submit = |host: &mut FakeHost, menu: &mut WorkspaceMenuState, name: &str| {
        apply_row_command(
            host,
            Some((&mut access, &actor)),
            menu,
            1,
            "WINV26",
            WorkspaceCommand::BeginNew,
        );
        menu.dialog.as_mut().unwrap().name = name.to_owned();
        apply_row_command(
            host,
            Some((&mut access, &actor)),
            menu,
            1,
            "WINV26",
            WorkspaceCommand::SubmitDialog,
        )
    };
    let before = host.book.clone();
    assert_eq!(submit(&mut host, &mut menu, "scalp"), None);
    assert!(menu.dialog.as_ref().unwrap().refusal.is_some());
    assert_eq!(host.book, before);
    menu.close_dialog();
    let note = submit(&mut host, &mut menu, "Blank");
    assert_eq!(note, Some(text::created_toast("WINV26", "Blank")));
    assert!(menu.dialog.is_none());
    assert_eq!(
        host.book
            .tab_state(1)
            .map(|state| (state.workspace, state.modified)),
        Some(("Blank", false))
    );
    assert_eq!(host.book.tab_state(2).unwrap().workspace, DEFAULT_WORKSPACE);
    assert_eq!(host.shown, [1]);
}

/// `workspace.new` is not `workspace.save_as`: it copies no indicators, and a
/// repeat is refused with the name taken.
#[test]
fn the_new_call_makes_an_empty_recipe_and_a_repeat_is_refused() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    let made = new(&mut host, &mut access, &actor, &json!({ "name": "Blank" })).unwrap();
    assert_eq!(
        made,
        json!({ "tab_id": "1", "workspace": "Blank", "dropped_edits": true })
    );
    assert!(
        host.book
            .recipe("Blank")
            .unwrap()
            .flow_indicators
            .is_empty()
    );
    assert_eq!(host.writes, 1);
    let again = new(&mut host, &mut access, &actor, &json!({ "name": "Blank" })).unwrap_err();
    assert_eq!(detail(&again), "workspace.name_taken");
    assert_eq!((host.writes, access.events.len()), (1, 1));
}

/// A tab still opening has no arrangement to empty: the call is refused as
/// retryable, the way a tab the book has not met is.
#[test]
fn the_new_call_on_a_tab_still_opening_is_refused_retryably() {
    let (mut host, mut access, actor) = parts();
    let before = host.book.clone();
    let error = new(&mut host, &mut access, &actor, &json!({ "name": "Blank" })).unwrap_err();
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert!(error.retryable);
    assert_eq!(detail(&error), TAB_NOT_READY_CODE);
    assert_eq!(host.book, before);
    assert_eq!((host.writes, access.events.len()), (0, 0));
}

/// Turning autosave on writes every held edit at once and the desk once; a
/// repeat changes nothing.
#[test]
fn autosave_on_saves_the_pending_edits_and_a_repeat_is_no_change() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    assert!(host.book.tab_state(1).unwrap().modified);
    let on = autosave_set(&mut host, &mut access, &actor, &json!({ "on": true })).unwrap();
    assert_eq!(on["auto_save"], true);
    assert_eq!(on["changed"], true);
    assert_eq!(on["saved"][0]["workspace"], DEFAULT_WORKSPACE);
    assert_eq!(host.desk_writes, [true]);
    assert!(!host.book.differs(1), "the edit reached its workspace");
    assert!(!host.book.tab_state(1).unwrap().modified);
    let again = autosave_set(&mut host, &mut access, &actor, &json!({ "on": true })).unwrap();
    assert_eq!(
        (again["changed"].clone(), host.desk_writes.len()),
        (json!(false), 1)
    );
}

/// A desk write that fails takes the choice back and saves nothing.
#[test]
fn a_failed_desk_write_keeps_autosave_as_it_was() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    host.write_fails = true;
    autosave_set(&mut host, &mut access, &actor, &json!({ "on": true })).unwrap_err();
    assert!(!host.book.autosave());
    assert!(host.book.tab_state(1).unwrap().modified);
    assert!(access.events.is_empty());
}

/// With autosave on an edit never marks, and the write-through saves it.
#[test]
fn autosave_on_writes_through_and_never_marks() {
    let (mut host, mut access, actor) = autosaving();
    edit(&mut host, 1);
    assert!(!host.book.tab_state(1).unwrap().modified);
    assert!(host.book.unsaved_edits(1).is_none());
    let notes = write_through(&mut host, Some((&mut access, &actor)), &[1]);
    assert_eq!(
        notes,
        ["Default saved \u{b7} SYM2 follows"],
        "tab 2 follows"
    );
    assert!(!host.book.differs(1));
    assert_eq!(host.writes, 1);
    let value = json!({ "tab_id": "2", "name": "Default" });
    save_as(&mut host, &mut access, &actor, &json!({ "name": "Solo" })).unwrap();
    set(&mut host, &mut access, &actor, &value).unwrap();
    edit(&mut host, 1);
    let notes = write_through(&mut host, Some((&mut access, &actor)), &[1]);
    assert!(notes.is_empty(), "no toast without followers: {notes:?}");
}

/// `tab.close` with edits held and no flag refuses and keeps the tab; `save`
/// true writes then closes, false closes and the workspace keeps its recipe.
#[test]
fn tab_close_refuses_unsaved_edits_then_obeys_the_flag() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    let value = json!({ "tab_id": "1" });
    let refused = close(&mut host, &mut access, &actor, &value).unwrap_err();
    assert_eq!(detail(&refused), "workspace.unsaved_edits");
    let details = refused.context.details.clone().unwrap();
    assert_eq!(details["workspace"], DEFAULT_WORKSPACE);
    assert_eq!(details["followers"], json!(["SYM2"]));
    assert_eq!(host.tabs, [1, 2], "the tab stays open");
    assert!(access.events.is_empty());

    let value = json!({ "tab_id": "1", "save": true });
    let closed = close(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(closed["tab_id"], "1");
    assert_eq!(closed["saved"]["workspace"], DEFAULT_WORKSPACE);
    assert_eq!(host.closed, [1]);
    assert_eq!(host.writes, 1);
}

#[test]
fn tab_close_without_saving_leaves_the_workspace_as_it_was() {
    let (mut host, mut access, actor) = parts();
    edit(&mut host, 1);
    let before = host.book.recipe(DEFAULT_WORKSPACE).cloned();
    let value = json!({ "tab_id": "1", "save": false });
    let closed = close(&mut host, &mut access, &actor, &value).unwrap();
    assert_eq!(closed["saved"], Value::Null);
    assert_eq!(host.closed, [1]);
    assert_eq!(host.writes, 0);
    assert_eq!(host.book.recipe(DEFAULT_WORKSPACE).cloned(), before);
}

/// Autosave on: a close never asks; the pending edit is written first.
#[test]
fn tab_close_with_autosave_on_never_asks() {
    let (mut host, mut access, actor) = autosaving();
    edit(&mut host, 1);
    let closed = close(&mut host, &mut access, &actor, &json!({ "tab_id": "1" })).unwrap();
    assert_eq!(closed["tab_id"], "1");
    assert_eq!((host.closed.as_slice(), host.writes), ([1].as_slice(), 1));
}
