//! The cell menu (designer B, "workspace first"): the header on the tab's
//! workspace, the list in its stored order with each row's summary and
//! status, the filter, the footer, what each asks and the call that does the
//! same, read off a book as the window would draw them.

use quantick_control_schema::workspace_book as calls;
use quantick_session::workspace_menu::WorkspaceMenuState;
use quantick_stores::workspaces::{WorkspaceBook, WorkspaceRecipe};
use quantick_workspace::canvas::{PaneKind, PaneTree};
use quantick_workspace::indicator_document::{SavedIndicator, SavedKind};
use quantick_workspace::workspace_recipe::DeclaredLayout;

use super::*;

/// One tab on `Default`, autosave as asked.
fn book(autosave: bool) -> WorkspaceBook {
    let mut book = WorkspaceBook::default();
    book.open_tab(1);
    book.set_autosave(autosave);
    book
}

fn indicator(id: &str) -> SavedIndicator {
    SavedIndicator {
        kind: SavedKind::native(id),
        hidden: false,
        mouse_vertical_line: false,
        inputs: Vec::new(),
        plot_styles: Vec::new(),
    }
}

/// A recipe on `layout` with `count` flow indicators.
fn recipe(layout: DeclaredLayout, count: usize) -> WorkspaceRecipe {
    WorkspaceRecipe {
        layout,
        flow_indicators: (0..count).map(|_| indicator("native.ema")).collect(),
        ..WorkspaceRecipe::default()
    }
}

/// `name` saved from tab `tab` as `recipe` holds it.
fn save(book: &mut WorkspaceBook, tab: u64, name: &str, recipe: &WorkspaceRecipe) {
    book.observe(tab, recipe).unwrap();
    book.save_as(tab, name).unwrap();
}

/// A tab's words in the list: `T<id>`.
fn label(tab: u64) -> String {
    format!("T{tab}")
}

fn live() -> PaneTree {
    PaneTree::pane(0, PaneKind::Flow)
}

fn menu_of(book: &WorkspaceBook, tab: u64) -> CellMenu {
    cell_menu(book, tab, "Ctrl+S", &live(), true, label)
}

fn keys(rows: &[CellRow]) -> Vec<&str> {
    rows.iter().map(|row| row.key.as_str()).collect()
}

fn names(rows: &[CellRow]) -> Vec<&str> {
    rows.iter().map(|row| row.label.as_str()).collect()
}

/// Tab 1 on `Pair` (time+flow, nine indicators), tab 2 on it too, `Solo`
/// (flow alone, no indicator) beside them, autosave as asked.
fn desk(autosave: bool) -> WorkspaceBook {
    let mut book = book(false);
    book.open_tab(2);
    let solo = WorkspaceRecipe {
        footprint_style: Some("heatmap".to_owned()),
        ..recipe(DeclaredLayout::Flow, 0)
    };
    save(&mut book, 2, "Solo", &solo);
    save(
        &mut book,
        1,
        "Pair",
        &recipe(DeclaredLayout::TimeAndFlow, 9),
    );
    book.set(2, "Pair").unwrap();
    book.set_autosave(autosave);
    book
}

#[test]
fn the_blocks_keep_the_designed_order_and_name_their_calls() {
    let menu = menu_of(&desk(false), 1);
    assert_eq!(menu.current.kind, RowKind::Current);
    assert_eq!(menu.current.key, "current");
    assert_eq!(keys(&menu.head), ["save", "save_as", "rename"]);
    assert_eq!(
        names(&menu.head),
        ["Save", "Save as\u{2026}", "Rename\u{2026}"]
    );
    assert_eq!(menu.head[0].beside, "Ctrl+S");
    assert_eq!(keys(&menu.foot), ["autosave", "new", "manage"]);
    assert_eq!(
        names(&menu.foot),
        ["Autosave", "New workspace\u{2026}", "Manage\u{2026}"]
    );
    let calls_of =
        |rows: &[CellRow]| -> Vec<Option<&str>> { rows.iter().map(|row| row.capability).collect() };
    assert_eq!(
        calls_of(&menu.head),
        [
            Some(calls::WORKSPACE_SAVE_TO_CAPABILITY_ID),
            Some(calls::WORKSPACE_SAVE_AS_CAPABILITY_ID),
            Some(calls::WORKSPACE_RENAME_CAPABILITY_ID),
        ]
    );
    assert_eq!(
        calls_of(&menu.foot),
        [
            Some(calls::WORKSPACE_AUTOSAVE_SET_CAPABILITY_ID),
            Some(calls::WORKSPACE_NEW_CAPABILITY_ID),
            Some(calls::WORKSPACE_LIST_CAPABILITY_ID),
        ]
    );
    assert_eq!(CellRow::menu(1).scene_id(), SCENE_ID);
    assert_eq!(menu.head[0].scene_id(), "workspace_menu.save");
    assert_eq!(CellRow::menu(1).capability, Some(MENU_CAPABILITY_ID));
    assert_eq!(CAPTION, "WORKSPACES");
}

