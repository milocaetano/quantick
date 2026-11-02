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

fn recipe(layout: DeclaredLayout) -> WorkspaceRecipe {
    WorkspaceRecipe {
        layout,
        ..WorkspaceRecipe::default()
    }
}

/// `Scalp` holds the time+flow canvas; tabs 1 and 2 use it, clean.
fn two_tabs_on_scalp() -> WorkspaceBook {
    let mut book = WorkspaceBook::default();
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
    let mut book = WorkspaceBook::default();
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

#[test]
fn revert_goes_back_to_the_recipe_once() {
    let mut book = two_tabs_on_scalp();
    book.observe(1, &recipe(DeclaredLayout::Time)).unwrap();
    assert!(book.revert(1).unwrap());
    assert_eq!(
        book.tab_copy(1).unwrap().layout,
        DeclaredLayout::TimeAndFlow
    );
    assert!(!book.revert(1).unwrap());
}

#[test]
fn rename_carries_its_tabs_and_default_cannot_be_renamed() {
    let mut book = two_tabs_on_scalp();
    assert!(book.rename("Scalp", "Open").unwrap());
    assert_eq!(state(&book, 2), ("Open".to_owned(), false));
    assert!(!book.rename("Open", "Open").unwrap());
    assert!(
        book.rename("Open", "OPEN").unwrap(),
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
    assert!(!book.delete("Scalp").unwrap().changed);
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
    let mut book = WorkspaceBook::default();
    let live = recipe(DeclaredLayout::TimeTimeAndFlow);
    let seed = |tab, symbol: &str| SeedTab {
        tab,
        symbol: symbol.to_owned(),
        live: live.clone(),
    };
    let seeded = book.seed_first_run(vec![seed(1, "WINV26"), seed(2, "WINV26"), seed(3, "BTC")]);
    assert_eq!(
        seeded,
        [
            (1, "from WINV26".to_owned()),
            (2, "from WINV26 2".to_owned()),
            (3, "from BTC".to_owned())
        ]
    );
    for tab in 1..=3 {
        assert!(!book.tab_state(tab).unwrap().modified);
        assert_eq!(book.tab_copy(tab), Some(&live));
    }
    assert_eq!(book.recipe("from WINV26"), Some(&live));
    assert!(
        book.seed_first_run(vec![seed(1, "WINV26")]).is_empty(),
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
    let live = capture_recipe(&tab, &layouts, BTreeMap::new(), None);
    assert_eq!(live.layout, DeclaredLayout::TimeAndFlow);
    assert_eq!(live.context_bars, ["time:1m"]);
    assert_eq!(live.split_fraction, Some(0.25));
    assert_eq!(
        live.flow_indicators,
        [ema()],
        "an unknown id reads the active"
    );
    assert_eq!(live.context_indicators[0].indicators, [ema()]);

    let mut book = WorkspaceBook::default();
    book.seed_first_run(vec![SeedTab {
        tab: 1,
        symbol: tab.symbol.clone(),
        live: live.clone(),
    }]);
    assert_eq!(state(&book, 1), ("from WINV26".to_owned(), false));

    layouts.get_mut(LayoutId(1)).unwrap().indicators.clear();
    assert_eq!(
        book.recipe("from WINV26").unwrap().flow_indicators,
        [ema()],
        "an edit on the shared layout does not reach the recipe"
    );
}

#[test]
fn applying_a_recipe_keeps_the_markets_own_fields() {
    let mut tab: SavedTab = toml::from_str(
        "feed = \"mt5\"\nsymbol = \"WINV26\"\nlayout = \"flow\"\nflow_bars = \"tick:50\"\n",
    )
    .unwrap();
    let mut scalp = recipe(DeclaredLayout::TimeAndFlow);
    scalp.context_bars = vec!["time:5m".to_owned()];
    apply_canvas(&scalp, &mut tab);
    assert_eq!(tab.layout, DeclaredLayout::TimeAndFlow);
    assert_eq!(tab.time_bars.as_deref(), Some("time:5m"));
    assert_eq!(tab.flow_bars, "tick:50");
    assert_eq!(tab.symbol, "WINV26");
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
    reopened.restore_tab(2, &name.unwrap(), copy);
    reopened.restore_tab(1, "Scalp", None);
    assert_eq!(state(&reopened, 2), ("Scalp".to_owned(), true));
    assert_eq!(state(&reopened, 1), ("Scalp".to_owned(), false));
    reopened.restore_tab(3, "Gone", Some(recipe(DeclaredLayout::Time)));
    assert_eq!(state(&reopened, 3), ("Default".to_owned(), true));
    reopened.restore_tab(4, "Gone", None);
    assert_eq!(state(&reopened, 4), ("Default".to_owned(), false));
}

#[test]
fn a_full_recipe_round_trips_through_the_file_exactly() {
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    let live = WorkspaceRecipe {
        layout: DeclaredLayout::TimeTimeAndFlow,
        split_fraction: Some(0.1 + 0.2),
        context_heights: vec![0.3, 0.7],
        context_panes_collapsed: vec![false, true],
        context_bars: vec!["time:1m".to_owned(), "time:1h".to_owned()],
        flow_indicators: vec![ema()],
        context_indicators: vec![
            ContextPaneIndicators {
                indicators: vec![ema()],
            },
            ContextPaneIndicators {
                indicators: Vec::new(),
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
