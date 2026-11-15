//! The colour invariant through every book change: no two workspaces ever
//! hold identical colours after create, set, swap, delete, rename and reload,
//! and the stored `[colours]` table reads leniently, value by value.
use super::*;
use crate::arrangement_document::DeclaredLayout;
use crate::workspace_colour::ColourTaken;
use crate::workspace_colour::WorkspaceColour::{self, Custom};

fn preset(index: u8) -> WorkspaceColour {
    WorkspaceColour::preset(index).unwrap()
}

/// Every workspace holds a colour, and no two hold identical ones.
fn assert_unique(book: &WorkspaceBook) {
    let held: Vec<WorkspaceColour> = book.list().iter().map(|row| row.colour).collect();
    for (at, a) in held.iter().enumerate() {
        for b in &held[at + 1..] {
            assert!(!a.identical(*b), "a colour is shared: {held:?}");
        }
    }
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

fn colours(book: &WorkspaceBook) -> Vec<(String, WorkspaceColour)> {
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

/// A Default and a Scalp workspace with this `[colours]` table.
fn stored(colours: &str) -> WorkspaceBook {
    let text = format!(
        "version = {FORMAT_VERSION}\n{}{}[colours]\n{colours}",
        recipe("Default"),
        recipe("Scalp")
    );
    parse_book(&text).unwrap()
}

#[test]
fn default_is_slate_and_each_new_workspace_takes_the_next_free_colour() {
    let mut book = book();
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(preset(4)));
    book.save_as(1, "Scalp").unwrap();
    book.create(1, "Swing").unwrap();
    assert_eq!(book.colour("Scalp"), Some(preset(0)));
    assert_eq!(book.colour("Swing"), Some(preset(1)));
    // A name sorting first does not take an earlier colour.
    book.create(1, "Alpha").unwrap();
    assert_eq!(book.colour("Alpha"), Some(preset(2)));
    assert_eq!(book.colour("Scalp"), Some(preset(0)));
    assert_unique(&book);
}

#[test]
fn a_seventeenth_workspace_takes_a_generated_custom_and_a_freed_preset_goes_next() {
    let mut book = book();
    for n in 1..17 {
        book.create(1, &format!("W{n:02}")).unwrap();
        assert_unique(&book);
    }
    assert!(matches!(book.colour("W16"), Some(Custom(_))));
    book.delete("W03").unwrap();
    book.create(1, "Next").unwrap();
    assert_eq!(book.colour("Next"), Some(preset(2)), "W03's Sky freed");
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
    assert_eq!(book.colour("Next"), Some(preset(1)), "Swing's Orchid");
}

#[test]
fn a_taken_colour_is_refused_without_swap_and_swapped_with_it() {
    let mut book = book();
    book.create(1, "Scalp").unwrap();
    book.create(1, "Swing").unwrap();
    let refused = book.set_colour("Swing", preset(4), false);
    assert_eq!(
        refused,
        Err(WorkspaceRefusal::ColourTaken(ColourTaken::Held {
            owner: DEFAULT_WORKSPACE.to_owned(),
            owner_would_get: preset(1),
        }))
    );
    assert_eq!(refused.unwrap_err().code(), "workspace.colour_taken");
    assert_eq!(book.colour("Swing"), Some(preset(1)), "nothing moved");
    let outcome = book.set_colour("Swing", preset(4), true).unwrap();
    assert_eq!(outcome.swapped_with.as_deref(), Some(DEFAULT_WORKSPACE));
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(preset(1)));
    assert_eq!(book.colour("Swing"), Some(preset(4)));
    assert_unique(&book);
    let again = book.set_colour("Swing", preset(4), false).unwrap();
    assert!(!again.changed, "a repeat");
    assert_eq!(
        book.set_colour("Nope", preset(12), false),
        Err(WorkspaceRefusal::UnknownWorkspace("Nope".to_owned()))
    );
}

