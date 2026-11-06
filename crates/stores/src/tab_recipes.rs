//! What a frame owes the workspace book: adopting the open tabs, carrying each
//! recipe on its way and noticing edits. The window only reads a tab into a
//! recipe and puts a recipe on a tab, through [`TabRecipeHost`]; every decision
//! is `quantick_workspace::workspace_sync`'s or the book's.

use crate::workspaces::{SeedTab, TabAdoption, WorkspaceBookStore, WorkspaceRecipe};
use std::time::Instant;

/// What the window lends the book's upkeep.
pub trait TabRecipeHost {
    fn recipe_store(&self) -> &WorkspaceBookStore;
    fn recipe_store_mut(&mut self) -> &mut WorkspaceBookStore;
    /// The open tabs, in strip order.
    fn recipe_tabs(&self) -> Vec<u64>;
    fn active_recipe_tab(&self) -> u64;
    /// The tab's panes stand: seeded, and the context charts it asks for built.
    fn tab_settled(&self, tab: u64) -> bool;
    /// What the tab shows now, as a recipe.
    fn live_recipe(&self, tab: u64) -> Option<WorkspaceRecipe>;
    fn recipe_tab_symbol(&self, tab: u64) -> Option<String>;
    /// The name of the layout the tab shows, which a first run names its
    /// workspace after.
    fn recipe_tab_layout(&self, tab: u64) -> Option<String>;
    /// What a new tab on this tab's feed opens with: what the factory
    /// `Default` amounts to here.
    fn new_tab_recipe(&self, tab: u64) -> WorkspaceRecipe;
    /// A cheap reading of everything [`Self::live_recipe`] reads, which moves
    /// whenever any of it may have. An idle frame compares this and builds no
    /// recipe.
    fn recipe_signal(&self, tab: u64) -> u64;
    /// The footprint style is the tab's: each pane holds the style it draws
    /// with now, and only the style, so an edit on another tab reaches every
    /// other knob here but never the style. No pixel changes.
    fn pin_tab_footprint(&mut self, tab: u64);
    /// The canvas and the context intervals. Drawings, anchored studies, the flow
    /// bar size and the paper position are not touched: they are the market's.
    fn apply_recipe_canvas(&mut self, tab: u64, recipe: &WorkspaceRecipe);
    /// Layers and the footprint style, on every pane the tab has now.
    fn apply_recipe_switches(&mut self, tab: u64, recipe: &WorkspaceRecipe);
    /// Each pane by role takes the recipe's set, onto a layout entry only it holds.
    fn apply_recipe_indicators(&mut self, tab: u64, recipe: &WorkspaceRecipe);
}

/// What the frame owes the book, once its panes are built and seeded. Answers
/// the tabs autosave writes through now: edits quiet for a second, never while
/// it is off.
pub fn maintain<H: TabRecipeHost + ?Sized>(host: &mut H, now: Instant) -> Vec<u64> {
    adopt_tabs(host);
    apply_pending_recipes(host);
    let active = host.active_recipe_tab();
    let tabs = host.recipe_tabs();
    let other = host.recipe_store_mut().sync_mut().next_watch(&tabs, active);
    for id in std::iter::once(active).chain(other) {
        if observe_tab(host, id) {
            host.recipe_store_mut().sync_mut().note_edit(id, now);
        }
    }
    let store = host.recipe_store_mut();
    let due = store.sync_mut().take_due(now);
    if store.book().autosave() {
        due.into_iter()
            .filter(|tab| store.book().differs(*tab))
            .collect()
    } else {
        Vec::new()
    }
}

fn adopt_tabs<H: TabRecipeHost + ?Sized>(host: &mut H) {
    let mut seeds = Vec::new();
    let mut all_settled = true;
    for id in host.recipe_tabs() {
        let store = host.recipe_store();
        let held = store.book().tab_state(id).is_some();
        if held && !store.sync().has_arrival(id) {
            continue;
        }
        if !host.tab_settled(id) {
            all_settled = false;
            continue;
        }
        let store = host.recipe_store_mut();
        let unused = store.book().is_unused();
        let adoption = store.sync_mut().adoption(id, held, unused);
        match adoption {
            TabAdoption::Held => {}
            TabAdoption::Restore { workspace, copy } => restore(host, id, &workspace, copy),
            TabAdoption::OnDefault => {
                let store = host.recipe_store_mut();
                let recipe = store.book_mut().open_tab(id).clone();
                if !recipe.is_factory() {
                    store.sync_mut().queue(id, recipe);
                }
            }
            TabAdoption::Seed => {
                host.pin_tab_footprint(id);
                let live = host.live_recipe(id);
                let symbol = host.recipe_tab_symbol(id);
                if let (Some(live), Some(symbol)) = (live, symbol) {
                    seeds.push(SeedTab {
                        tab: id,
                        layout: host.recipe_tab_layout(id),
                        symbol,
                        live,
                    });
                }
            }
        }
    }
    if all_settled {
        host.recipe_store_mut().sync_mut().finish_first_pass();
    }
    if seeds.is_empty() {
        return;
    }
    let store = host.recipe_store_mut();
    for seed in &seeds {
        store.sync_mut().settle(seed.tab, seed.live.clone());
    }
    store.book_mut().seed_first_run(seeds);
    if let Err(reason) = store.persist() {
        tracing::warn!(target: "quantick::app", schema_version = 1_u8,
            event_code = "WORKSPACES_SEED_NOT_SAVED", reason = %reason,
            action = "seeded_for_this_session_only",
            "the workspaces seeded from the open tabs were not saved");
    }
}

