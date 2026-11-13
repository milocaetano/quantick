//! The colour invariant through every book change: no two workspaces ever
//! share a colour after create, pick, swap, delete, rename and reload.
use super::*;
use crate::arrangement_document::DeclaredLayout;
use crate::workspace_colour::WorkspaceColour::{self, *};

/// Every colour held, and none held twice.
fn assert_unique(book: &WorkspaceBook) {
    let held: Vec<WorkspaceColour> = book.list().iter().filter_map(|row| row.colour).collect();
    let mut sorted = held.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), held.len(), "a colour is shared: {held:?}");
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

fn colours(book: &WorkspaceBook) -> Vec<(String, Option<WorkspaceColour>)> {
    book.list()
        .into_iter()
        .map(|row| (row.name, row.colour))
        .collect()
}

/// A stored recipe of the flow canvas, as a file writes it.
fn recipe(name: &str) -> String {
    format!("[recipes.{name}]\nlayout = \"flow\"\n")
}

fn reload(book: &WorkspaceBook) -> WorkspaceBook {
    parse_book(&toml::to_string_pretty(book).unwrap()).unwrap()
}

#[test]
fn default_is_steel_and_each_new_workspace_takes_the_next_free_colour() {
    let mut book = book();
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(Steel));
    book.save_as(1, "Scalp").unwrap();
    book.create(1, "Swing").unwrap();
    assert_eq!(book.colour("Scalp"), Some(Violet));
    assert_eq!(book.colour("Swing"), Some(Cyan));
    // A name sorting first does not take an earlier colour.
    book.create(1, "Alpha").unwrap();
    assert_eq!(book.colour("Alpha"), Some(Magenta));
    assert_eq!(book.colour("Scalp"), Some(Violet));
    assert_unique(&book);
}

#[test]
fn a_ninth_workspace_holds_none_until_a_colour_frees() {
    let mut book = book();
    for name in ["A", "B", "C", "D", "E", "F", "G", "H"] {
        book.create(1, name).unwrap();
        assert_unique(&book);
    }
    assert_eq!(book.colour("G"), Some(Cobalt));
    assert_eq!(book.colour("H"), None, "all eight are held");
    book.delete("C").unwrap();
    assert_eq!(book.colour("H"), Some(Magenta), "C's colour freed");
    assert_eq!(book.colour("A"), Some(Violet), "nothing else moved");
    assert_unique(&book);
}

#[test]
fn deleting_a_workspace_frees_its_colour_and_recolours_nothing_else() {
    let mut book = book();
    for name in ["Scalp", "Swing", "Flow"] {
        book.create(1, name).unwrap();
    }
    let before = colours(&book);
    book.delete("Swing").unwrap();
    let after = colours(&book);
    let kept: Vec<_> = before
        .into_iter()
        .filter(|(name, _)| name != "Swing")
        .collect();
    assert_eq!(after, kept);
    book.create(1, "Next").unwrap();
    assert_eq!(
        book.colour("Next"),
        Some(Cyan),
        "Swing's colour is the first free"
    );
}

#[test]
fn picking_a_held_colour_swaps_the_two_and_an_unknown_name_is_refused() {
    let mut book = book();
    book.create(1, "Scalp").unwrap();
    book.create(1, "Swing").unwrap();
    let outcome = book.set_colour("Swing", Steel).unwrap();
    assert_eq!(outcome.swapped_with.as_deref(), Some(DEFAULT_WORKSPACE));
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(Cyan));
    assert_eq!(book.colour("Swing"), Some(Steel));
    assert_unique(&book);
    assert!(
        !book.set_colour("Swing", Steel).unwrap().changed,
        "a repeat"
    );
    assert_eq!(
        book.set_colour("Nope", Chalk),
        Err(WorkspaceRefusal::UnknownWorkspace("Nope".to_owned()))
    );
}

#[test]
fn a_colour_follows_its_workspace_through_a_rename_and_a_reload() {
    let mut book = book();
    book.create(1, "Scalp").unwrap();
    book.set_colour("Scalp", Chalk).unwrap();
    book.rename("Scalp", "Aardvark").unwrap();
    assert_eq!(book.colour("Aardvark"), Some(Chalk));
    let reloaded = reload(&book);
    assert_eq!(colours(&reloaded), colours(&book));
    assert_unique(&reloaded);
}

/// The file lists `Scalp` before `Default`; the colours still go in the
/// book's order, `Default` first, then by name.
#[test]
fn a_stored_book_without_colours_gets_them_in_book_order_then_keeps_them() {
    let text = format!(
        "version = 1\n{}{}{}",
        recipe("Scalp"),
        recipe("Default"),
        recipe("Swing")
    );
    let text = text.as_str();
    let book = parse_book(text).unwrap();
    assert_eq!(
        colours(&book),
        [
            ("Default".to_owned(), Some(Steel)),
            ("Scalp".to_owned(), Some(Violet)),
            ("Swing".to_owned(), Some(Cyan)),
        ]
    );
    assert_eq!(
        colours(&parse_book(text).unwrap()),
        colours(&book),
        "deterministic"
    );
    assert_eq!(colours(&reload(&book)), colours(&book), "kept once written");
}

/// A tab the book has not adopted reads `Default`'s colour, as its workspace.
#[test]
fn a_tab_not_yet_adopted_wears_defaults_colour() {
    let mut book = book();
    book.save_as(1, "Scalp").unwrap();
    assert_eq!(book.tab_colour(1), Some(Violet));
    assert_eq!(book.tab_workspace(7), DEFAULT_WORKSPACE);
    assert_eq!(book.tab_colour(7), Some(Steel));
}

#[test]
fn a_stored_duplicate_or_stranger_is_settled_on_load() {
    let colours = "[colours]\nDefault = \"cyan\"\nScalp = \"cyan\"\nGone = \"steel\"\n";
    let text = format!(
        "version = 1\n{}{}{colours}",
        recipe("Default"),
        recipe("Scalp")
    );
    let book = parse_book(&text).unwrap();
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(Cyan));
    assert_eq!(book.colour("Scalp"), Some(Steel));
    assert_unique(&book);
}

#[test]
fn an_unknown_colour_word_is_dropped_and_the_workspace_gets_a_valid_one() {
    let text = format!(
        "version = 1
{}{}[colours]
Default = \"cyan\"
Scalp = \"green\"
",
        recipe("Default"),
        recipe("Scalp")
    );
    let book = parse_book(&text).unwrap();
    assert_eq!(book.list().len(), 2, "every recipe is kept");
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(Cyan));
    assert_eq!(book.colour("Scalp"), Some(Steel));
    assert_unique(&book);
}

#[test]
fn colours_naming_a_missing_workspace_are_dropped_on_load() {
    let text = format!(
        "version = 1
{}[colours]
Default = \"steel\"
Ghost = \"violet\"
",
        recipe("Default")
    );
    let book = parse_book(&text).unwrap();
    assert_eq!(book.colour("Ghost"), None);
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(Steel));
    assert!(!toml::to_string_pretty(&book).unwrap().contains("Ghost"));
}
