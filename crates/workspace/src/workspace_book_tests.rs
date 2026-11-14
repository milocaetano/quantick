use super::*;
use crate::arrangement_document::DeclaredLayout;
use crate::indicator_document::SavedKind;

fn ema() -> SavedIndicator {
    SavedIndicator {
        kind: SavedKind::native("native.ema"),
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    }
}

/// A book with autosave off: a tab holds its edits, marked, until Save to.
fn held_book() -> WorkspaceBook {
    let mut book = WorkspaceBook::default();
    book.set_autosave(false);
    book
}

fn recipe(layout: DeclaredLayout) -> WorkspaceRecipe {
    WorkspaceRecipe {
        layout,
        ..WorkspaceRecipe::default()
    }
}

/// `Scalp` holds the time+flow canvas; tabs 1 and 2 use it, clean.
fn two_tabs_on_scalp() -> WorkspaceBook {
    let mut book = held_book();
    book.open_tab(1);
    book.open_tab(2);
    book.observe(1, &recipe(DeclaredLayout::TimeAndFlow))
        .unwrap();
    book.save_as(1, "Scalp").unwrap();
    book.set(2, "Scalp").unwrap();
    book
}

fn state(book: &WorkspaceBook, tab: u64) -> (String, bool) {
    let state = book.tab_state(tab).unwrap();
    (state.workspace.to_owned(), state.modified)
}

#[test]
fn a_new_tab_opens_on_default_clean_and_default_is_always_listed() {
    let mut book = held_book();
    book.open_tab(1);
    assert_eq!(state(&book, 1), ("Default".to_owned(), false));
    assert_eq!(
        book.list(),
        [WorkspaceListing {
            name: "Default".to_owned(),
            tabs: vec![1]
        }]
    );
    let parsed = parse_book("version = 1\n").unwrap();
    assert!(
        parsed.recipe(DEFAULT_WORKSPACE).is_some(),
        "put back on parse"
    );
}

#[test]
fn modified_is_derived_by_comparing_and_returns_when_the_edit_is_undone() {
    let mut book = two_tabs_on_scalp();
    assert_eq!(state(&book, 1), ("Scalp".to_owned(), false));
    assert!(book.observe(1, &recipe(DeclaredLayout::Time)).unwrap());
    assert!(book.tab_state(1).unwrap().modified);
    assert!(
        book.observe(1, &recipe(DeclaredLayout::TimeAndFlow))
            .unwrap()
    );
    assert!(!book.tab_state(1).unwrap().modified, "nothing is stored");
}

#[test]
fn an_edit_stays_on_its_tab_until_save_to_and_save_to_names_who_follows() {
    let mut book = two_tabs_on_scalp();
    book.open_tab(3);
    book.set(3, "Scalp").unwrap();
    book.observe(3, &recipe(DeclaredLayout::Flow)).unwrap();
    book.observe(1, &recipe(DeclaredLayout::Time)).unwrap();
    assert_eq!(state(&book, 2), ("Scalp".to_owned(), false));
    assert_eq!(
        book.tab_copy(2).unwrap().layout,
        DeclaredLayout::TimeAndFlow,
        "the other tab did not move"
    );
    assert_eq!(book.save_to_preview(1).unwrap(), [2]);
    let saved = book.save_to(1).unwrap();
    assert!(saved.changed);
    assert_eq!(saved.tabs_changed, [2]);
    assert_eq!(book.tab_copy(2).unwrap().layout, DeclaredLayout::Time);
    assert_eq!(state(&book, 1), ("Scalp".to_owned(), false));
    assert_eq!(book.tab_copy(3).unwrap().layout, DeclaredLayout::Flow);
    assert!(book.tab_state(3).unwrap().modified, "its edits are its own");
}

#[test]
fn a_repeated_save_to_reports_no_change() {
    let mut book = two_tabs_on_scalp();
    let again = book.save_to(1).unwrap();
    assert!(!again.changed);
    assert!(again.tabs_changed.is_empty());
    assert!(book.save_to_preview(1).unwrap().is_empty());
}

#[test]
fn save_as_refuses_empty_and_duplicate_names_with_the_reason() {
    let mut book = two_tabs_on_scalp();
    let empty = book.save_as(2, "   ").unwrap_err();
    assert_eq!(empty, WorkspaceRefusal::EmptyName);
    assert_eq!(empty.code(), "workspace.name_empty");
    assert_eq!(empty.to_string(), "a workspace needs a name");
    let duplicate = book.save_as(2, "scalp").unwrap_err();
    assert_eq!(duplicate.code(), "workspace.name_taken");
    assert_eq!(duplicate.to_string(), "a workspace is already called Scalp");
    assert_eq!(
        book.validate_new_name(&"x".repeat(MAX_WORKSPACE_NAME + 1)),
        Err(WorkspaceRefusal::NameTooLong)
    );
    assert_eq!(
        book.validate_new_name(" Tape  one "),
        Ok("Tape one".to_owned())
    );
}

