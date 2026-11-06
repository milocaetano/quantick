use super::*;
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
}

/// The hot path: once a tab has settled, a frame where nothing moved builds
/// no recipe. Before the signal it built one per observed tab per frame.
#[test]
fn an_idle_frame_builds_no_recipe() {
    let mut host = OneTab::default();
    let _ = maintain(&mut host);
    let _ = maintain(&mut host);
    host.builds.set(0);
    for _ in 0..5 {
        assert_eq!(maintain(&mut host), UpkeepSave::Nothing);
    }
    assert_eq!(host.builds.get(), 0, "five idle frames");
    host.shows.split_fraction = Some(0.4);
    assert_eq!(
        maintain(&mut host),
        UpkeepSave::Nothing,
        "an edit is not a seed"
    );
    assert_eq!(host.builds.get(), 1, "an edit is read once");
    assert!(host.book.tab_state(7).unwrap().modified);
}

/// The desk named a workspace deleted since, and kept no copy because the
/// tab was clean: the tab keeps what it shows, marked, on `Default`.
#[test]
fn a_tab_whose_workspace_is_gone_keeps_what_it_shows_as_default_marked() {
    let mut host = OneTab::default();
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
        maintain(&mut host),
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
    assert_eq!(maintain(&mut host), UpkeepSave::SeededBook);
    assert_eq!(maintain(&mut host), UpkeepSave::Nothing, "seeded once");
    assert_eq!(host.applied, 0, "a seeded tab keeps every pixel it had");
    let state = host.book.tab_state(7).expect("the tab was adopted");
    assert_eq!(state.workspace, "from WINV26");
    assert!(!state.modified);
}

#[test]
fn an_edit_on_a_seeded_tab_marks_it_modified() {
    let mut host = OneTab::default();
    let _ = maintain(&mut host);
    host.shows.split_fraction = Some(0.25);
    let _ = maintain(&mut host);
    let state = host.book.tab_state(7).expect("the tab was adopted");
    assert!(state.modified, "the edit became this tab's copy");
}

/// A recipe shown on a settled tab is laid on it and settles at once.
#[test]
fn a_shown_recipe_is_laid_on_a_settled_tab() {
    let mut host = OneTab::default();
    let _ = maintain(&mut host);
    let recipe = WorkspaceRecipe {
        split_fraction: Some(0.3),
        ..WorkspaceRecipe::default()
    };
    show_recipe(&mut host, 7, &recipe);
    assert_eq!(host.applied, 1);
    assert_eq!(host.shows, recipe);
    assert!(!host.sync.is_pending(7), "the recipe arrived");
}