#[test]
fn inherited_intervals_enable_an_explicit_save_without_a_dirty_mark() {
    for autosave in [false, true] {
        let mut book = desk(autosave);
        let mut live = book.tab_copy(1).unwrap().clone();
        live.flow_bars = Some("time:60000".to_owned());
        book.settle_tab(1, &live);
        let menu = menu_of(&book, 1);
        assert!(!menu.current.dot);
        assert_eq!(menu.head[0].command, Some(WorkspaceCommand::SaveTo));
        assert_eq!(menu.head[0].unavailable, None);
        book.save_to(1).unwrap();
        assert!(menu_of(&book, 1).head[0].unavailable.is_some());
    }
}

/// Every command the menu had before the redesign is still a row.
#[test]
fn every_command_the_menu_had_stays_reachable() {
    let menu = menu_of(&desk(false), 2);
    let commands: Vec<WorkspaceCommand> = (menu.head.iter())
        .chain(&menu.list)
        .chain(&menu.foot)
        .filter_map(|row| row.command.clone())
        .collect();
    for command in [
        WorkspaceCommand::SaveTo,
        WorkspaceCommand::BeginSaveAs,
        WorkspaceCommand::BeginRename("Pair".to_owned()),
        WorkspaceCommand::SetAutosave(true),
        WorkspaceCommand::BeginNew,
        WorkspaceCommand::BeginManage,
        WorkspaceCommand::Set("Default".to_owned()),
        WorkspaceCommand::Set("Solo".to_owned()),
    ] {
        assert!(commands.contains(&command), "{command:?} in {commands:?}");
    }
}

/// The header: the tab's workspace, its panes, indicators and tabs.
#[test]
fn the_header_names_the_workspace_its_summary_and_its_tabs() {
    let menu = menu_of(&desk(false), 1);
    let current = &menu.current;
    assert_eq!(current.label, "Pair");
    // What the tab shows now: the live layout, a lone flow pane here.
    assert_eq!(current.summary, "Flow \u{b7} 9 ind \u{b7} 2 tabs");
    assert!(current.on);
    assert!(!current.dot, "nothing held");
    assert_eq!(current.tree, Some(live()), "the capsule's live layout");
    let one = menu_of(&book(false), 1);
    assert!(
        one.current.summary.ends_with("1 tab"),
        "{}",
        one.current.summary
    );
}

/// A row's summary names its pane kinds and its indicators, even none.
#[test]
fn a_row_s_summary_names_its_panes_and_indicators() {
    let menu = menu_of(&desk(false), 1);
    let row = |name: &str| {
        let mut rows = menu.list.iter();
        rows.find(|row| row.label == name).expect(name)
    };
    assert_eq!(row("Pair").summary, "Time \u{b7} Flow \u{b7} 9 ind");
    assert_eq!(row("Solo").summary, "Flow \u{b7} 0 ind");
    let tree = row("Pair").tree.as_ref().expect("a miniature");
    let kinds: Vec<PaneKind> = tree.panes().into_iter().map(|(_, kind)| kind).collect();
    assert!(kinds.contains(&PaneKind::Time) && kinds.contains(&PaneKind::Flow));
    assert_eq!(row("Default").kind, RowKind::Workspace);
}