#[test]
fn save_as_moves_only_its_tab_and_a_retry_is_no_change() {
    let mut book = two_tabs_on_scalp();
    book.observe(1, &recipe(DeclaredLayout::Time)).unwrap();
    let saved = book.save_as(1, "Tape").unwrap();
    assert!(saved.changed);
    assert_eq!(state(&book, 1), ("Tape".to_owned(), false));
    assert_eq!(state(&book, 2), ("Scalp".to_owned(), false));
    assert!(!book.save_as(1, "Tape").unwrap().changed);
}

#[test]
fn set_drops_edits_and_says_so_and_a_repeat_keeps_them() {
    let mut book = two_tabs_on_scalp();
    book.observe(2, &recipe(DeclaredLayout::Time)).unwrap();
    let repeat = book.set(2, "Scalp").unwrap();
    assert!(!repeat.changed);
    assert!(book.tab_state(2).unwrap().modified);
    let moved = book.set(2, DEFAULT_WORKSPACE).unwrap();
    assert!(moved.changed && moved.dropped_edits);
    assert_eq!(state(&book, 2), ("Default".to_owned(), false));
    assert_eq!(
        book.set(2, "Nope"),
        Err(WorkspaceRefusal::UnknownWorkspace("Nope".to_owned()))
    );
    assert_eq!(book.set(9, "Scalp"), Err(WorkspaceRefusal::UnknownTab(9)));
}

/// Autosave on, the opening default: an edit is on its way to the workspace,
/// so the cell never marks it and closing never asks, though the copy differs.
#[test]
fn with_autosave_on_an_edit_never_reads_as_held() {
    let mut book = two_tabs_on_scalp();
    assert!(book.set_autosave(true));
    book.observe(1, &recipe(DeclaredLayout::Time)).unwrap();
    assert_eq!(state(&book, 1), ("Scalp".to_owned(), false));
    assert!(book.differs(1));
    assert_eq!(book.differing_tabs(), vec![1]);
    assert_eq!(book.unsaved_edits(1), None);
    let saved = book.save_to(1).unwrap();
    assert_eq!(saved.tabs_changed, vec![2], "the clean tab follows");
    assert!(!book.differs(1));
    assert!(!book.set_autosave(true), "a repeat moves nothing");
}

/// Autosave off: closing asks, naming the workspace, the tabs a save also
/// changes and what changed.
#[test]
fn with_autosave_off_the_close_prompt_names_what_would_be_lost() {
    let mut book = two_tabs_on_scalp();
    let mut held = recipe(DeclaredLayout::Time);
    held.flow_indicators = vec![ema()];
    book.observe(1, &held).unwrap();
    assert_eq!(state(&book, 1), ("Scalp".to_owned(), true));
    assert_eq!(
        book.unsaved_edits(1),
        Some(UnsavedEdits {
            workspace: "Scalp".to_owned(),
            followers: vec![2],
            changes: vec!["EMA".to_owned(), "panes".to_owned()],
        })
    );
    assert_eq!(book.unsaved_edits(2), None, "a clean tab closes at once");
}

fn flow_key() -> DrawingKey {
    DrawingKey::of_pane("mt5", "WIN*".to_owned(), None)
}

/// Autosave off: drawings held back light the mark on their own, the prompt
/// counts them after the recipe's changes, and no recipe follower is named.
#[test]
fn held_drawings_mark_the_tab_and_the_prompt_counts_them() {
    let mut book = two_tabs_on_scalp();
    book.note_drawings(2, vec![flow_key()], 3);
    assert_eq!(state(&book, 2), ("Scalp".to_owned(), true));
    assert_eq!(
        book.unsaved_edits(2),
        Some(UnsavedEdits {
            workspace: "Scalp".to_owned(),
            followers: Vec::new(),
            changes: vec!["3 drawings".to_owned()],
        })
    );
    let mut held = recipe(DeclaredLayout::Time);
    held.flow_indicators = vec![ema()];
    book.observe(1, &held).unwrap();
    book.note_drawings(1, vec![flow_key()], 1);
    let changes = book.unsaved_edits(1).unwrap().changes;
    assert_eq!(changes, ["EMA", "panes", "1 drawing"]);
    assert_eq!(book.drawing_keys_except(1), vec![flow_key()]);
    for (tab, keys) in book.drawing_sets() {
        book.note_drawings(tab, keys, 0);
    }
    assert_eq!(
        state(&book, 2),
        ("Scalp".to_owned(), false),
        "Save wrote them"
    );
    assert_eq!(book.unsaved_edits(2), None);
    assert_eq!(book.drawing_keys(2), [flow_key()], "the sets stay known");
    book.close_tab(2);
    assert!(
        book.drawing_keys(2).is_empty(),
        "a closed tab holds nothing"
    );
}

/// Autosave on: a count the frame notes never marks.
#[test]
fn with_autosave_on_held_drawings_never_mark() {
    let mut book = two_tabs_on_scalp();
    book.set_autosave(true);
    book.note_drawings(2, vec![flow_key()], 2);
    assert_eq!(state(&book, 2), ("Scalp".to_owned(), false));
    assert_eq!(book.unsaved_edits(2), None);
}

