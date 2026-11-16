//! What Manage workspaces and the create dialogs ask of the book: a new
//! workspace in a chosen colour, the suggested names, and a delete that can
//! be undone for ten seconds, restoring the recipe, the colour and the tabs.
use crate::arrangement_document::DeclaredLayout;
use crate::workspace_book::*;
use crate::workspace_colour::{ColourTaken, WorkspaceColour};

fn preset(index: u8) -> WorkspaceColour {
    WorkspaceColour::preset(index).unwrap()
}

/// A book with tab 1 on a canvas a new workspace can be made of.
fn book() -> WorkspaceBook {
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    let recipe = WorkspaceRecipe {
        layout: DeclaredLayout::TimeAndFlow,
        ..WorkspaceRecipe::default()
    };
    book.observe(1, &recipe).unwrap();
    book
}

/// The mock's eight workspaces: Scalp (Sky) holds tabs 2 and 3, tab 1 is
/// on Flow; the colours are the mock's.
fn mock() -> WorkspaceBook {
    let names = [
        ("Default", 4),
        ("Flow", 3),
        ("Macro", 14),
        ("Night", 15),
        ("Opening", 0),
        ("Scalp", 2),
        ("Swing", 5),
        ("Tape", 1),
    ];
    let mut text = String::from("version = 1\n");
    for (name, _) in names {
        text.push_str(&format!("[recipes.{name}]\nlayout = \"flow\"\n"));
    }
    text.push_str("[colours]\n");
    for (name, index) in names {
        text.push_str(&format!("{name} = {index}\n"));
    }
    let mut book = parse_book(&text).unwrap();
    for (tab, name) in [(1, "Flow"), (2, "Scalp"), (3, "Scalp")] {
        book.open_tab(tab);
        book.set(tab, name).unwrap();
    }
    book
}

#[test]
fn save_as_takes_the_chosen_colour_and_new_takes_one_too() {
    let mut book = book();
    let saved = book.save_as_coloured(1, "Scalp", Some(preset(6))).unwrap();
    assert!(saved.changed);
    assert_eq!(book.colour("Scalp"), Some(preset(6)));
    let made = book.create_coloured(1, "Blank", Some(preset(9))).unwrap();
    assert_eq!(made.workspace, "Blank");
    assert_eq!(book.colour("Blank"), Some(preset(9)));
}

/// Left out, a new workspace takes the next free colour, as before.
#[test]
fn without_a_colour_a_new_workspace_takes_the_next_free_one() {
    let mut book = book();
    book.save_as_coloured(1, "Scalp", None).unwrap();
    assert_eq!(
        book.colour("Scalp"),
        Some(preset(0)),
        "Cobalt, the lowest free"
    );
}

/// A colour another workspace holds is refused on create, with its owner,
/// and nothing changes: no swap happens on create.
#[test]
fn a_held_colour_is_refused_on_create_and_nothing_changes() {
    let mut book = mock();
    let before = book.clone();
    let refused = book.save_as_coloured(1, "Flow 2", Some(preset(2)));
    assert_eq!(
        refused,
        Err(WorkspaceRefusal::ColourTaken(ColourTaken::NewWorkspace {
            owners: vec!["Scalp".to_owned()]
        }))
    );
    assert_eq!(refused.unwrap_err().code(), "workspace.colour_taken");
    let custom = WorkspaceColour::Custom([0x5D, 0xBD, 0xEB]);
    assert!(
        book.create_coloured(1, "Workspace 9", Some(custom))
            .is_err()
    );
    assert_eq!(book, before);
    let words = WorkspaceRefusal::ColourTaken(ColourTaken::NewWorkspace {
        owners: vec!["Scalp".to_owned()],
    });
    assert_eq!(words.to_string(), "Used by Scalp. Pick another colour.");
}

/// A save as repeated from the tab that made the name keeps its own colour.
#[test]
fn a_repeated_save_as_keeps_its_own_colour() {
    let mut book = book();
    book.save_as_coloured(1, "Scalp", Some(preset(6))).unwrap();
    let again = book.save_as_coloured(1, "Scalp", Some(preset(6))).unwrap();
    assert!(!again.changed);
    assert_eq!(book.colour("Scalp"), Some(preset(6)));
}

#[test]
fn the_suggested_names_follow_the_mock() {
    let book = mock();
    assert_eq!(book.suggested_name("Flow", 2), "Flow 2");
    assert_eq!(book.suggested_name("Workspace", 9), "Workspace 9");
    let mut book = book;
    book.save_as(1, "Flow 2").unwrap();
    assert_eq!(
        book.suggested_name("Flow", 2),
        "Flow 3",
        "a taken name is skipped"
    );
    let long = "A".repeat(MAX_WORKSPACE_NAME);
    let said = book.suggested_name(&long, 2);
    assert!(said.chars().count() <= MAX_WORKSPACE_NAME, "{said}");
    assert!(said.ends_with(" 2"));
}

/// Undo inside the ten seconds restores the recipe, the colour, the old
/// names a desk may still use, and the tabs exactly as they stood.
#[test]
fn undo_inside_ten_seconds_restores_the_recipe_colour_and_tabs() {
    let mut book = mock();
    let before = book.clone();
    let gone = book.delete_at("Scalp", 1_000).unwrap();
    assert_eq!(gone.moved, vec![2, 3]);
    assert_eq!(book.undo_until("Scalp"), Some(1_000 + UNDO_WINDOW_MS));
    assert_eq!(book.tab_workspace(2), DEFAULT_WORKSPACE);
    let undone = book.undo_delete("Scalp", 10_999).unwrap();
    assert!(undone.changed && !undone.recoloured);
    assert_eq!(undone.colour, preset(2));
    assert_eq!(undone.tabs, vec![2, 3]);
    assert_eq!(book.list(), before.list());
    assert_eq!(book.recipe("Scalp"), before.recipe("Scalp"));
    for tab in [1, 2, 3] {
        assert_eq!(book.tab_state(tab), before.tab_state(tab), "tab {tab}");
        assert_eq!(book.tab_copy(tab), before.tab_copy(tab), "tab {tab}");
    }
    assert_eq!(book.undo_until("Scalp"), None, "nothing left to undo");
}

