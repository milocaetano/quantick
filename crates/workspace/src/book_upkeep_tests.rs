use super::*;
use crate::market_settings::{MarketSettings, parse_settings};
use crate::workspace_sync::AUTOSAVE_QUIET;
use std::cell::Cell;

/// One tab, always settled, whose panes show whatever `shows` says.
#[derive(Default)]
struct OneTab {
    book: WorkspaceBook,
    sync: TabRecipeSync,
    shows: WorkspaceRecipe,
    applied: usize,
    /// How many recipes were built off the tab.
    builds: Cell<usize>,
    layout: Option<String>,
    market: Option<MarketSettingsStore>,
}

impl OneTab {
    /// Whether the tab's cell reads marked.
    fn marked(&self) -> bool {
        self.book.tab_state(7).is_some_and(|state| state.modified)
    }
}

impl TabRecipeHost for OneTab {
    fn recipe_book(&self) -> (&WorkspaceBook, &TabRecipeSync) {
        (&self.book, &self.sync)
    }
    fn recipe_book_mut(&mut self) -> (&mut WorkspaceBook, &mut TabRecipeSync) {
        (&mut self.book, &mut self.sync)
    }
    fn recipe_tabs(&self) -> Vec<u64> {
        vec![7]
    }
    fn active_recipe_tab(&self) -> u64 {
        7
    }
    fn tab_settled(&self, _tab: u64) -> bool {
        true
    }
    fn live_recipe(&self, _tab: u64) -> Option<WorkspaceRecipe> {
        self.builds.set(self.builds.get() + 1);
        Some(self.shows.clone())
    }
    fn recipe_tab_symbol(&self, _tab: u64) -> Option<String> {
        Some("WINV26".to_owned())
    }
    fn recipe_tab_layout(&self, _tab: u64) -> Option<String> {
        self.layout.clone()
    }
    fn new_tab_recipe(&self, _tab: u64) -> WorkspaceRecipe {
        WorkspaceRecipe::default()
    }
    fn recipe_signal(&self, _tab: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::hash::DefaultHasher::new();
        format!("{:?}", self.shows).hash(&mut hash);
        hash.finish()
    }
    fn pin_tab_footprint(&mut self, _tab: u64) {}
    fn apply_recipe_canvas(&mut self, _tab: u64, recipe: &WorkspaceRecipe) {
        self.shows = recipe.clone();
        self.applied += 1;
    }
    fn apply_recipe_switches(&mut self, _tab: u64, _recipe: &WorkspaceRecipe) {}
    fn apply_recipe_indicators(&mut self, _tab: u64, _recipe: &WorkspaceRecipe) {}
    fn tab_drawing_keys(&self, _tab: u64) -> Vec<DrawingKey> {
        vec![flow_key()]
    }
    fn recipe_market(&mut self) -> Option<&mut MarketSettingsStore> {
        self.market.as_mut()
    }
}

/// One frame's upkeep, now.
fn frame(host: &mut OneTab) -> Upkeep {
    maintain(host, Instant::now())
}

/// The hot path: once a tab has settled, a frame where nothing moved builds
/// no recipe. Before the signal it built one per observed tab per frame.
#[test]
fn an_idle_frame_builds_no_recipe() {
    let mut host = OneTab::default();
    let _ = frame(&mut host);
    let _ = frame(&mut host);
    host.builds.set(0);
    for _ in 0..5 {
        assert_eq!(frame(&mut host).save, UpkeepSave::Nothing);
    }
    assert_eq!(host.builds.get(), 0, "five idle frames");
    host.shows.split_fraction = Some(0.4);
    assert_eq!(
        frame(&mut host).save,
        UpkeepSave::Nothing,
        "an edit is not a seed"
    );
    assert_eq!(host.builds.get(), 1, "an edit is read once");
    assert!(host.book.differs(7));
}