#[test]
fn rename_carries_its_tabs_and_default_cannot_be_renamed() {
    let mut book = two_tabs_on_scalp();
    let renamed = book.rename("Scalp", " The  open ").unwrap();
    assert_eq!(
        renamed,
        RenameOutcome {
            workspace: "The open".to_owned(),
            changed: true
        },
        "the name as stored"
    );
    assert_eq!(state(&book, 2), ("The open".to_owned(), false));
    assert!(book.rename("The open", "Open").unwrap().changed);
    assert!(!book.rename("Open", " Open ").unwrap().changed);
    assert!(
        book.rename("Open", "OPEN").unwrap().changed,
        "case alone is a rename"
    );
    assert_eq!(
        book.rename(DEFAULT_WORKSPACE, "Other"),
        Err(WorkspaceRefusal::DefaultProtected)
    );
    assert_eq!(
        book.rename("OPEN", "default").unwrap_err().code(),
        "workspace.name_taken"
    );
    assert_eq!(
        book.rename("Gone", "x").unwrap_err().code(),
        "workspace.unknown"
    );
}

#[test]
fn delete_moves_tabs_to_default_and_edits_survive_as_default_marked() {
    let mut book = two_tabs_on_scalp();
    book.observe(2, &recipe(DeclaredLayout::Time)).unwrap();
    let deleted = book.delete("Scalp").unwrap();
    assert_eq!(deleted.moved, [1, 2]);
    assert_eq!(deleted.adopted, [1]);
    assert_eq!(deleted.kept_edits, [2]);
    assert_eq!(state(&book, 1), ("Default".to_owned(), false));
    assert_eq!(book.tab_copy(1).unwrap().layout, DeclaredLayout::Flow);
    assert_eq!(state(&book, 2), ("Default".to_owned(), true));
    assert_eq!(book.tab_copy(2).unwrap().layout, DeclaredLayout::Time);
    assert_eq!(
        book.delete("Scalp").unwrap_err().code(),
        "workspace.unknown",
        "a repeat finds no such workspace"
    );
    assert_eq!(
        book.delete(DEFAULT_WORKSPACE),
        Err(WorkspaceRefusal::DefaultProtected)
    );
}

#[test]
fn list_is_default_first_then_by_name_with_each_workspaces_tabs() {
    let mut book = two_tabs_on_scalp();
    book.open_tab(3);
    book.save_as(3, "Alpha").unwrap();
    let rows: Vec<_> = book
        .list()
        .into_iter()
        .map(|row| (row.name, row.tabs))
        .collect();
    assert_eq!(
        rows,
        [
            ("Default".to_owned(), vec![]),
            ("Alpha".to_owned(), vec![3]),
            ("Scalp".to_owned(), vec![1, 2]),
        ]
    );
}

#[test]
fn the_first_run_seeds_one_workspace_per_tab_deduplicated_and_unmodified() {
    let mut book = held_book();
    let live = recipe(DeclaredLayout::TimeTimeAndFlow);
    let seed = |tab, layout: Option<&str>, symbol: &str| SeedTab {
        tab,
        layout: layout.map(str::to_owned),
        symbol: symbol.to_owned(),
        live: live.clone(),
    };
    let seeded = book.seed_first_run(vec![
        seed(1, Some("Fluxo"), "WINV26"),
        seed(2, Some("Fluxo"), "WINV26"),
        seed(3, None, "BTC"),
        seed(4, Some("Default"), "WDOZ26"),
    ]);
    assert_eq!(
        seeded,
        [
            (1, "Fluxo".to_owned()),
            (2, "Fluxo 2".to_owned()),
            (3, "BTC".to_owned()),
            (4, "WDOZ26".to_owned())
        ],
        "the layout's name, else the bare symbol; never `from `"
    );
    for tab in 1..=4 {
        assert!(!book.tab_state(tab).unwrap().modified);
        assert_eq!(book.tab_copy(tab), Some(&live));
    }
    assert_eq!(book.recipe("Fluxo"), Some(&live));
    assert!(
        book.seed_first_run(vec![seed(1, None, "WINV26")])
            .is_empty(),
        "a seeded tab is left alone"
    );
}

/// A desk tab written before workspaces existed: the fixture a first run reads.
#[test]
fn a_seed_from_an_old_desk_tab_copies_the_indicators_and_holds_no_layout_id() {
    let tab: SavedTab = toml::from_str(concat!(
        "feed = \"mt5\"\nsymbol = \"WINV26\"\nlayout = \"time+flow\"\n",
        "flow_bars = \"tick:50\"\ntime_bars = \"time:1m\"\n",
        "split_fraction = 0.25\nflow_layout = 7\ncontext_layouts = [0]\n",
    ))
    .unwrap();
    assert_eq!(tab.workspace, None, "an old desk names no workspace");
    let mut layouts = LayoutBook::starter(vec![ema()]);
    let second = layouts.create(Some("Empty")).unwrap();
    assert_eq!(second.0, 2);
    let live = capture_recipe(&tab, &layouts, (BTreeMap::new(), Vec::new()), None);
    assert_eq!(live.layout, DeclaredLayout::TimeAndFlow);
    assert_eq!(
        live.context_panes.len(),
        1,
        "one context pane, from time_bars"
    );
    assert_eq!(live.split_fraction, Some(0.25));
    assert_eq!(
        live.flow_indicators,
        [ema()],
        "an unknown id reads the active"
    );
    assert_eq!(live.context_panes[0].indicators, [ema()]);

    let mut book = held_book();
    book.seed_first_run(vec![SeedTab {
        tab: 1,
        layout: None,
        symbol: tab.symbol.clone(),
        live: live.clone(),
    }]);
    assert_eq!(state(&book, 1), ("WINV26".to_owned(), false));

    layouts.get_mut(LayoutId(1)).unwrap().indicators.clear();
    assert_eq!(
        book.recipe("WINV26").unwrap().flow_indicators,
        [ema()],
        "an edit on the shared layout does not reach the recipe"
    );
}

