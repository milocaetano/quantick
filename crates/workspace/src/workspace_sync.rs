//! How the window keeps its tabs and the workspace book in step.
//!
//! The book holds what a tab should show; the window holds what it does show.
//! These are the decisions between the two: how a tab joins the book, which
//! recipe is still on its way to a tab whose panes are not built yet, and whether
//! what a tab shows now is an edit. Headless: the window reads its panes and
//! carries out the answers.
use crate::workspace_recipe::WorkspaceRecipe;
use std::collections::BTreeMap;

/// Keep in `edit` a layer the copy names, or one that moved since the recipe
/// settled. The rest stay open.
fn keep_named_layers(
    edit: &mut BTreeMap<String, bool>,
    copy: Option<&BTreeMap<String, bool>>,
    baseline: Option<&BTreeMap<String, bool>>,
) {
    edit.retain(|layer, on| {
        copy.is_some_and(|copy| copy.contains_key(layer))
            || baseline.and_then(|baseline| baseline.get(layer)) != Some(&*on)
    });
}

/// How a tab the window holds joins the book.
#[derive(Debug, Clone, PartialEq)]
pub enum TabAdoption {
    /// The book already holds it.
    Held,
    /// A desk brought it back on this workspace, with any copy it kept.
    Restore {
        workspace: String,
        copy: Option<WorkspaceRecipe>,
    },
    /// A first run, or a desk from before workspaces: its recipe is what it shows.
    Seed,
    /// A new tab: it opens on `Default`.
    OnDefault,
}

/// A recipe on its way to a tab.
#[derive(Debug, Clone, PartialEq)]
struct PendingRecipe {
    recipe: WorkspaceRecipe,
    canvas_done: bool,
}

/// What the window last read off a tab.
#[derive(Debug, Clone, PartialEq)]
struct SeenTab {
    /// As the tab stood when a recipe last settled on it.
    baseline: WorkspaceRecipe,
    last: WorkspaceRecipe,
}

/// The window's side of the book, kept apart from the recipes: none of it is
/// saved.
#[derive(Debug, Default)]
pub struct TabRecipeSync {
    arrivals: BTreeMap<u64, (Option<String>, Option<WorkspaceRecipe>)>,
    pending: BTreeMap<u64, PendingRecipe>,
    seen: BTreeMap<u64, SeenTab>,
    first_pass_done: bool,
    watch: usize,
}

impl TabRecipeSync {
    /// A desk restored this tab with these saved fields.
    pub fn arrive(&mut self, tab: u64, workspace: Option<String>, copy: Option<WorkspaceRecipe>) {
        self.arrivals.insert(tab, (workspace, copy));
    }

    /// Whether a desk's word on this tab is still to be read.
    #[must_use]
    pub fn has_arrival(&self, tab: u64) -> bool {
        self.arrivals.contains_key(&tab)
    }

    /// How this tab joins the book. `held`: the book has it already. `unused`:
    /// the book has never held a workspace but `Default`, so tabs open before
    /// the first pass ended are the first run's; with a used book they are new.
    pub fn adoption(&mut self, tab: u64, held: bool, unused: bool) -> TabAdoption {
        if let Some((workspace, copy)) = self.arrivals.remove(&tab) {
            return match workspace {
                Some(workspace) => TabAdoption::Restore { workspace, copy },
                None if held => TabAdoption::Held,
                None => TabAdoption::Seed,
            };
        }
        if held {
            TabAdoption::Held
        } else if !self.first_pass_done && unused {
            TabAdoption::Seed
        } else {
            TabAdoption::OnDefault
        }
    }

    /// Every tab open at launch has joined; a tab from here on is a new one.
    pub fn finish_first_pass(&mut self) {
        self.first_pass_done = true;
    }

    /// Put `recipe` on its way to `tab`, replacing one still on its way.
    pub fn queue(&mut self, tab: u64, recipe: WorkspaceRecipe) {
        self.pending.insert(
            tab,
            PendingRecipe {
                recipe,
                canvas_done: false,
            },
        );
    }

    #[must_use]
    pub fn pending_tabs(&self) -> Vec<u64> {
        self.pending.keys().copied().collect()
    }

    #[must_use]
    pub fn is_pending(&self, tab: u64) -> bool {
        self.pending.contains_key(&tab)
    }

    #[must_use]
    pub fn pending_recipe(&self, tab: u64) -> Option<&WorkspaceRecipe> {
        self.pending.get(&tab).map(|pending| &pending.recipe)
    }

    /// The recipe whose canvas is due, once: the panes it asks for arrive a
    /// frame later, and the canvas must not be laid twice while they do.
    pub fn take_canvas(&mut self, tab: u64) -> Option<WorkspaceRecipe> {
        let pending = self.pending.get_mut(&tab)?;
        if pending.canvas_done {
            return None;
        }
        pending.canvas_done = true;
        Some(pending.recipe.clone())
    }

    /// The tab's panes stand: the recipe has arrived.
    pub fn finish(&mut self, tab: u64) -> Option<WorkspaceRecipe> {
        self.pending.remove(&tab).map(|pending| pending.recipe)
    }

    /// What the tab shows now that a recipe settled on it. A pane may clamp what
    /// it was told, so this, not the recipe, is what a later edit is told from.
    pub fn settle(&mut self, tab: u64, live: WorkspaceRecipe) {
        self.seen.insert(
            tab,
            SeenTab {
                baseline: live.clone(),
                last: live,
            },
        );
    }

    /// What the tab shows now, against its held `copy`: the copy to record if
    /// that is an edit. A layer or a footprint style the copy leaves open stays
    /// open while it reads as it did when the recipe settled.
    pub fn observe(
        &mut self,
        tab: u64,
        live: WorkspaceRecipe,
        copy: &WorkspaceRecipe,
    ) -> Option<WorkspaceRecipe> {
        let Some(seen) = self.seen.get_mut(&tab) else {
            self.settle(tab, live);
            return None;
        };
        if seen.last == live {
            return None;
        }
        seen.last = live.clone();
        let mut edit = live;
        let baseline = &seen.baseline;
        keep_named_layers(&mut edit.layers, Some(&copy.layers), Some(&baseline.layers));
        for (index, pane) in edit.context_panes.iter_mut().enumerate() {
            keep_named_layers(
                &mut pane.layers,
                copy.context_panes.get(index).map(|pane| &pane.layers),
                baseline.context_panes.get(index).map(|pane| &pane.layers),
            );
        }
        if copy.footprint_style.is_none() && edit.footprint_style == baseline.footprint_style {
            edit.footprint_style = None;
        }
        (edit != *copy).then_some(edit)
    }

    /// The tab closed.
    pub fn forget(&mut self, tab: u64) {
        self.arrivals.remove(&tab);
        self.pending.remove(&tab);
        self.seen.remove(&tab);
    }

    /// A store reloaded under the window: recipes in flight were the old book's.
    pub fn restart(&mut self) {
        self.pending.clear();
        self.seen.clear();
    }

    /// Which background tab is read this frame, one per frame in turn: the
    /// active tab is read every frame, and only a control call edits another.
    pub fn next_watch(&mut self, tabs: &[u64], active: u64) -> Option<u64> {
        let others = tabs.iter().filter(|tab| **tab != active).count();
        if others == 0 {
            return None;
        }
        self.watch = (self.watch + 1) % others;
        tabs.iter()
            .copied()
            .filter(|tab| *tab != active)
            .nth(self.watch)
    }
}

#[cfg(test)]
#[path = "workspace_sync_tests.rs"]
mod workspace_sync_tests;