/// Default's colour changes like any other's.
#[test]
fn defaults_colour_can_change() {
    let mut book = book();
    let custom = Custom([0x3A, 0x7B, 0xD5]);
    assert!(
        book.set_colour(DEFAULT_WORKSPACE, custom, false)
            .unwrap()
            .changed
    );
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(custom));
    assert_eq!(colours(&reload(&book)), colours(&book));
}

#[test]
fn a_colour_follows_its_workspace_through_a_rename_and_a_reload() {
    let mut book = book();
    book.create(1, "Scalp").unwrap();
    book.set_colour("Scalp", preset(12), false).unwrap();
    book.rename("Scalp", "Aardvark").unwrap();
    assert_eq!(book.colour("Aardvark"), Some(preset(12)));
    let reloaded = reload(&book);
    assert_eq!(colours(&reloaded), colours(&book));
    assert_unique(&reloaded);
}

/// The file lists `Scalp` before `Default`; the colours still go in the
/// book's order, `Default` first, then by name.
#[test]
fn a_main_era_file_without_colours_gets_them_in_book_order_then_keeps_them() {
    let text = format!(
        "version = {FORMAT_VERSION}\n{}{}{}",
        recipe("Scalp"),
        recipe("Default"),
        recipe("Swing")
    );
    let book = parse_book(&text).unwrap();
    assert_eq!(
        colours(&book),
        [
            ("Default".to_owned(), preset(4)),
            ("Scalp".to_owned(), preset(0)),
            ("Swing".to_owned(), preset(1)),
        ]
    );
    assert_eq!(colours(&parse_book(&text).unwrap()), colours(&book));
    assert_eq!(colours(&reload(&book)), colours(&book), "kept once written");
}

/// A preset is written as its index, a custom colour as upper-case hex, and
/// the file's version does not move, so an older build still opens it.
#[test]
fn the_stored_form_is_an_index_or_a_hex_string_and_round_trips() {
    let mut book = book();
    book.create(1, "Scalp").unwrap();
    book.set_colour("Scalp", Custom([0x3A, 0x7B, 0xD5]), false)
        .unwrap();
    let text = toml::to_string_pretty(&book).unwrap();
    assert!(text.contains("[colours]"), "{text}");
    assert!(text.contains("Default = 4"), "{text}");
    assert!(text.contains("Scalp = \"#3A7BD5\""), "{text}");
    assert!(text.contains(&format!("version = {FORMAT_VERSION}")));
    assert_eq!(FORMAT_VERSION, 1);
    assert_eq!(colours(&parse_book(&text).unwrap()), colours(&book));
}

#[test]
fn an_index_and_hex_of_either_case_read_as_written() {
    let book = stored("Default = 9\nScalp = \"#3a7bd5\"\n");
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(preset(9)));
    assert_eq!(book.colour("Scalp"), Some(Custom([0x3A, 0x7B, 0xD5])));
    let book = stored("Default = \"#3A7BD5\"\nScalp = 0\n");
    assert_eq!(
        book.colour(DEFAULT_WORKSPACE),
        Some(Custom([0x3A, 0x7B, 0xD5]))
    );
    assert_eq!(book.colour("Scalp"), Some(preset(0)));
}

/// #978's words read through the fixed table, so its files keep their look.
#[test]
fn the_old_words_read_through_the_fixed_table() {
    let table = [
        ("steel", 4),
        ("violet", 11),
        ("cyan", 2),
        ("magenta", 1),
        ("azure", 0),
        ("indigo", 3),
        ("chalk", 12),
        ("cobalt", 8),
    ];
    for (word, index) in table {
        let book = stored(&format!("Scalp = \"{word}\"\n"));
        assert_eq!(book.colour("Scalp"), Some(preset(index)), "{word}");
        assert_unique(&book);
    }
}