/// The desk named a workspace deleted since, and kept no copy because the
/// tab was clean: the tab keeps what it shows, marked, on `Default`.
#[test]
fn a_tab_whose_workspace_is_gone_keeps_what_it_shows_as_default_marked() {
    let mut host = OneTab::default();
    host.book.set_autosave(false);
    let saved = WorkspaceRecipe {
        split_fraction: Some(0.5),
        ..WorkspaceRecipe::default()
    };
    host.book.open_tab(1);
    host.book.observe(1, &saved).unwrap();
    host.book.save_to(1).unwrap();
    host.book.close_tab(1);
    assert!(!host.book.recipe("Default").unwrap().is_factory());
    host.shows.split_fraction = Some(0.2);
    host.sync.arrive(7, Some("Deleted since".to_owned()), None);
    assert_eq!(
        frame(&mut host).save,
        UpkeepSave::Nothing,
        "nothing was seeded"
    );
    assert_eq!(host.applied, 0, "Default's recipe was not laid over it");
    let state = host.book.tab_state(7).expect("the tab was restored");
    assert_eq!(state.workspace, "Default");
    assert!(state.modified, "Default •");
    assert_eq!(host.book.tab_copy(7), Some(&host.shows));
}

/// A first run seeds the tab, applies nothing to it, and asks for the book to
/// be saved once: the next frame has nothing to write.
#[test]
fn a_first_run_seeds_the_tab_and_applies_nothing_to_it() {
    let mut host = OneTab::default();
    assert_eq!(frame(&mut host).save, UpkeepSave::SeededBook);
    assert_eq!(frame(&mut host).save, UpkeepSave::Nothing, "seeded once");
    assert_eq!(host.applied, 0, "a seeded tab keeps every pixel it had");
    let state = host.book.tab_state(7).expect("the tab was adopted");
    assert_eq!(state.workspace, "WINV26", "the bare symbol, no `from `");
    assert!(!state.modified);
}

#[test]
fn an_edit_on_a_seeded_tab_marks_it_modified() {
    let mut host = OneTab::default();
    host.book.set_autosave(false);
    let _ = frame(&mut host);
    host.shows.split_fraction = Some(0.25);
    let _ = frame(&mut host);
    let state = host.book.tab_state(7).expect("the tab was adopted");
    assert!(state.modified, "the edit became this tab's copy");
}

/// A recipe shown on a settled tab is laid on it and settles at once.
#[test]
fn a_shown_recipe_is_laid_on_a_settled_tab() {
    let mut host = OneTab::default();
    let _ = frame(&mut host);
    let recipe = WorkspaceRecipe {
        split_fraction: Some(0.3),
        ..WorkspaceRecipe::default()
    };
    show_recipe(&mut host, 7, &recipe);
    assert_eq!(host.applied, 1);
    assert_eq!(host.shows, recipe);
    assert!(!host.sync.is_pending(7), "the recipe arrived");
}

#[test]
fn a_first_run_names_the_workspace_after_the_tab_layout() {
    let mut host = OneTab {
        layout: Some("Fluxo".to_owned()),
        ..OneTab::default()
    };
    let _ = frame(&mut host);
    let state = host.book.tab_state(7).expect("the tab was adopted");
    assert_eq!(state.workspace, "Fluxo");
    assert!(!host.marked());
}

/// Autosave on: an edit never reads as held, and is written through once
/// it has stood quiet for a second, not before.
#[test]
fn autosave_writes_an_edit_through_after_a_quiet_second_and_never_marks() {
    let mut host = OneTab::default();
    let start = Instant::now();
    let _ = maintain(&mut host, start);
    let _ = maintain(&mut host, start);
    host.shows.split_fraction = Some(0.3);
    let inside = maintain(&mut host, start).write_through;
    assert!(inside.is_empty(), "not inside the second");
    assert!(!host.marked(), "never marked");
    let due = maintain(&mut host, start + AUTOSAVE_QUIET).write_through;
    assert_eq!(due, vec![7]);
    let again = maintain(&mut host, start + AUTOSAVE_QUIET * 2).write_through;
    assert!(again.is_empty(), "taken once");
}

/// Autosave off: the edit is held, marked, and nothing is due.
#[test]
fn autosave_off_holds_the_edit_marked_and_writes_nothing() {
    let mut host = OneTab::default();
    host.book.set_autosave(false);
    let start = Instant::now();
    let _ = maintain(&mut host, start);
    let _ = maintain(&mut host, start);
    host.shows.split_fraction = Some(0.3);
    let _ = maintain(&mut host, start);
    let late = maintain(&mut host, start + AUTOSAVE_QUIET * 3).write_through;
    assert!(late.is_empty());
    assert!(host.marked(), "the cell's mark");
    let edits = host.book.unsaved_edits(7).expect("closing asks");
    assert_eq!(edits.changes, vec!["panes".to_owned()]);
}

