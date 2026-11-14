//! What a frame owes the workspace book: adopting the open tabs, carrying each
//! recipe on its way and noticing edits. The window only reads a tab into a
//! recipe and puts a recipe on a tab, through [`TabRecipeHost`]; every decision
//! is [`crate::workspace_sync`]'s or the book's. Nothing here touches a file:
//! a write the upkeep calls for comes back as [`UpkeepSave`].

use crate::market_settings::DrawingKey;
use crate::market_settings_store::MarketSettingsStore;
use crate::workspace_book::{DEFAULT_WORKSPACE, SeedTab, WorkspaceBook};
use crate::workspace_recipe::WorkspaceRecipe;
use crate::workspace_sync::{TabAdoption, TabRecipeSync};
use std::time::Instant;

/// What the upkeep leaves its caller to write.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpkeepSave {
    Nothing,
    /// A first run seeded workspaces from the open tabs: the book is to be saved.
    SeededBook,
}

/// What a frame's upkeep leaves its caller: the book to save, and the tabs
/// autosave writes through now.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upkeep {
    pub save: UpkeepSave,
    /// Edits quiet for a second, never while autosave is off, one tab per
    /// workspace ([`WorkspaceBook::autosave_winners`]).
    pub write_through: Vec<u64>,
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
    /// The canvas and the bar rules: the context intervals and the flow bar,
    /// where the recipe names one. Drawings, anchored studies and the paper
    /// position are not touched: they are the market's.
    fn apply_recipe_canvas(&mut self, tab: u64, recipe: &WorkspaceRecipe);
    /// Layers and the footprint style, on every pane the tab has now.
    fn apply_recipe_switches(&mut self, tab: u64, recipe: &WorkspaceRecipe);
    /// Each pane by role takes the recipe's set, onto a layout entry only it holds.
    fn apply_recipe_indicators(&mut self, tab: u64, recipe: &WorkspaceRecipe);
    /// The drawing sets (anchored studies among them) the tab's panes hold.
    fn tab_drawing_keys(&self, _tab: u64) -> Vec<DrawingKey> {
        Vec::new()
    }
    /// The market store those sets live in; `None` for a host without drawings.
    fn recipe_market(&mut self) -> Option<&mut MarketSettingsStore> {
        None
    }
}

/// What the frame owes the book, once its panes are built and seeded. A
/// write-through that failed is armed again, so it is retried a quiet second
/// later, never per frame.
pub fn maintain<H: TabRecipeHost + ?Sized>(host: &mut H, now: Instant) -> Upkeep {
    let save = adopt_tabs(host);
    note_drawings(host);
    apply_pending_recipes(host);
    let active = host.active_recipe_tab();
    let tabs = host.recipe_tabs();
    let other = host.recipe_book_mut().1.next_watch(&tabs, active);
    for id in std::iter::once(active).chain(other) {
        watch_tab(host, id, now);
    }
    let (book, sync) = host.recipe_book_mut();
    book.note_active(active);
    let mut due = Vec::new();
    if book.autosave() {
        for tab in book.failed_writes() {
            sync.arm(tab, now);
        }
        due = book.autosave_winners(&sync.take_due(now), active);
    }
    Upkeep {
        save,
        write_through: due,
    }
}

/// Read one tab as [`observe_tab`] does and, when its copy changed, start
/// the quiet second autosave waits: the one path every reading takes, the
/// frame's and a command's, so no edit it picks up is left unwritten.
pub fn watch_tab<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64, now: Instant) -> bool {
    let changed = observe_tab(host, id);
    if changed {
        host.recipe_book_mut().1.note_edit(id, now);
    }
    changed
}

/// The market store waits for Save while autosave is off, and the book learns
/// what each tab's drawings hold back, so its mark and close prompt count them.
///
/// Per frame, so autosave on reads no pane (nothing is held, nothing marks)
/// and a tab whose sets and store revision stand as they were last counted
/// compares no drawing.
pub fn note_drawings<H: TabRecipeHost + ?Sized>(host: &mut H) {
    let held = !host.recipe_book().0.autosave();
    let Some(market) = host.recipe_market() else {
        return;
    };
    market.set_held(held);
    if !held {
        return;
    }
    let revision = market.revision();
    let book = host.recipe_book().0;
    let stale: Vec<(u64, Vec<DrawingKey>)> = (host.recipe_tabs().into_iter())
        .map(|tab| (tab, host.tab_drawing_keys(tab)))
        .filter(|(tab, keys)| !book.drawings_current(*tab, keys, revision))
        .collect();
    if stale.is_empty() {
        return;
    }
    let Some(market) = host.recipe_market() else {
        return;
    };
    let counts: Vec<usize> = stale
        .iter()
        .map(|(_, keys)| market.held_changes(keys))
        .collect();
    let book = host.recipe_book_mut().0;
    for ((tab, keys), held) in stale.into_iter().zip(counts) {
        book.note_drawings_at(tab, keys, held, revision);
    }
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
                        layout: host.recipe_tab_layout(id),
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
    if let Some(state) = book.tab_state(id) {
        sync.settled_on(id, state.workspace);
    }
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
        let (book, sync) = host.recipe_book();
        let on = book.tab_state(id).map(|state| state.workspace);
        let arrived = sync.stands_on(id, on)
            && host.tab_settled(id)
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
        if !arrived {
            share_shown_sets(host, id);
        }
    }
}