/// A tab the desk brought back. On a workspace the book still has, it is
/// carried to its copy; on one deleted since, it keeps what it shows, as
/// `Default •`, rather than take `Default`'s recipe.
fn restore<H: TabRecipeHost + ?Sized>(
    host: &mut H,
    id: u64,
    workspace: &str,
    copy: Option<Box<WorkspaceRecipe>>,
) {
    let lost = copy.is_none() && host.recipe_store().book().resolve(workspace).is_none();
    let copy = match copy {
        Some(copy) => Some(*copy),
        None if lost => host.live_recipe(id),
        None => None,
    };
    let store = host.recipe_store_mut();
    store.book_mut().restore_tab(id, workspace, copy);
    if lost {
        settle(host, id);
    } else if let Some(recipe) = store.book().tab_copy(id).filter(|copy| !copy.is_factory()) {
        let recipe = recipe.clone();
        store.sync_mut().queue(id, recipe);
    }
}

/// The tab's panes stand: what it shows is what later edits are told from.
fn settle<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64) {
    host.pin_tab_footprint(id);
    let signal = host.recipe_signal(id);
    let Some(live) = host.live_recipe(id) else {
        return;
    };
    let store = host.recipe_store_mut();
    store.book_mut().settle_tab(id, &live);
    store.sync_mut().settle(id, live);
    store.sync_mut().note_signal(id, signal);
}

/// Queue `recipe` for a tab and carry it as far as the panes allow. The
/// factory `Default` is laid as a new tab on its feed: never a bare canvas.
pub fn show_recipe<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64, recipe: &WorkspaceRecipe) {
    let shown = if recipe.is_factory() {
        host.new_tab_recipe(id)
    } else {
        recipe.clone()
    };
    host.recipe_store_mut().sync_mut().queue(id, shown);
    apply_pending_recipes(host);
}

/// Carry each recipe on its way as far as the tab's panes allow.
pub fn apply_pending_recipes<H: TabRecipeHost + ?Sized>(host: &mut H) {
    for id in host.recipe_store().sync().pending_tabs() {
        let arrived = host.tab_settled(id)
            && host.live_recipe(id).as_ref() == host.recipe_store().sync().pending_recipe(id);
        if !arrived && let Some(recipe) = host.recipe_store_mut().sync_mut().take_canvas(id) {
            host.apply_recipe_canvas(id, &recipe);
        }
        if !host.tab_settled(id) {
            continue;
        }
        let Some(recipe) = host.recipe_store_mut().sync_mut().finish(id) else {
            continue;
        };
        if !arrived {
            host.apply_recipe_switches(id, &recipe);
            host.apply_recipe_indicators(id, &recipe);
        }
        settle(host, id);
    }
}