/// The list keeps the stored order whichever workspace is the tab's.
#[test]
fn the_order_never_changes_with_the_selection() {
    let mut book = desk(false);
    let before: Vec<String> = menu_of(&book, 1)
        .list
        .into_iter()
        .map(|row| row.label)
        .collect();
    assert_eq!(before, ["Default", "Pair", "Solo"]);
    book.set(1, "Solo").unwrap();
    let after = menu_of(&book, 1);
    assert_eq!(names(&after.list), before);
    let lit: Vec<&str> = (after.list.iter())
        .filter(|row| row.on)
        .map(|row| row.label.as_str())
        .collect();
    assert_eq!(lit, ["Solo"]);
}

/// An edit held by another tab is told on its workspace's row: where, and
/// a dot; the tab's own edit is the header's Unsaved mark and Save.
#[test]
fn an_edit_held_by_a_tab_says_where_and_marks_its_row() {
    let mut book = desk(false);
    book.open_tab(3);
    save(&mut book, 3, "Third", &recipe(DeclaredLayout::Time, 1));
    book.set(3, "Solo").unwrap();
    book.observe(3, &recipe(DeclaredLayout::Time, 4)).unwrap();
    book.observe(1, &recipe(DeclaredLayout::TimeAndFlow, 2))
        .unwrap();
    let menu = menu_of(&book, 1);
    let solo = menu.list.iter().find(|row| row.label == "Solo").unwrap();
    assert_eq!(solo.summary, "Flow \u{b7} 0 ind \u{b7} unsaved in T3");
    assert!(solo.dot && !solo.on);
    assert!(menu.current.dot, "the header's Unsaved mark");
    let save = &menu.head[0];
    assert_eq!(save.kind, RowKind::Press);
    assert!(save.unavailable.is_none() && save.dot, "Save stands out");
    assert_eq!(save.command, Some(WorkspaceCommand::SaveTo));
}

/// Autosave on: Save is a quiet line that keeps its reason, and no
/// Unsaved mark appears anywhere.
#[test]
fn autosave_on_quiets_save_and_hides_every_unsaved_mark() {
    let mut book = desk(false);
    book.observe(2, &recipe(DeclaredLayout::TimeAndFlow, 3))
        .unwrap();
    book.set_autosave(true);
    let menu = menu_of(&book, 1);
    let save = &menu.head[0];
    assert_eq!(save.kind, RowKind::Quiet);
    assert_eq!(save.label, QUIET_SAVE);
    assert_eq!(save.unavailable, Some(AUTOSAVE_SAVES_REASON));
    assert!(!menu.current.dot);
    assert!(menu.list.iter().all(|row| !row.dot), "no dot");
    assert!(menu.list.iter().all(|row| !row.summary.contains("unsaved")));
    let switch = &menu.foot[0];
    assert_eq!(switch.kind, RowKind::Switch);
    assert!(switch.on);
    assert_eq!(switch.command, Some(WorkspaceCommand::SetAutosave(false)));
    assert_eq!(switch.summary, "On \u{b7} every change is saved");
}

#[test]
fn autosave_off_says_the_shortcut_saves_and_greys_save_without_edits() {
    let menu = menu_of(&desk(false), 1);
    let switch = &menu.foot[0];
    assert!(!switch.on);
    assert_eq!(switch.summary, "Off \u{b7} Ctrl+S saves");
    assert_eq!(switch.command, Some(WorkspaceCommand::SetAutosave(true)));
    let save = &menu.head[0];
    assert_eq!(save.kind, RowKind::Press);
    assert_eq!(save.unavailable, Some(NO_EDITS_REASON));
    assert!(!save.dot);
    let locked = menu_of(&book(false), 1);
    assert_eq!(locked.head[0].kind, RowKind::Quiet);
    assert_eq!(locked.head[0].unavailable, Some(DEFAULT_PROTECTED_REASON));
    let rename = &locked.head[2];
    assert_eq!(rename.unavailable, Some(DEFAULT_PROTECTED_REASON));
    assert!(rename.disabled_hover.is_some());
}