/// After `landed` took its recipe, which may repaint the other tabs on its
/// workspace: each that stands is read again, as no edit. Where any tab
/// there holds an indicator edit (another kind of edit stays its tab's, so a
/// heal beside it marks no one), the sets shown are that edit, the
/// workspace's, and
/// every one of them holds it ([`WorkspaceBook::hold_shown`]): each reads
/// held, a save from any writes it, and a discard on one leaves it with the
/// rest. With no edit held a landing's heal stays out of every copy.
fn share_shown_sets<H: TabRecipeHost + ?Sized>(host: &mut H, landed: u64) {
    let book = host.recipe_book().0;
    let Some(state) = book.tab_state(landed) else {
        return;
    };
    let tabs = book.tabs_on(state.workspace);
    let held = tabs.iter().any(|tab| book.holds_set_edit(*tab));
    for tab in tabs {
        let sync = host.recipe_book().1;
        if sync.is_pending(tab) || !sync.is_settled(tab) {
            continue;
        }
        let signal = host.recipe_signal(tab);
        let Some(live) = host.live_recipe(tab) else {
            continue;
        };
        let (book, sync) = host.recipe_book_mut();
        if held {
            book.hold_shown(tab, &live);
        }
        sync.reread(tab, live, signal);
    }
}

/// Read one tab; a change the book has not seen becomes its copy, and only its.
/// A tab whose change signal has not moved since it was last read is not read.
/// Answers whether the copy changed.
pub fn observe_tab<H: TabRecipeHost + ?Sized>(host: &mut H, id: u64) -> bool {
    let (book, sync) = host.recipe_book();
    if sync.is_pending(id) || book.tab_state(id).is_none() {
        return false;
    }
    if !host.tab_settled(id) {
        return false;
    }
    if !host.recipe_book().1.is_settled(id) {
        settle(host, id);
        return false;
    }
    let signal = host.recipe_signal(id);
    if host.recipe_book().1.is_quiet(id, signal) {
        return false;
    }
    let Some(live) = host.live_recipe(id) else {
        return false;
    };
    let (book, sync) = host.recipe_book_mut();
    sync.note_signal(id, signal);
    let Some(copy) = book.tab_copy(id) else {
        return false;
    };
    // The tab was just found in the book, so observe cannot refuse.
    sync.observe(id, live, copy)
        .is_some_and(|edit| book.observe(id, &edit).unwrap_or(false))
}

/// Each open tab's workspace, as `(tab, workspace, settled)` in tab id
/// order, for [`crate::session::LayoutSession::hold_workspaces`]: the book's
/// word, or for a tab the book has not adopted yet the desk's, followed
/// through a rename and `Default` for one deleted since, as the book will
/// restore it. So tabs a desk brings back share their workspace's copies
/// from their first frame. `settled`: the tab's panes stand on what its
/// workspace showed it, no recipe on its way.
pub fn tab_holdings<'a>(
    (book, sync): (&'a WorkspaceBook, &'a TabRecipeSync),
) -> impl Iterator<Item = (u64, &'a str, bool)> + Clone + 'a {
    let settled = move |tab| sync.is_settled(tab) && !sync.is_pending(tab);
    let held = (book.tab_workspaces()).map(move |(tab, name)| (tab, name, settled(tab)));
    let arriving = (sync.arriving())
        .filter(move |(tab, _)| book.tab_state(*tab).is_none())
        .map(move |(tab, name)| (tab, book.resolve(name).unwrap_or(DEFAULT_WORKSPACE), false));
    held.chain(arriving)
}

/// Before a save: the tab's copy takes the indicator sets its panes showed
/// when last read, so a save writes what every tab on the workspace shows, a
/// set healed on landing included, and nothing else of the copy moves. A
/// heal alone never reaches a recipe this way: autosave and a flush write a
/// tab only once its copy differs.
pub fn take_shown_sets(book: &mut WorkspaceBook, sync: &TabRecipeSync, tab: u64) {
    if let Some(shown) = sync.shown(tab).filter(|_| !sync.is_pending(tab)) {
        book.take_sets(tab, shown);
    }
}

#[cfg(test)]
#[path = "book_upkeep_tests.rs"]
mod book_upkeep_tests;