/// Read one tab; a change the book has not seen becomes its copy, and only its.
/// A tab whose change signal has not moved since it was last read is not read.
/// Answers whether the copy changed.
pub fn observe_tab<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64) -> bool {
    let store = host.recipe_store();
    if store.sync().is_pending(id) || store.book().tab_state(id).is_none() {
        return false;
    }
    if !host.tab_settled(id) {
        return false;
    }
    if !host.recipe_store().sync().is_settled(id) {
        settle(host, id);
        return false;
    }
    let signal = host.recipe_signal(id);
    if host.recipe_store().sync().is_quiet(id, signal) {
        return false;
    }
    let Some(live) = host.live_recipe(id) else {
        return false;
    };
    let (book, sync) = host.recipe_store_mut().parts_mut();
    sync.note_signal(id, signal);
    let Some(copy) = book.tab_copy(id) else {
        return false;
    };
    // The tab was just found in the book, so observe cannot refuse.
    sync.observe(id, live, copy)
        .is_some_and(|edit| book.observe(id, &edit).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspaces::path_beside;
    use quantick_workspace::workspace_sync::AUTOSAVE_QUIET;

    /// One tab, always settled, whose panes show whatever `shows` says.
    struct OneTab {
        store: WorkspaceBookStore,
        shows: WorkspaceRecipe,
        applied: usize,
        /// How many recipes were built off the tab.
        builds: std::cell::Cell<usize>,
        layout: Option<String>,
    }

    impl TabRecipeHost for OneTab {
        fn recipe_store(&self) -> &WorkspaceBookStore {
            &self.store
        }
        fn recipe_store_mut(&mut self) -> &mut WorkspaceBookStore {
            &mut self.store
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
    }

    fn one_tab(test: &str) -> OneTab {
        let folder = crate::scratch::thread_dir(test);
        OneTab {
            store: WorkspaceBookStore::open_at(path_beside(&folder.join("layouts.toml"))),
            shows: WorkspaceRecipe::default(),
            applied: 0,
            builds: std::cell::Cell::new(0),
            layout: None,
        }
    }

    /// The hot path: once a tab has settled, a frame where nothing moved builds
    /// no recipe. Before the signal it built one per observed tab per frame.
    #[test]
    fn an_idle_frame_builds_no_recipe() {
        let mut host = one_tab("tab-recipes-idle");
        maintain(&mut host, Instant::now());
        maintain(&mut host, Instant::now());
        host.builds.set(0);
        for _ in 0..5 {
            maintain(&mut host, Instant::now());
        }
        assert_eq!(host.builds.get(), 0, "five idle frames");
        host.shows.split_fraction = Some(0.4);
        maintain(&mut host, Instant::now());
        assert_eq!(host.builds.get(), 1, "an edit is read once");
        assert!(host.store.book().differs(7));
    }

    /// The desk named a workspace deleted since, and kept no copy because the
    /// tab was clean: the tab keeps what it shows, marked, on `Default`.
    #[test]
    fn a_tab_whose_workspace_is_gone_keeps_what_it_shows_as_default_marked() {
        let mut host = one_tab("tab-recipes-lost");
        host.store.book_mut().set_autosave(false);
        let saved = WorkspaceRecipe {
            split_fraction: Some(0.5),
            ..WorkspaceRecipe::default()
        };
        host.store.book_mut().open_tab(1);
        host.store.book_mut().observe(1, &saved).unwrap();
        host.store.book_mut().save_to(1).unwrap();
        host.store.book_mut().close_tab(1);
        assert!(!host.store.book().recipe("Default").unwrap().is_factory());
        host.shows.split_fraction = Some(0.2);
        host.store
            .sync_mut()
            .arrive(7, Some("Deleted since".to_owned()), None);
        maintain(&mut host, Instant::now());
        assert_eq!(host.applied, 0, "Default's recipe was not laid over it");
        assert_eq!(
            host.store.tab_row(7),
            ("Default".to_owned(), true),
            "Default •"
        );
        assert_eq!(host.store.book().tab_copy(7), Some(&host.shows));
    }

    #[test]
    fn a_first_run_seeds_the_tab_and_applies_nothing_to_it() {
        let mut host = one_tab("tab-recipes-seed");
        maintain(&mut host, Instant::now());
        assert_eq!(host.applied, 0, "a seeded tab keeps every pixel it had");
        let state = host.store.book().tab_state(7).expect("the tab was adopted");
        assert_eq!(state.workspace, "WINV26", "the bare symbol, no `from `");
        assert!(!state.modified);
    }

    #[test]
    fn an_edit_on_a_seeded_tab_marks_it_modified() {
        let mut host = one_tab("tab-recipes-edit");
        host.store.book_mut().set_autosave(false);
        maintain(&mut host, Instant::now());
        host.shows.split_fraction = Some(0.25);
        maintain(&mut host, Instant::now());
        let state = host.store.book().tab_state(7).expect("the tab was adopted");
        assert!(state.modified, "the edit became this tab's copy");
    }

    #[test]
    fn a_first_run_names_the_workspace_after_the_tab_layout() {
        let mut host = one_tab("tab-recipes-seed-layout");
        host.layout = Some("Fluxo".to_owned());
        maintain(&mut host, Instant::now());
        assert_eq!(host.store.tab_row(7), ("Fluxo".to_owned(), false));
    }

    /// Autosave on: an edit never reads as held, and is written through once
    /// it has stood quiet for a second, not before.
    #[test]
    fn autosave_writes_an_edit_through_after_a_quiet_second_and_never_marks() {
        let mut host = one_tab("tab-recipes-autosave");
        let start = Instant::now();
        maintain(&mut host, start);
        maintain(&mut host, start);
        host.shows.split_fraction = Some(0.3);
        assert!(
            maintain(&mut host, start).is_empty(),
            "not inside the second"
        );
        assert!(!host.store.tab_row(7).1, "never marked");
        let due = maintain(&mut host, start + AUTOSAVE_QUIET);
        assert_eq!(due, vec![7]);
        assert!(
            maintain(&mut host, start + AUTOSAVE_QUIET * 2).is_empty(),
            "taken once"
        );
    }

    /// Autosave off: the edit is held, marked, and nothing is due.
    #[test]
    fn autosave_off_holds_the_edit_marked_and_writes_nothing() {
        let mut host = one_tab("tab-recipes-autosave-off");
        host.store.book_mut().set_autosave(false);
        let start = Instant::now();
        maintain(&mut host, start);
        maintain(&mut host, start);
        host.shows.split_fraction = Some(0.3);
        maintain(&mut host, start);
        assert!(maintain(&mut host, start + AUTOSAVE_QUIET * 3).is_empty());
        assert!(host.store.tab_row(7).1, "the cell's mark");
        let edits = host.store.book().unsaved_edits(7).expect("closing asks");
        assert_eq!(edits.changes, vec!["panes".to_owned()]);
    }
}