/// The tab's own row only closes the menu; every other row switches.
#[test]
fn the_current_row_asks_nothing_and_the_others_switch() {
    let menu = menu_of(&desk(false), 1);
    for row in &menu.list {
        assert_eq!(row.kind, RowKind::Workspace);
        assert_eq!(row.key, format!("workspace.{}", row.label));
        assert_eq!(row.capability, Some(calls::TAB_WORKSPACE_SET_CAPABILITY_ID));
        let asks = (!row.on).then(|| WorkspaceCommand::Set(row.label.clone()));
        assert_eq!(row.command, asks, "{}", row.label);
    }
}

/// `count` workspaces in all: `Default` and `Desk 01`.. made on tab 1.
fn crowded(count: usize) -> WorkspaceBook {
    let mut book = book(false);
    for n in 1..count {
        save(
            &mut book,
            1,
            &format!("Desk {n:02}"),
            &recipe(DeclaredLayout::Time, n),
        );
    }
    book
}

#[test]
fn the_filter_shows_from_thirteen_and_narrows_then_clears() {
    assert!(!menu_of(&crowded(12), 1).filters());
    let menu = menu_of(&crowded(30), 1);
    assert!(menu.filters());
    assert_eq!(menu.shown("").len(), 30, "an empty filter shows all");
    let narrowed: Vec<&str> = (menu.shown("desk 2").into_iter())
        .map(|row| row.label.as_str())
        .collect();
    assert_eq!(
        narrowed,
        [
            "Desk 20", "Desk 21", "Desk 22", "Desk 23", "Desk 24", "Desk 25", "Desk 26", "Desk 27",
            "Desk 28", "Desk 29"
        ]
    );
    assert!(menu.shown("nothing like it").is_empty());
    assert_eq!(menu.shown("  ").len(), 30, "cleared");
    let field = menu.filter_row("desk").expect("the field");
    assert_eq!(
        (field.kind, field.key.as_str()),
        (RowKind::Filter, "filter")
    );
    assert_eq!(field.label, "desk");
    assert_eq!(
        menu.filter_row("").expect("the field").label,
        FILTER_PLACEHOLDER
    );
    assert!(menu_of(&crowded(12), 1).filter_row("").is_none());
}

/// Down from the current row, then Enter: the next workspace is asked.
#[test]
fn up_down_and_enter_switch_to_the_focused_row() {
    let mut book = crowded(30);
    book.set(1, "Desk 05").unwrap();
    let menu = menu_of(&book, 1);
    let mut state = WorkspaceMenuState::default();
    state.toggle_chip_menu(1);
    let shown = menu.shown(&state.filter);
    let current = shown.iter().position(|row| row.on);
    state.move_focus(true, shown.len(), current);
    let chosen = state.enter(shown.len()).expect("a focused row");
    assert_eq!(current, Some(5));
    let next = "Desk 06".to_owned();
    assert_eq!(shown[chosen].label, next);
    assert_eq!(shown[chosen].command, Some(WorkspaceCommand::Set(next)));
    state.filter = "desk 07".to_owned();
    state.filter_changed();
    let shown = menu.shown(&state.filter);
    let chosen = state.enter(shown.len()).expect("the first match");
    assert_eq!(shown[chosen].label, "Desk 07");
}

/// The scene reads the whole menu: the header with its summary, each row
/// with its summary and status, the filter and the footer.
#[test]
fn the_scene_reads_the_header_rows_filter_and_footer() {
    let mut book = desk(false);
    book.observe(1, &recipe(DeclaredLayout::TimeAndFlow, 2))
        .unwrap();
    let menu = menu_of(&book, 1);
    let current = menu.current.scene_control("toolbar", None);
    assert_eq!(current.control_id, "toolbar.workspace_menu.current");
    assert_eq!(current.label, "Pair");
    let said = current.description.expect("the summary");
    assert!(
        said.contains("2 tabs") && said.contains("Unsaved"),
        "{said}"
    );
    let pair = menu.list.iter().find(|row| row.on).unwrap();
    let pair = pair.scene_control("toolbar", None);
    assert_eq!(pair.control_id, "toolbar.workspace_menu.workspace.Pair");
    assert!(pair.selected, "the current one");
    assert!(pair.description.unwrap().starts_with("Time"));
    let switch = menu.foot[0].scene_control("toolbar", None);
    assert_eq!(
        switch.description.as_deref(),
        Some("Off \u{b7} Ctrl+S saves")
    );
    let menu_row = CellRow::menu(menu.list.len()).scene_control("toolbar", None);
    assert_eq!(menu_row.description.as_deref(), Some("3 workspaces"));
}