/// A renamed workspace's old name, which a desk may still hold, comes back
/// with it.
#[test]
fn undo_restores_the_old_names_a_desk_may_hold() {
    let mut book = mock();
    book.rename("Scalp", "Scalper").unwrap();
    book.delete_at("Scalper", 0).unwrap();
    assert_eq!(book.resolve("Scalp"), None);
    book.undo_delete("Scalper", 5).unwrap();
    assert_eq!(book.resolve("Scalp"), Some("Scalper"));
}

/// At exactly ten seconds the Undo has ended.
#[test]
fn at_exactly_ten_seconds_undo_is_refused_as_expired() {
    let mut book = mock();
    book.delete_at("Scalp", 1_000).unwrap();
    let late = book.undo_delete("Scalp", 1_000 + UNDO_WINDOW_MS);
    let refusal = late.unwrap_err();
    assert_eq!(refusal, WorkspaceRefusal::UndoExpired("Scalp".to_owned()));
    assert_eq!(refusal.code(), "workspace.undo_expired");
    assert_eq!(book.recipe("Scalp"), None, "nothing came back");
}

/// No recent delete of the name is `workspace.unknown`; a repeat after an
/// Undo answers no change.
#[test]
fn undo_is_idempotent_and_refuses_a_name_never_deleted() {
    let mut book = mock();
    assert_eq!(
        book.undo_delete("Scalp", 0).unwrap_err().code(),
        "workspace.unknown"
    );
    book.delete_at("Scalp", 0).unwrap();
    assert!(book.undo_delete("Scalp", 1).unwrap().changed);
    let after = book.clone();
    let again = book.undo_delete("Scalp", 2).unwrap();
    assert!(!again.changed);
    assert_eq!(book, after);
}

/// A colour taken meanwhile gives the restored workspace the next free one,
/// and the outcome says it was recoloured.
#[test]
fn a_colour_taken_meanwhile_gives_the_next_free_one() {
    let mut book = mock();
    book.delete_at("Scalp", 0).unwrap();
    book.set_colour("Tape", preset(2), false).unwrap();
    let undone = book.undo_delete("Scalp", 1).unwrap();
    assert!(undone.recoloured);
    assert_eq!(undone.colour, preset(1), "Orchid, the lowest free now");
    assert_eq!(book.colour("Scalp"), Some(preset(1)));
}

/// A tab closed meanwhile stays closed, and one moved off Default stays
/// where it went; the rest come back.
#[test]
fn only_tabs_still_open_and_still_on_default_come_back() {
    let mut book = mock();
    book.delete_at("Scalp", 0).unwrap();
    book.close_tab(2);
    book.set(3, "Tape").unwrap();
    let undone = book.undo_delete("Scalp", 1).unwrap();
    assert!(undone.tabs.is_empty());
    assert_eq!(book.tab_workspace(3), "Tape");
    assert_eq!(book.tabs_on("Scalp"), Vec::<u64>::new());
}

/// A name taken meanwhile blocks the Undo with the taken name's reason.
#[test]
fn a_name_taken_meanwhile_refuses_the_undo() {
    let mut book = mock();
    book.delete_at("Scalp", 0).unwrap();
    book.save_as(1, "Scalp").unwrap();
    let refused = book.undo_delete("Scalp", 1).unwrap_err();
    assert_eq!(refused.code(), "workspace.name_taken");
}

/// Default is refused before anything is recorded.
#[test]
fn default_cannot_be_deleted_so_nothing_is_recorded() {
    let mut book = mock();
    assert!(book.delete_at(DEFAULT_WORKSPACE, 0).is_err());
    assert_eq!(book.undo_until(DEFAULT_WORKSPACE), None);
}

/// With Default on the factory recipe the window settles a moved tab's copy
/// to what it shows; a tab still clean on Default comes back exactly.
#[test]
fn a_tab_settled_clean_on_a_factory_default_comes_back_exactly() {
    let mut book = book();
    book.save_as(1, "Scalp").unwrap();
    assert!(book.recipe(DEFAULT_WORKSPACE).unwrap().is_factory());
    let before = book.clone();
    book.delete_at("Scalp", 0).unwrap();
    let shown = WorkspaceRecipe {
        layout: DeclaredLayout::Time,
        ..WorkspaceRecipe::default()
    };
    book.settle_tab(1, &shown);
    let undone = book.undo_delete("Scalp", 1).unwrap();
    assert_eq!(undone.tabs, vec![1]);
    assert_eq!(book.tab_state(1), before.tab_state(1), "clean, not marked");
    assert_eq!(book.tab_copy(1), before.tab_copy(1), "its own copy back");
}

/// Before a delete, which tabs would go back to Default and which keep
/// their edits there: the split the delete itself makes.
#[test]
fn the_delete_split_names_the_tabs_as_the_delete_moves_them() {
    let mut book = mock();
    let edited = WorkspaceRecipe {
        layout: DeclaredLayout::Time,
        ..WorkspaceRecipe::default()
    };
    book.observe(3, &edited).unwrap();
    assert_eq!(book.delete_split("Scalp"), (vec![2], vec![3]));
    let gone = book.delete_at("Scalp", 0).unwrap();
    assert_eq!((gone.adopted, gone.kept_edits), (vec![2], vec![3]));
}