/// A value this build cannot read is dropped and re-settled: the rest of the
/// file, and every other colour, is kept.
#[test]
fn an_unreadable_value_is_dropped_and_the_workspace_settles_on_a_free_colour() {
    for bad in [
        "16",
        "-1",
        "\"#12345\"",
        "\"#GG0000\"",
        "\"green\"",
        "1.5",
        "true",
    ] {
        let book = stored(&format!("Default = 9\nScalp = {bad}\n"));
        assert_eq!(book.list().len(), 2, "every recipe is kept: {bad}");
        assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(preset(9)), "{bad}");
        assert_eq!(book.colour("Scalp"), Some(preset(0)), "{bad}");
    }
}

#[test]
fn a_stored_duplicate_or_stranger_is_settled_on_load() {
    let book = stored("Default = 2\nScalp = \"#1F86BF\"\nGone = 4\n");
    assert_eq!(book.colour(DEFAULT_WORKSPACE), Some(preset(2)));
    assert_eq!(book.colour("Scalp"), Some(preset(0)), "Sky's Light value");
    assert_eq!(book.colour("Gone"), None);
    assert!(!toml::to_string_pretty(&book).unwrap().contains("Gone"));
    assert_unique(&book);
}

/// A tab the book has not adopted reads `Default`'s colour, as its workspace.
#[test]
fn a_tab_not_yet_adopted_wears_defaults_colour() {
    let mut book = book();
    book.save_as(1, "Scalp").unwrap();
    assert_eq!(book.tab_colour(1), Some(preset(0)));
    assert_eq!(book.tab_workspace(7), DEFAULT_WORKSPACE);
    assert_eq!(book.tab_colour(7), Some(preset(4)));
}

/// Cobalt's Light value on A and its Dark value on B: C asking for Cobalt is
/// refused even with `swap`, and a reload keeps every colour as it was.
#[test]
fn a_colour_two_workspaces_match_is_refused_and_a_reload_keeps_everything() {
    let mut book = book();
    for name in ["A", "B", "C"] {
        book.create(1, name).unwrap();
    }
    book.set_colour("A", Custom([0x2F, 0x5B, 0xD8]), false)
        .unwrap();
    book.set_colour("B", Custom([0x73, 0x93, 0xF5]), false)
        .unwrap();
    let before = colours(&book);
    let refused = book.set_colour("C", preset(0), true).unwrap_err();
    let owners = vec!["A".to_owned(), "B".to_owned()];
    assert_eq!(
        refused,
        WorkspaceRefusal::ColourTaken(ColourTaken::HeldByMany { owners })
    );
    assert!(refused.to_string().contains("A and B"), "{refused}");
    assert_eq!(colours(&book), before);
    assert_eq!(colours(&reload(&book)), before, "a reload changes nothing");
    assert_unique(&book);
}

/// Every applied custom colour becomes the book's last custom colour, the
/// palette's seventeenth swatch: a preset or a refusal leaves it, it round
/// trips as `last_custom = "#RRGGBB"`, and an old file or a bad value has none.
#[test]
fn an_applied_custom_colour_is_the_last_custom_and_round_trips() {
    let mut book = stored("Default = 4\nScalp = 2\n");
    assert_eq!(book.last_custom(), None, "an old file has none");
    let blue = [0x3A, 0x7B, 0xD5];
    book.set_colour("Scalp", Custom(blue), false).unwrap();
    assert_eq!(book.last_custom(), Some(blue));
    book.set_colour("Scalp", preset(0), false).unwrap();
    assert_eq!(book.last_custom(), Some(blue), "a preset leaves it");
    let refused = book.set_colour("Default", preset(0), false);
    assert!(refused.is_err());
    assert_eq!(book.last_custom(), Some(blue));
    let text = toml::to_string_pretty(&book).unwrap();
    assert!(text.contains("last_custom = \"#3A7BD5\""), "{text}");
    assert_eq!(reload(&book).last_custom(), Some(blue));
    let bad = format!(
        "version = {FORMAT_VERSION}\nlast_custom = \"blue\"\n{}",
        recipe("Default")
    );
    assert_eq!(parse_book(&bad).unwrap().last_custom(), None);
}
