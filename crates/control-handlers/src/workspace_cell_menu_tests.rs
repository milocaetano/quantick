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
    assert_eq!(scene_id(""), SCENE_ID);
    assert_eq!(scene_id("save"), "toolbar.workspace_menu.save");
}

#[test]
fn autosave_on_greys_save_and_flips_the_switch() {
    let menu = cell_menu(&book(true), 1, "Ctrl+S");
    let (switch, save) = (&menu.head[0], &menu.head[3]);
    assert!(switch.on);
    assert_eq!(switch.command, Some(WorkspaceCommand::SetAutosave(false)));
    assert!(!save.enabled && !save.dot);
    let rename = &menu.head[5];
    assert!(!rename.enabled, "Default cannot be renamed");
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
    assert!(save.enabled && save.dot);
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