const DRAWN: &str = "version = 1\n\n[[drawings]]\nvenue = \"mt5\"\nasset = \"WINV26\"\nrole = \"flow\"\n\n[[drawings.items]]\ntool = \"horizontal-line\"\nband = { axis = \"price\" }\ncolor = [255, 200, 0, 255]\nwidth_px = 1.5\nfill_alpha = 0\npoints = [{ time_ms = 1000, price = 67000.5, bar = 3.0 }]\n";

fn flow_key() -> DrawingKey {
    DrawingKey::of_pane("mt5", "WINV26".to_owned(), None)
}

/// A tab whose saved level was erased, autosave off: the store holds it.
fn erased_level(saved: MarketSettings) -> OneTab {
    let path = std::path::PathBuf::from("market-settings.toml");
    let mut market = MarketSettingsStore::new(saved, path, false);
    market
        .settings_mut()
        .restore(&MarketSettings::default(), &flow_key());
    market.mark_changed(Instant::now());
    let mut host = OneTab {
        market: Some(market),
        ..OneTab::default()
    };
    host.book.set_autosave(false);
    host
}

/// Autosave off holds the market store and the book counts what the tab's
/// panes hold back; autosave on lets it through and nothing is counted.
#[test]
fn autosave_off_holds_the_market_store_and_counts_the_tab_s_drawings() {
    let mut host = erased_level(parse_settings(DRAWN).unwrap());
    let _ = frame(&mut host);
    assert!(host.market.as_ref().unwrap().is_held());
    assert_eq!(host.book.held_drawings(7), 1, "the erased level");
    assert_eq!(host.book.drawing_keys(7), [flow_key()]);
    host.book.set_autosave(true);
    let _ = frame(&mut host);
    assert!(!host.market.as_ref().unwrap().is_held());
    assert_eq!(host.book.held_drawings(7), 0);
}

/// Autosave off with a drawing held: an idle frame compares no drawing;
/// a change to the store is counted on the next frame.
#[test]
fn an_idle_frame_counts_no_held_drawing() {
    let saved = parse_settings(DRAWN).unwrap();
    let mut host = erased_level(saved.clone());
    let _ = frame(&mut host);
    let counted = host.market.as_ref().unwrap().held_counts();
    for _ in 0..5 {
        let _ = frame(&mut host);
    }
    let market = host.market.as_ref().unwrap();
    assert_eq!(market.held_counts(), counted, "five idle frames");
    assert_eq!(host.book.held_drawings(7), 1);
    let market = host.market.as_mut().unwrap();
    market.settings_mut().restore(&saved, &flow_key());
    let _ = frame(&mut host);
    assert_eq!(host.book.held_drawings(7), 0, "the level is back");
}

/// A command reads the tab before it acts; an edit it picks up there
/// starts the quiet second like the frame's reading, so it is written.
#[test]
fn an_edit_a_command_reads_is_written_through() {
    let mut host = OneTab::default();
    let start = Instant::now();
    let _ = maintain(&mut host, start);
    let _ = maintain(&mut host, start);
    host.shows.split_fraction = Some(0.3);
    assert!(watch_tab(&mut host, 7, start), "the command saw the edit");
    let inside = maintain(&mut host, start).write_through;
    assert!(inside.is_empty(), "not inside the second");
    let due = maintain(&mut host, start + AUTOSAVE_QUIET).write_through;
    assert_eq!(due, vec![7]);
}

/// A write-through that failed is retried a quiet second later, and the
/// tab reads modified meanwhile, autosave on or not.
#[test]
fn a_failed_write_through_is_retried_after_the_quiet_second() {
    let mut host = OneTab::default();
    let start = Instant::now();
    let _ = maintain(&mut host, start);
    let _ = maintain(&mut host, start);
    host.shows.split_fraction = Some(0.3);
    let _ = maintain(&mut host, start);
    let due = maintain(&mut host, start + AUTOSAVE_QUIET).write_through;
    assert_eq!(due, vec![7]);
    host.book.note_write_failed(7);
    assert!(host.marked(), "a failed write reads modified");
    let failed_at = start + AUTOSAVE_QUIET;
    let early = maintain(&mut host, failed_at).write_through;
    assert!(early.is_empty(), "not per frame");
    let retry = maintain(&mut host, failed_at + AUTOSAVE_QUIET).write_through;
    assert_eq!(retry, vec![7]);
}