#[test]
fn a_greyed_row_is_listed_unavailable_with_its_reason() {
    let menu = menu_of(&desk(false), 1);
    let save = menu.head[0].scene_control("toolbar", None);
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

/// A long name is told whole on hover while cut short; a short one is not.
#[test]
fn a_cut_name_reads_whole_on_hover() {
    let menu = menu_of(&desk(false), 1);
    let solo = menu.list.iter().find(|row| row.label == "Solo").unwrap();
    assert_eq!(solo.tip(true), Some("Solo"));
    assert_eq!(solo.tip(false), None);
}

/// The header describes what the tab shows now, its edit included, beside
/// its live miniature and its Unsaved mark.
#[test]
fn the_header_describes_what_the_tab_shows_now() {
    let mut book = desk(false);
    book.observe(1, &recipe(DeclaredLayout::TimeAndFlow, 2))
        .unwrap();
    let menu = menu_of(&book, 1);
    assert_eq!(menu.current.summary, "Flow \u{b7} 2 ind \u{b7} 2 tabs");
    assert_eq!(menu.current.tree, Some(live()));
    assert!(menu.current.dot);
}

/// A row says what `workspace.list` says of the same workspace.
#[test]
fn a_row_says_what_the_workspace_list_says() {
    let book = crowded(5);
    for row in &menu_of(&book, 1).list {
        assert_eq!(row.summary, book.summary(&row.label), "{}", row.label);
    }
}

/// An edit only this tab holds ends its row with just `unsaved`: the
/// header already says where.
#[test]
fn an_edit_held_only_here_says_unsaved() {
    let mut book = desk(false);
    book.observe(1, &recipe(DeclaredLayout::TimeAndFlow, 2))
        .unwrap();
    let menu = menu_of(&book, 1);
    let pair = menu.list.iter().find(|row| row.label == "Pair").unwrap();
    let end = format!("\u{b7} {UNSAVED_HERE}");
    assert!(pair.summary.ends_with(&end), "{}", pair.summary);
    assert!(!pair.summary.contains("unsaved in"));
}

/// Holders are named once each; two that read alike are counted instead.
#[test]
fn holders_are_named_once_or_counted_when_they_read_alike() {
    let mut book = desk(false);
    book.observe(1, &recipe(DeclaredLayout::TimeAndFlow, 2))
        .unwrap();
    book.observe(2, &recipe(DeclaredLayout::TimeAndFlow, 3))
        .unwrap();
    let pair = |menu: CellMenu| {
        menu.list
            .into_iter()
            .find(|row| row.label == "Pair")
            .unwrap()
    };
    let named = pair(cell_menu(&book, 1, "Ctrl+S", &live(), true, label));
    assert!(
        named.summary.ends_with("unsaved in T1, T2"),
        "{}",
        named.summary
    );
    let alike = pair(cell_menu(&book, 1, "Ctrl+S", &live(), true, |_| {
        "WINZ25".to_owned()
    }));
    assert!(
        alike.summary.ends_with("unsaved in 2 tabs"),
        "{}",
        alike.summary
    );
}

/// A quiet frame borrows the last build; a change to the book builds again.
#[test]
fn the_rows_are_built_again_only_when_the_book_changes() {
    let mut book = desk(false);
    let mut cache = CellMenuCache::default();
    for _ in 0..3 {
        cache.menu(&book, 1, "Ctrl+S", &live(), true, label);
    }
    assert_eq!(cache.builds, 1, "quiet frames");
    book.observe(1, &recipe(DeclaredLayout::TimeAndFlow, 2))
        .unwrap();
    let menu = cache.menu(&book, 1, "Ctrl+S", &live(), true, label).clone();
    assert_eq!(cache.builds, 2, "the edit");
    assert_eq!(menu, menu_of(&book, 1));
    cache.menu(&book, 1, "Ctrl+Alt+S", &live(), true, label);
    assert_eq!(cache.builds, 3, "the shortcut's words");
}

/// Each row and the header carry their workspace's colour for the theme, and
/// the scene ends their description with its name and hex; another theme
/// builds the rows again.
#[test]
fn every_row_carries_its_colour_and_the_scene_names_it() {
    use quantick_stores::workspaces::WorkspaceColour;
    let mut book = desk(false);
    let custom = WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]);
    book.set_colour("Solo", custom, false).unwrap();
    let sky = WorkspaceColour::preset(2).unwrap();
    book.set_colour("Pair", sky, false).unwrap();
    let dark = menu_of(&book, 1);
    let colour = |row: &CellRow| row.colour.clone().expect("a colour");
    let pair = dark.list.iter().find(|row| row.label == "Pair").unwrap();
    assert_eq!(colour(pair).rgb, sky.rgb(true));
    assert_eq!(colour(&dark.current).rgb, sky.rgb(true), "tab 1 is on Pair");
    let solo = dark.list.iter().find(|row| row.label == "Solo").unwrap();
    assert_eq!(colour(solo).rgb, [0x3A, 0x7B, 0xD5]);
    let said = pair.scene_control("toolbar", None).description.unwrap();
    assert!(said.ends_with(" \u{b7} Sky #5DBDEB"), "{said}");
    let said = solo.scene_control("toolbar", None).description.unwrap();
    assert!(said.ends_with(" \u{b7} Custom #3A7BD5"), "{said}");
    let said = dark.current.scene_control("toolbar", None).description;
    assert!(said.unwrap().ends_with("Sky #5DBDEB"));
    assert!(dark.head.iter().all(|row| row.colour.is_none()), "buttons");
    let light = cell_menu(&book, 1, "Ctrl+S", &live(), false, label);
    let pair = light.list.iter().find(|row| row.label == "Pair").unwrap();
    assert_eq!(colour(pair).rgb, sky.rgb(false));
    let mut cache = CellMenuCache::default();
    cache.menu(&book, 1, "Ctrl+S", &live(), true, label);
    cache.menu(&book, 1, "Ctrl+S", &live(), false, label);
    assert_eq!(cache.builds, 2, "the theme");
}