/// The desk is written on exit or save; workspaces.toml at the call. With
/// save-on-exit off, the desk read at the next launch still names the old name.
#[test]
fn a_restart_follows_a_rename_and_a_lost_workspace_keeps_what_the_tab_shows() {
    let mut book = two_tabs_on_scalp();
    let desk = book.saved_fields(1);
    assert_eq!(desk, (Some("Scalp".to_owned()), None));
    book.rename("Scalp", "Tape").unwrap();
    book.rename("Tape", "Open").unwrap();
    let text = toml::to_string_pretty(&book).unwrap();
    let mut reopened = parse_book(&text).unwrap();
    reopened.set_autosave(false);
    reopened.restore_tab(1, "Scalp", None);
    assert_eq!(state(&reopened, 1), ("Open".to_owned(), false));
    assert_eq!(
        reopened.tab_copy(1),
        reopened.recipe("Open"),
        "the renamed recipe"
    );

    reopened.delete("Open").unwrap();
    assert_eq!(reopened.resolve("Scalp"), None, "deleted since");
    let live = recipe(DeclaredLayout::TimeTimeAndFlow);
    reopened.restore_tab(2, "Scalp", Some(live.clone()));
    assert_eq!(state(&reopened, 2), ("Default".to_owned(), true));
    assert_eq!(reopened.tab_copy(2), Some(&live), "Default • what it shows");
}

#[test]
fn a_rename_back_and_forth_resolves_without_a_loop() {
    let mut book = two_tabs_on_scalp();
    book.rename("Scalp", "Tape").unwrap();
    book.rename("Tape", "Scalp").unwrap();
    assert_eq!(book.resolve("Scalp"), Some("Scalp"));
    assert_eq!(book.resolve("Tape"), Some("Scalp"));
}

/// The factory `Default` names no arrangement, so a tab on it is told against
/// what it settled on: undoing an edit by hand clears the mark.
#[test]
fn on_the_factory_default_an_edit_undone_by_hand_reads_clean() {
    let mut book = held_book();
    book.open_tab(1);
    let opened = recipe(DeclaredLayout::TimeAndFlow);
    book.settle_tab(1, &opened);
    assert_eq!(state(&book, 1), ("Default".to_owned(), false));
    book.observe(1, &recipe(DeclaredLayout::Time)).unwrap();
    assert_eq!(state(&book, 1), ("Default".to_owned(), true));
    book.settle_tab(1, &recipe(DeclaredLayout::Flow));
    assert!(book.tab_state(1).unwrap().modified, "edits are kept");
    book.observe(1, &opened).unwrap();
    assert_eq!(state(&book, 1), ("Default".to_owned(), false));

    assert_eq!(book.saved_fields(1), (Some("Default".to_owned()), None));
}

#[test]
fn a_tab_keeps_its_edits_across_a_restart_still_marked() {
    let mut book = two_tabs_on_scalp();
    book.observe(2, &recipe(DeclaredLayout::Time)).unwrap();
    let (name, copy) = book.saved_fields(2);
    assert!(
        book.saved_fields(1).1.is_none(),
        "a clean tab keeps no copy"
    );
    let text = toml::to_string_pretty(&book).unwrap();
    let mut reopened = parse_book(&text).unwrap();
    reopened.set_autosave(false);
    reopened.restore_tab(2, &name.unwrap(), copy);
    reopened.restore_tab(1, "Scalp", None);
    assert_eq!(state(&reopened, 2), ("Scalp".to_owned(), true));
    assert_eq!(state(&reopened, 1), ("Scalp".to_owned(), false));
    reopened.restore_tab(3, "Gone", Some(recipe(DeclaredLayout::Time)));
    assert_eq!(state(&reopened, 3), ("Default".to_owned(), true));
}

#[test]
fn a_full_recipe_round_trips_through_the_file_exactly() {
    let mut book = held_book();
    book.open_tab(1);
    let live = WorkspaceRecipe {
        layout: DeclaredLayout::TimeTimeAndFlow,
        split_fraction: Some(0.1 + 0.2),
        context_heights: vec![0.3, 0.7],
        context_panes_collapsed: vec![false, true],
        flow_indicators: vec![ema()],
        context_panes: vec![
            ContextPaneRecipe {
                indicators: vec![ema()],
                layers: BTreeMap::from([("grid".to_owned(), true)]),
                bars: None,
            },
            ContextPaneRecipe {
                indicators: Vec::new(),
                layers: BTreeMap::new(),
                bars: None,
            },
        ],
        layers: BTreeMap::from([("grid".to_owned(), false)]),
        footprint_style: Some("ladder".to_owned()),
        ..WorkspaceRecipe::default()
    };
    book.observe(1, &live).unwrap();
    book.save_as(1, "from WINV26").unwrap();
    let text = toml::to_string_pretty(&book).unwrap();
    let parsed = parse_book(&text).unwrap();
    assert_eq!(parsed.recipe("from WINV26"), Some(&live), "{text}");
    assert!(matches!(
        parse_book("version = 9\n"),
        Err(BookParseError::Version(9))
    ));
}

