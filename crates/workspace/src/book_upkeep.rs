//! What a frame owes the workspace book: adopting the open tabs, carrying each
//! recipe on its way and noticing edits. The window only reads a tab into a
//! recipe and puts a recipe on a tab, through [`TabRecipeHost`]; every decision
//! is [`crate::workspace_sync`]'s or the book's. Nothing here touches a file:
//! a write the upkeep calls for comes back as [`UpkeepSave`].

use crate::workspace_book::{SeedTab, WorkspaceBook};
use crate::workspace_recipe::WorkspaceRecipe;
use crate::workspace_sync::{TabAdoption, TabRecipeSync};

/// What the upkeep leaves its caller to write.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpkeepSave {
    Nothing,
    /// A first run seeded workspaces from the open tabs: the book is to be saved.
    SeededBook,
}

/// What the window lends the book's upkeep.
pub trait TabRecipeHost {
    /// The book and the window's side of it.
    fn recipe_book(&self) -> (&WorkspaceBook, &TabRecipeSync);
    fn recipe_book_mut(&mut self) -> (&mut WorkspaceBook, &mut TabRecipeSync);
    /// The open tabs, in strip order.
    fn recipe_tabs(&self) -> Vec<u64>;
    fn active_recipe_tab(&self) -> u64;
    /// The tab's panes stand: seeded, and the context charts it asks for built.
    fn tab_settled(&self, tab: u64) -> bool;
    /// What the tab shows now, as a recipe.
    fn live_recipe(&self, tab: u64) -> Option<WorkspaceRecipe>;
    fn recipe_tab_symbol(&self, tab: u64) -> Option<String>;
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

/// What the frame owes the book, once its panes are built and seeded.
pub fn maintain<H: TabRecipeHost + ?Sized>(host: &mut H) -> UpkeepSave {
    let save = adopt_tabs(host);
    apply_pending_recipes(host);
    let active = host.active_recipe_tab();
    observe_tab(host, active);
    let tabs = host.recipe_tabs();
    let other = host.recipe_book_mut().1.next_watch(&tabs, active);
    if let Some(other) = other {
        observe_tab(host, other);
    }
    save
}

fn adopt_tabs<H: TabRecipeHost + ?Sized>(host: &mut H) -> UpkeepSave {
    let mut seeds = Vec::new();
    let mut all_settled = true;
    for id in host.recipe_tabs() {
        let (book, sync) = host.recipe_book();
        let held = book.tab_state(id).is_some();
        if held && !sync.has_arrival(id) {
            continue;
        }
        if !host.tab_settled(id) {
            all_settled = false;
            continue;
        }
        let (book, sync) = host.recipe_book_mut();
        let adoption = sync.adoption(id, held, book.is_unused());
        match adoption {
            TabAdoption::Held => {}
            TabAdoption::Restore { workspace, copy } => restore(host, id, &workspace, copy),
            TabAdoption::OnDefault => {
                let recipe = book.open_tab(id).clone();
                if !recipe.is_factory() {
                    sync.queue(id, recipe);
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
    let (book, sync) = host.recipe_book_mut();
    if all_settled {
        sync.finish_first_pass();
    }
    if seeds.is_empty() {
        return UpkeepSave::Nothing;
    }
    for seed in &seeds {
        sync.settle(seed.tab, seed.live.clone());
    }
    book.seed_first_run(seeds);
    UpkeepSave::SeededBook
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
    let lost = copy.is_none() && host.recipe_book().0.resolve(workspace).is_none();
    let copy = match copy {
        Some(copy) => Some(*copy),
        None if lost => host.live_recipe(id),
        None => None,
    };
    let (book, sync) = host.recipe_book_mut();
    book.restore_tab(id, workspace, copy);
    if lost {
        settle(host, id);
    } else if let Some(recipe) = book.tab_copy(id).filter(|copy| !copy.is_factory()) {
        sync.queue(id, recipe.clone());
    }
}

/// The tab's panes stand: what it shows is what later edits are told from.
fn settle<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64) {
    host.pin_tab_footprint(id);
    let signal = host.recipe_signal(id);
    let Some(live) = host.live_recipe(id) else {
        return;
    };
    let (book, sync) = host.recipe_book_mut();
    book.settle_tab(id, &live);
    sync.settle(id, live);
    sync.note_signal(id, signal);
}

/// Queue `recipe` for a tab and carry it as far as the panes allow. The
/// factory `Default` is laid as a new tab on its feed: never a bare canvas.
pub fn show_recipe<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64, recipe: &WorkspaceRecipe) {
    let shown = if recipe.is_factory() {
        host.new_tab_recipe(id)
    } else {
        recipe.clone()
    };
    host.recipe_book_mut().1.queue(id, shown);
    apply_pending_recipes(host);
}

/// Carry each recipe on its way as far as the tab's panes allow.
pub fn apply_pending_recipes<H: TabRecipeHost + ?Sized>(host: &mut H) {
    for id in host.recipe_book().1.pending_tabs() {
        let arrived = host.tab_settled(id)
            && host.live_recipe(id).as_ref() == host.recipe_book().1.pending_recipe(id);
        if !arrived && let Some(recipe) = host.recipe_book_mut().1.take_canvas(id) {
            host.apply_recipe_canvas(id, &recipe);
        }
        if !host.tab_settled(id) {
            continue;
        }
        let Some(recipe) = host.recipe_book_mut().1.finish(id) else {
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
pub fn observe_tab<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64) {
    let (book, sync) = host.recipe_book();
    if sync.is_pending(id) || book.tab_state(id).is_none() {
        return;
    }
    if !host.tab_settled(id) {
        return;
    }
    if !host.recipe_book().1.is_settled(id) {
        settle(host, id);
        return;
    }
    let signal = host.recipe_signal(id);
    if host.recipe_book().1.is_quiet(id, signal) {
        return;
    }
    let Some(live) = host.live_recipe(id) else {
        return;
    };
    let (book, sync) = host.recipe_book_mut();
    sync.note_signal(id, signal);
    let Some(copy) = book.tab_copy(id) else {
        return;
    };
    if let Some(edit) = sync.observe(id, live, copy) {
        // The tab was just found in the book, so this cannot refuse.
        let _ = book.observe(id, &edit);
    }
}

#[cfg(test)]
#[path = "book_upkeep_tests.rs"]
mod book_upkeep_tests;
