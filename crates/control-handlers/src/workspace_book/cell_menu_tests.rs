//! The cell menu's rows: their order, words, what each asks and the call that
//! does the same, read off a book as the window would draw them.

use quantick_control_schema::workspace_book as calls;
use quantick_stores::workspaces::{WorkspaceBook, WorkspaceRecipe};

use super::*;

/// One tab on `Default`, autosave as asked.
fn book(autosave: bool) -> WorkspaceBook {
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    book.set_autosave(autosave);
    book
}

fn keys(rows: &[CellRow]) -> Vec<&str> {
    rows.iter().map(|row| row.key.as_str()).collect()
}

#[test]
fn the_rows_keep_the_designed_order_and_name_their_calls() {
    let menu = cell_menu(&book(true), 1, "Ctrl+S");
    let kinds: Vec<RowKind> = menu.head.iter().map(|row| row.kind).collect();
    use RowKind::{Caption, Press, Separator, Switch};
    assert_eq!(
        kinds,
        [
            Switch, Caption, Separator, Press, Press, Press, Separator, Caption
        ]
    );
    assert_eq!(
        keys(&menu.head),
        ["autosave", "", "", "save", "save_as", "rename", "", ""]
    );
    assert_eq!(menu.head[1].label, "Every change is saved");
    assert_eq!(menu.head[3].label, "Save \u{201c}Default\u{201d}");
    assert_eq!(menu.head[3].beside, "Ctrl+S");
    assert_eq!(menu.head[7].label, "WORKSPACES");
    assert_eq!(keys(&menu.foot), ["", "new", "manage"]);
    let calls_of =
        |rows: &[CellRow]| -> Vec<Option<&str>> { rows.iter().map(|row| row.capability).collect() };
    assert_eq!(
        calls_of(&menu.head)[..1],
        [Some(calls::WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID)]
    );
    assert_eq!(
        calls_of(&menu.foot)[1..],
        [
            Some(calls::WORKSPACE_NEW_CAPABILITY_ID),
            Some(calls::WORKSPACE_LIST_CAPABILITY_ID)
        ]
    );
    assert_eq!(CellRow::menu().scene_id(), SCENE_ID);
    assert_eq!(menu.head[3].scene_id(), "workspace_menu.save");
    assert_eq!(CellRow::menu().capability, Some(MENU_CAPABILITY_ID));
}

#[test]
fn autosave_on_greys_save_and_flips_the_switch() {
    let menu = cell_menu(&book(true), 1, "Ctrl+S");
    let (switch, save) = (&menu.head[0], &menu.head[3]);
    assert!(switch.on);
    assert_eq!(switch.command, Some(WorkspaceCommand::SetAutosave(false)));
    assert_eq!(save.unavailable, Some(AUTOSAVE_SAVES_REASON));
    assert!(!save.dot);
    let rename = &menu.head[5];
    assert_eq!(rename.unavailable, Some(DEFAULT_PROTECTED_REASON));
    assert!(rename.disabled_hover.is_some());
}

#[test]
fn a_held_edit_lights_save_and_its_dot() {
    let mut book = book(false);
    let edited = WorkspaceRecipe {
        footprint_style: Some("heatmap".to_owned()),
        ..WorkspaceRecipe::default()
    };
    book.observe(1, &edited).unwrap();
    let menu = cell_menu(&book, 1, "Ctrl+S");
    assert_eq!(menu.head[1].label, "Save with Ctrl+S");
    let save = &menu.head[3];
    assert!(save.unavailable.is_none() && save.dot);
    assert_eq!(save.command, Some(WorkspaceCommand::SaveTo));
}

#[test]
fn every_workspace_is_a_row_the_tab_s_own_lit() {
    let menu = cell_menu(&book(true), 1, "Ctrl+S");
    assert_eq!(menu.list.len(), book(true).list().len());
    let default = &menu.list[0];
    assert_eq!(default.key, "workspace.Default");
    assert!(default.on);
    assert_eq!(
        default.command,
        Some(WorkspaceCommand::Set("Default".to_owned()))
    );
    assert_eq!(
        default.capability,
        Some(calls::TAB_WORKSPACE_SET_CAPABILITY_ID)
    );
}

#[test]
fn the_window_menu_s_action_rows_are_the_cell_menu_s() {
    let book = book(false);
    let menu = cell_menu(&book, 1, "");
    let new = action_row(&book, 1, RowId::WorkspaceNew).expect("a workspace row");
    assert_eq!(Some(&new), menu.foot.get(1));
    let autosave = action_row(&book, 1, RowId::WorkspaceAutosave).expect("a workspace row");
    assert_eq!(autosave.command, Some(WorkspaceCommand::SetAutosave(true)));
    assert_eq!(action_row(&book, 1, RowId::SaveToWorkspace), None);
}

#[test]
fn a_greyed_row_is_listed_unavailable_with_its_reason() {
    let menu = cell_menu(&book(false), 1, "Ctrl+S");
    let save = menu.head[3].scene_control("toolbar", None);
    assert_eq!(save.control_id, "toolbar.workspace_menu.save");
    assert!(!save.availability.available);
    assert_eq!(save.availability.reason.as_deref(), Some(NO_EDITS_REASON));
    assert!(!save.bounds_availability.available, "no rectangle given");
    let new = menu.foot[1].scene_control("toolbar", None);
    assert!(new.availability.available);
    assert_eq!(
        new.capability_id.as_deref(),
        Some(calls::WORKSPACE_NEW_CAPABILITY_ID)
    );
}

#[test]
fn the_signature_moves_with_the_words_and_nothing_else() {
    let mut book = book(false);
    let before = cell_menu(&book, 1, "Ctrl+S").signature();
    assert_eq!(cell_menu(&book, 1, "Ctrl+S").signature(), before);
    assert_ne!(cell_menu(&book, 1, "Ctrl+Alt+S").signature(), before);
    book.set_autosave(true);
    assert_ne!(
        cell_menu(&book, 1, "Ctrl+S").signature(),
        before,
        "the caption's words"
    );
}

#[test]
fn the_window_menu_list_names_the_tabs_on_each_workspace() {
    let book = book(true);
    let rows = window_list(&book, 1, |tabs| {
        tabs.iter().map(|tab| format!("T{tab}")).collect()
    });
    assert_eq!(rows[0].beside, "T1");
    assert!(rows[0].lit());
}