#[test]
fn the_factory_default_amounts_to_what_a_new_tab_opens_with_and_no_saved_recipe_reads_as_it() {
    let book = held_book();
    assert!(book.recipe(DEFAULT_WORKSPACE).unwrap().is_factory());
    let opened = WorkspaceRecipe::new_tab_arrangement(DeclaredLayout::TimeTimeAndFlow, &[ema()]);
    assert_eq!(opened.layout, DeclaredLayout::TimeTimeAndFlow);
    assert_eq!(opened.flow_indicators, [ema()]);
    assert_eq!(opened.context_panes.len(), 2);
    assert_eq!(opened.context_panes[1].indicators, [ema()]);
    assert!(!opened.is_factory());
}

#[test]
fn each_context_pane_keeps_its_own_layers_by_place() {
    let tab: SavedTab = toml::from_str(concat!(
        "feed = \"mt5\"
symbol = \"WINV26\"
layout = \"time+time+flow\"
",
        "flow_bars = \"tick:50\"
context_bars = [\"time:1m\", \"time:1h\"]
",
    ))
    .unwrap();
    let top = BTreeMap::from([("bubbles".to_owned(), true)]);
    let flow = BTreeMap::from([("bubbles".to_owned(), false)]);
    let recipe = capture_recipe(
        &tab,
        &LayoutBook::starter(vec![ema()]),
        (flow.clone(), vec![top.clone()]),
        None,
    );
    assert_eq!(recipe.layers, flow);
    assert_eq!(recipe.context_panes[0].layers, top);
    assert!(
        recipe.context_panes[1].layers.is_empty(),
        "a pane the window did not read leaves its layers open"
    );
}

#[test]
fn each_listed_workspace_says_its_panes_interval_and_indicator_count() {
    let mut book = two_tabs_on_scalp();
    book.open_tab(3);
    let mut held = recipe(DeclaredLayout::TimeTimeAndFlow);
    held.flow_indicators = vec![ema(), ema()];
    held.context_panes = vec![
        ContextPaneRecipe {
            indicators: vec![ema()],
            layers: BTreeMap::new(),
            bars: None,
        };
        2
    ];
    book.observe(3, &held).unwrap();
    book.save_as(3, "Swing").unwrap();
    let rows: Vec<_> = book
        .list()
        .into_iter()
        .map(|row| {
            let summary = book.summary(&row.name);
            (row.name, summary)
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("Default".to_owned(), "as the feed opens".to_owned()),
            ("Scalp".to_owned(), "Time · Flow".to_owned()),
            ("Swing".to_owned(), "Time · Time · Flow · 4 ind".to_owned()),
        ]
    );
}

#[test]
fn create_puts_the_tab_on_an_empty_recipe_and_no_other_tab_moves() {
    let mut book = two_tabs_on_scalp();
    let mut held = recipe(DeclaredLayout::TimeAndFlow);
    held.flow_indicators = vec![ema()];
    held.split_fraction = Some(0.3);
    held.layers.insert("bubbles".to_owned(), true);
    book.observe(1, &held).unwrap();
    let made = book.create(1, " Blank ").unwrap();
    assert_eq!(
        made,
        CreateOutcome {
            workspace: "Blank".to_owned(),
            dropped_edits: true
        }
    );
    assert_eq!(state(&book, 1), ("Blank".to_owned(), false));
    assert_eq!(state(&book, 2), ("Scalp".to_owned(), false));
    let empty = book.recipe("Blank").unwrap();
    assert!(empty.flow_indicators.is_empty());
    assert_eq!(
        empty.split_fraction,
        Some(0.3),
        "the sizes the tab reads back"
    );
    assert_eq!(empty.layers.get("bubbles"), Some(&true));
    assert_eq!(book.tab_copy(1), Some(empty));
    let shown = empty.clone();
    assert!(
        !book.observe(1, &shown).unwrap(),
        "the tab reads back what it was given"
    );
    assert_eq!(state(&book, 1), ("Blank".to_owned(), false));
}

/// A tab still opening holds the factory `Default`, which names no
/// arrangement: an empty recipe read from it would be that recipe again.
#[test]
fn create_refuses_a_tab_still_opening() {
    let mut book = held_book();
    book.open_tab(1);
    let before = book.clone();
    assert_eq!(
        book.create(1, "Blank"),
        Err(WorkspaceRefusal::TabOpening(1))
    );
    assert_eq!(book, before);
}

#[test]
fn create_refuses_a_taken_or_empty_name_and_changes_nothing() {
    let mut book = two_tabs_on_scalp();
    let before = book.clone();
    assert!(matches!(
        book.create(1, "scalp"),
        Err(WorkspaceRefusal::DuplicateName(_))
    ));
    assert!(book.create(1, "  ").is_err());
    assert!(book.create(9, "Fresh").is_err(), "no such tab");
    assert_eq!(book, before);
}

/// Autosave on, two tabs on one workspace both edited: writing one leaves the
/// other differing, and it reads modified, never silently unsaved.
#[test]
fn an_outvoted_tab_reads_modified_with_autosave_on() {
    let mut book = two_tabs_on_scalp();
    book.set_autosave(true);
    let mut one = recipe(DeclaredLayout::Time);
    one.flow_indicators = vec![ema()];
    book.observe(1, &one).unwrap();
    book.observe(2, &recipe(DeclaredLayout::Flow)).unwrap();
    let mut lowest = book.clone();
    assert_eq!(
        book.autosave_winners(&[1, 2], 2),
        [2],
        "the active tab wins"
    );
    assert_eq!(
        lowest.autosave_winners(&[2, 1], 9),
        [1],
        "else the lowest id"
    );
    book.save_to(2).unwrap();
    assert!(book.differs(1));
    assert_eq!(book.unsynced(1), Some(Unsynced::Outvoted));
    assert_eq!(state(&book, 1), ("Scalp".to_owned(), true));
    assert_eq!(state(&book, 2), ("Scalp".to_owned(), false));
    book.save_to(1).unwrap();
    assert_eq!(
        state(&book, 1),
        ("Scalp".to_owned(), false),
        "saved at last"
    );
    assert!(!book.differs(2), "the clean tab followed");
}

/// Autosave on: a write-through that failed marks the tab and is news once.
#[test]
fn a_failed_write_through_marks_the_tab_until_a_save_lands() {
    let mut book = two_tabs_on_scalp();
    book.set_autosave(true);
    book.observe(1, &recipe(DeclaredLayout::Flow)).unwrap();
    assert!(!state(&book, 1).1);
    assert!(book.note_write_failed(1));
    assert!(
        !book.note_write_failed(1),
        "a retry that fails again is no news"
    );
    assert_eq!(state(&book, 1), ("Scalp".to_owned(), true));
    assert_eq!(book.failed_writes(), [1]);
    book.save_to(1).unwrap();
    assert!(!state(&book, 1).1);
    assert!(book.failed_writes().is_empty());
}

/// An exit's Close without saving puts the copy back, so the desk keeps no
/// edits and the next launch opens the tab on its workspace's recipe.
#[test]
fn discarded_edits_are_not_what_the_next_launch_restores() {
    let mut book = two_tabs_on_scalp();
    book.observe(1, &recipe(DeclaredLayout::Flow)).unwrap();
    assert!(
        book.saved_fields(1).1.is_some(),
        "the desk would keep the edit"
    );
    assert!(book.discard_edits(1));
    let (workspace, copy) = book.saved_fields(1);
    assert_eq!((workspace.as_deref(), copy.as_ref()), (Some("Scalp"), None));
    let mut next = book.clone();
    next.close_tab(1);
    next.restore_tab(1, "Scalp", copy);
    assert_eq!(next.tab_copy(1), next.recipe("Scalp"));
    assert!(!book.discard_edits(1), "a repeat changes nothing");
}

/// A count stands while the store and the tab's sets stay as they were.
#[test]
fn a_drawing_count_is_current_only_at_its_revision_and_sets() {
    let mut book = two_tabs_on_scalp();
    book.note_drawings_at(1, vec![flow_key()], 2, 7);
    assert!(book.drawings_current(1, &[flow_key()], 7));
    assert!(!book.drawings_current(1, &[flow_key()], 8));
    assert!(!book.drawings_current(1, &[], 7));
    book.note_drawings(1, vec![flow_key()], 2);
    assert!(
        !book.drawings_current(1, &[flow_key()], 7),
        "told without one"
    );
}

/// Tabs 1 and 2 on `Scalp`, autosave on, both holding a different edit.
fn contested() -> WorkspaceBook {
    let mut book = two_tabs_on_scalp();
    book.set_autosave(true);
    book.observe(1, &recipe(DeclaredLayout::Time)).unwrap();
    book.observe(2, &recipe(DeclaredLayout::Flow)).unwrap();
    book
}

/// Only tab 2 is due, but the active tab 1 holds edits on the same workspace:
/// the winner is weighed over every differing tab there, so tab 2 is not
/// written and reads modified.
#[test]
fn a_due_tab_never_beats_the_active_tab_that_is_not_due() {
    let mut book = contested();
    assert!(book.autosave_winners(&[2], 1).is_empty());
    assert_eq!(book.unsynced(2), Some(Unsynced::Outvoted));
    assert_eq!(state(&book, 2), ("Scalp".to_owned(), true));
    assert_eq!(book.autosave_winners(&[1], 1), [1], "the active tab's turn");
}

/// An outvoted copy is never written by autosave, however often it comes due,
/// and closing it asks as a held edit does.
#[test]
fn an_outvoted_tab_is_never_written_through_and_its_close_asks() {
    let mut book = contested();
    assert_eq!(book.autosave_winners(&[1, 2], 1), [1]);
    book.save_to(1).unwrap();
    assert!(book.autosave_winners(&[2], 1).is_empty());
    assert!(book.autosave_winners(&[2], 2).is_empty(), "active or not");
    let asked = book.unsaved_edits(2).expect("its close asks");
    assert_eq!(asked.workspace, "Scalp");
}

/// A write that failed is not retried over a later save of its workspace by
/// another tab: the failed copy turns outvoted, marked, and is never written.
#[test]
fn a_failed_copy_is_outvoted_once_another_tab_saves_its_workspace() {
    let mut book = contested();
    book.note_write_failed(1);
    book.save_to(2).unwrap();
    assert_eq!(book.unsynced(1), Some(Unsynced::Outvoted));
    assert!(book.failed_writes().is_empty(), "no retry is owed");
    assert!(book.autosave_winners(&[1], 1).is_empty());
    assert_eq!(state(&book, 1), ("Scalp".to_owned(), true));
}

/// After a restart with autosave on, a tab whose copy the desk kept differing
/// is not in its workspace: it reads modified, asks on close and is never
/// written unasked. A clean tab restores clean.
#[test]
fn a_restored_copy_that_differs_reads_modified_with_autosave_on() {
    let mut next = two_tabs_on_scalp();
    next.set_autosave(true);
    next.close_tab(1);
    next.close_tab(2);
    next.restore_tab(1, "Scalp", Some(recipe(DeclaredLayout::Flow)));
    next.restore_tab(2, "Scalp", None);
    assert_eq!(state(&next, 1), ("Scalp".to_owned(), true));
    assert_eq!(next.unsynced(1), Some(Unsynced::Outvoted));
    assert!(next.unsaved_edits(1).is_some());
    assert!(next.autosave_winners(&[1], 1).is_empty());
    assert_eq!(state(&next, 2), ("Scalp".to_owned(), false));
}

/// The tab's bar rules, as the window reads them into a recipe.
fn on_bars(flow: &str, time: &str) -> WorkspaceRecipe {
    let tab: SavedTab = toml::from_str(&format!(
        "feed = \"mt5\"\nsymbol = \"WINV26\"\nlayout = \"time+flow\"\n\
         flow_bars = \"{flow}\"\ncontext_bars = [\"{time}\"]\n"
    ))
    .unwrap();
    capture_recipe(
        &tab,
        &LayoutBook::starter(Vec::new()),
        (BTreeMap::new(), Vec::new()),
        None,
    )
}

/// Golden: a workspace remembers its timeframes. A saved on 5m and Tick 2000,
/// B on 1m and Tick 500; back on A, the tab is handed 5m and Tick 2000.
#[test]
fn a_workspace_reopens_on_the_timeframes_it_saved() {
    let mut book = held_book();
    book.open_tab(1);
    book.observe(1, &on_bars("tick:2000", "time:300000"))
        .unwrap();
    book.save_as(1, "A").unwrap();
    book.observe(1, &on_bars("tick:500", "time:60000")).unwrap();
    assert!(
        book.tab_state(1).unwrap().modified,
        "a timeframe change is an edit"
    );
    assert_eq!(
        book.unsaved_edits(1).unwrap().changes,
        ["timeframe"],
        "and the close prompt names it"
    );
    book.save_as(1, "B").unwrap();
    book.set(1, "A").unwrap();
    let a = book.tab_copy(1).unwrap();
    assert_eq!(a.flow_bars.as_deref(), Some("tick:2000"));
    assert_eq!(a.context_panes[0].bars.as_deref(), Some("time:300000"));
    book.set(1, "B").unwrap();
    let b = book.tab_copy(1).unwrap();
    assert_eq!(b.flow_bars.as_deref(), Some("tick:500"));
    assert_eq!(b.context_panes[0].bars.as_deref(), Some("time:60000"));
}

/// The recipe's rule wins on each pane that has one; a pane it says nothing
/// of keeps the tab's, and a pane the tab lacks opens on the default.
#[test]
fn a_recipe_s_context_bars_lay_over_the_tab_s() {
    let recipe = WorkspaceRecipe {
        context_panes: vec![
            ContextPaneRecipe::default(),
            ContextPaneRecipe {
                bars: Some("time:300000".to_owned()),
                ..ContextPaneRecipe::default()
            },
        ],
        ..WorkspaceRecipe::default()
    };
    assert_eq!(
        recipe.context_bars_over(&["time:60000".to_owned()]),
        ["time:60000", "time:300000"]
    );
    assert_eq!(
        WorkspaceRecipe::default().context_bars_over(&["time:60000".to_owned()]),
        ["time:60000"]
    );
    let with_flow = WorkspaceRecipe {
        flow_bars: Some("tick:2000".to_owned()),
        ..recipe.clone()
    };
    assert_eq!(
        with_flow.rules_over(
            "tick:500",
            &["time:60000".to_owned(), "time:300000".to_owned()]
        ),
        [(0, "tick:2000")],
        "only a rule the tab does not show yet is put on it"
    );
    assert_eq!(
        recipe.rules_over("tick:500", &["time:60000".to_owned()]),
        [(2, "time:300000")]
    );
}

/// A recipe written before workspaces kept timeframes reads with none.
#[test]
fn an_old_recipe_reads_with_no_bars() {
    let old: WorkspaceRecipe =
        toml::from_str("layout = \"time+flow\"\n[[context_panes]]\n").unwrap();
    assert_eq!(old.flow_bars, None);
    assert_eq!(old.context_panes[0].bars, None);
}

/// `Default` follows the market: it never keeps a bar rule, so one market's
/// flow bar never reaches a new tab on another, and a timeframe change on a
/// tab on it is no edit.
#[test]
fn default_keeps_no_bar_rule_and_a_timeframe_on_it_is_no_edit() {
    let mut book = held_book();
    book.open_tab(1);
    let mut wider = on_bars("tick:2000", "time:300000");
    wider.split_fraction = Some(0.4);
    book.observe(1, &wider).unwrap();
    book.save_to(1).unwrap();
    let default = book.recipe(DEFAULT_WORKSPACE).unwrap();
    assert_eq!(default.flow_bars, None);
    assert_eq!(default.context_panes[0].bars, None);
    assert_eq!(default.split_fraction, Some(0.4), "the rest is saved");
    let mut narrower = on_bars("tick:500", "time:60000");
    narrower.split_fraction = Some(0.4);
    book.observe(1, &narrower).unwrap();
    assert!(
        !book.tab_state(1).unwrap().modified,
        "the market's bar is no edit"
    );
}

/// The Manage dialog greys Delete by the rule `delete` refuses by.
#[test]
fn delete_refusal_names_default_and_an_unknown_name_and_spares_the_rest() {
    let mut book = two_tabs_on_scalp();
    assert_eq!(
        book.delete_refusal(DEFAULT_WORKSPACE),
        Some(WorkspaceRefusal::DefaultProtected)
    );
    assert_eq!(
        book.delete_refusal("Nope"),
        Some(WorkspaceRefusal::UnknownWorkspace("Nope".to_owned()))
    );
    assert_eq!(
        book.delete_refusal("Scalp"),
        None,
        "in use and current, still deletable"
    );
    assert_eq!(
        book.delete(DEFAULT_WORKSPACE),
        Err(WorkspaceRefusal::DefaultProtected)
    );
    assert!(book.delete("Scalp").unwrap().changed);
    assert_eq!(
        book.delete("Scalp"),
        Err(WorkspaceRefusal::UnknownWorkspace("Scalp".to_owned())),
        "a repeat is refused"
    );
}

/// A recipe written before 13f24fa3 kept its intervals apart: read, they land
/// on the panes, and Default still names none.
#[test]
fn an_old_book_s_context_bars_land_on_the_panes() {
    let book = parse_book(concat!(
        "version = 1\n",
        "[recipes.Scalp]\nlayout = \"time+flow\"\ncontext_bars = [\"time:5m\"]\n",
        "[[recipes.Scalp.context_panes]]\n",
        "[recipes.Default]\nlayout = \"time+flow\"\ncontext_bars = [\"time:1m\"]\n",
        "[[recipes.Default.context_panes]]\n",
    ))
    .unwrap();
    let scalp = book.recipe("Scalp").unwrap();
    assert_eq!(scalp.context_panes[0].bars.as_deref(), Some("time:5m"));
    assert!(scalp.legacy_context_bars.is_empty());
    assert_eq!(
        book.recipe(DEFAULT_WORKSPACE).unwrap().context_panes[0].bars,
        None
    );
}

/// Tabs 1 and 2 on Scalp, autosave off, a Swing to switch to; each tab's copy
/// edited by `edit`.
fn switch_away(edit: impl Fn(u64, &mut WorkspaceRecipe)) -> SetOutcome {
    let mut book = two_tabs_on_scalp();
    book.open_tab(3);
    book.observe(3, &recipe(DeclaredLayout::Flow)).unwrap();
    book.save_as(3, "Swing").unwrap();
    for tab in [1, 2] {
        let mut copy = book.tab_copy(tab).unwrap().clone();
        edit(tab, &mut copy);
        book.observe(tab, &copy).unwrap();
    }
    book.set(1, "Swing").unwrap()
}

/// A footprint style is the tab's: a switch away drops it and says so, even
/// while the other tab holds edits of its own.
#[test]
fn a_switch_drops_a_tabs_own_edit_beside_another_tabs() {
    let dropped = switch_away(|tab, copy| {
        copy.footprint_style = Some(format!("style-{tab}"));
    });
    assert!(dropped.dropped_edits);
}

/// An indicator edit both tabs hold stays with the tab still there, but a
/// footprint edit of the leaving tab's own is dropped, and says so.
#[test]
fn a_switch_drops_its_own_edit_beside_a_shared_indicator_edit() {
    let dropped = switch_away(|tab, copy| {
        copy.flow_indicators = vec![ema()];
        if tab == 1 {
            copy.footprint_style = Some("mine".to_owned());
        }
    });
    assert!(dropped.dropped_edits);
}

/// The leaving tab holds only the indicator edit the other still shows:
/// nothing of it is dropped, and nothing is said.
#[test]
fn a_switch_keeps_quiet_over_an_indicator_edit_another_tab_holds() {
    let dropped = switch_away(|_, copy| copy.flow_indicators = vec![ema()]);
    assert!(!dropped.dropped_edits);
}