/// A row with no summary still names its colour in the scene.
#[test]
fn a_row_without_a_summary_still_names_its_colour() {
    let menu = menu_of(&desk(false), 1);
    let mut row = menu.list[0].clone();
    row.summary.clear();
    let said = row.scene_control("toolbar", None).description;
    let colour = row.colour.clone().expect("a colour");
    assert_eq!(said, Some(colour.words));
    let mut header = menu.current.clone();
    header.summary.clear();
    header.dot = true;
    let said = header.scene_control("toolbar", None).description.unwrap();
    assert!(
        said.starts_with(UNSAVED) && said.contains(" \u{b7} "),
        "{said}"
    );
}

/// The header's swatch opens the palette popover, names the call that does
/// the same and says its colour on hover: a preset by name, a custom colour
/// by its hex.
#[test]
fn the_header_swatch_opens_the_popover_and_says_its_colour() {
    use quantick_session::colour_popover::ColourIntent;
    use quantick_stores::workspaces::WorkspaceColour;
    let mut book = desk(false);
    book.set_colour("Pair", WorkspaceColour::preset(2).unwrap(), false)
        .unwrap();
    let menu = menu_of(&book, 1);
    let swatch = &menu.swatch;
    assert_eq!(swatch.scene_id(), "workspace_menu.colour");
    assert_eq!(
        swatch.command,
        Some(WorkspaceCommand::Colour(ColourIntent::Toggle))
    );
    assert_eq!(swatch.capability, Some("workspace.colour.popover"));
    assert_eq!(swatch.tip(false), Some("Sky \u{b7} change colour"));
    assert_eq!(swatch.colour.as_ref().unwrap().rgb, [0x5D, 0xBD, 0xEB]);
    book.set_colour("Pair", WorkspaceColour::Custom([0x3A, 0x7B, 0xD5]), false)
        .unwrap();
    let tip = menu_of(&book, 1).swatch.tip(false).map(str::to_owned);
    assert_eq!(tip.as_deref(), Some("Custom #3A7BD5 \u{b7} change colour"));
}
