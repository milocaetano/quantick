//! What a frame owes the workspace book: adopting the open tabs, carrying each
//! recipe on its way and noticing edits. The window only reads a tab into a
//! recipe and puts a recipe on a tab, through [`TabRecipeHost`]; every decision
//! is `quantick_workspace::workspace_sync`'s or the book's.

use crate::workspaces::{SeedTab, TabAdoption, WorkspaceBookStore, WorkspaceRecipe};

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
    /// A window-wide footprint edit must not move a tab on its own recipe, so a
    /// seeded tab holds the setup it shows. No pixel changes.
    fn pin_tab_footprint(&mut self, tab: u64);
    /// The canvas and the context intervals. Drawings, anchored studies, the flow
    /// bar size and the paper position are not touched: they are the market's.
    fn apply_recipe_canvas(&mut self, tab: u64, recipe: &WorkspaceRecipe);
    /// Layers and the footprint style, on every pane the tab has now.
    fn apply_recipe_switches(&mut self, tab: u64, recipe: &WorkspaceRecipe);
    /// Each pane by role takes the recipe's set, onto a layout entry only it holds.
    fn apply_recipe_indicators(&mut self, tab: u64, recipe: &WorkspaceRecipe);
}

/// What the frame owes the book, once its panes are built and seeded.
pub fn maintain<H: TabRecipeHost + ?Sized>(host: &mut H) {
    adopt_tabs(host);
    apply_pending_recipes(host);
    let active = host.active_recipe_tab();
    observe_tab(host, active);
    let tabs = host.recipe_tabs();
    let other = host.recipe_store_mut().sync_mut().next_watch(&tabs, active);
    if let Some(other) = other {
        observe_tab(host, other);
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
        match store.sync_mut().adoption(id, held, unused) {
            TabAdoption::Held => {}
            TabAdoption::Restore { workspace, copy } => {
                let book = store.book_mut();
                book.restore_tab(id, &workspace, copy.map(|copy| *copy));
                if let Some(recipe) = book.tab_copy(id).filter(|copy| !copy.is_factory()) {
                    let recipe = recipe.clone();
                    store.sync_mut().queue(id, recipe);
                }
            }
            TabAdoption::OnDefault => {
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

/// Queue `shown` for a tab and carry it as far as the panes allow.
pub fn show_recipe<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64, shown: WorkspaceRecipe) {
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
        if let Some(live) = host.live_recipe(id) {
            host.recipe_store_mut().sync_mut().settle(id, live);
        }
    }
}

/// Read one tab; a change the book has not seen becomes its copy, and only its.
pub fn observe_tab<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64) {
    let store = host.recipe_store();
    if store.sync().is_pending(id) || store.book().tab_state(id).is_none() {
        return;
    }
    if !host.tab_settled(id) {
        return;
    }
    let Some(live) = host.live_recipe(id) else {
        return;
    };
    let (book, sync) = host.recipe_store_mut().parts_mut();
    let Some(copy) = book.tab_copy(id) else {
        return;
    };
    if let Some(edit) = sync.observe(id, live, copy) {
        // The tab was just found in the book, so this cannot refuse.
        let _ = book.observe(id, &edit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspaces::path_beside;

    /// One tab, always settled, whose panes show whatever `shows` says.
    struct OneTab {
        store: WorkspaceBookStore,
        shows: WorkspaceRecipe,
        applied: usize,
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
            Some(self.shows.clone())
        }
        fn recipe_tab_symbol(&self, _tab: u64) -> Option<String> {
            Some("WINV26".to_owned())
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
        }
    }

    #[test]
    fn a_first_run_seeds_the_tab_and_applies_nothing_to_it() {
        let mut host = one_tab("tab-recipes-seed");
        maintain(&mut host);
        assert_eq!(host.applied, 0, "a seeded tab keeps every pixel it had");
        let state = host.store.book().tab_state(7).expect("the tab was adopted");
        assert_eq!(state.workspace, "from WINV26");
        assert!(!state.modified);
    }

    #[test]
    fn an_edit_on_a_seeded_tab_marks_it_modified() {
        let mut host = one_tab("tab-recipes-edit");
        maintain(&mut host);
        host.shows.split_fraction = Some(0.25);
        maintain(&mut host);
        let state = host.store.book().tab_state(7).expect("the tab was adopted");
        assert!(state.modified, "the edit became this tab's copy");
    }
}
