//! The bar rules a settled tab can capture on an explicit workspace save.
use super::{DEFAULT_WORKSPACE, TabWorkspace, WorkspaceBook, WorkspaceRecipe};

/// `copy` as the tab's workspace `base` reads it: [`WorkspaceRecipe::masked`],
/// except that a bar rule `base` names none for is an edit once it differs
/// from the one the tab settled on. Without a settled reading, as on the
/// factory `Default`, the tab's own rules stay no edit.
pub(super) fn read(
    state: &TabWorkspace,
    base: &WorkspaceRecipe,
    copy: &WorkspaceRecipe,
) -> WorkspaceRecipe {
    let mut read = base.masked(copy);
    let Some(settled) = &state.settled else {
        return read;
    };
    if base.flow_bars.is_none() && copy.flow_bars != settled.flow_bars {
        read.flow_bars.clone_from(&copy.flow_bars);
    }
    let bars = |recipe: &WorkspaceRecipe, slot: usize| {
        (recipe.context_panes.get(slot)).and_then(|pane| pane.bars.clone())
    };
    for (slot, pane) in read.context_panes.iter_mut().enumerate() {
        if bars(base, slot).is_none() && bars(copy, slot) != bars(settled, slot) {
            pane.bars = bars(copy, slot);
        }
    }
    read
}

impl WorkspaceBook {
    /// After a failed save the requested intervals are held until saved or discarded.
    pub(super) fn held_capture(&self, tab: u64) -> bool {
        self.unsynced.contains_key(&tab) && self.captures_inherited_bars(tab)
    }

    /// An explicit save can record a named workspace's inherited intervals
    /// even when the tab has no edits for autosave or the close prompt.
    #[must_use]
    pub fn captures_inherited_bars(&self, tab: u64) -> bool {
        self.tabs.get(&tab).is_some_and(|state| {
            state.workspace != DEFAULT_WORKSPACE
                && self
                    .baseline(state)
                    .is_some_and(|base| base.inherits_bars_from(&state.copy))
        })
    }

    /// Once the panes stand, remember inherited intervals for an explicit save.
    /// A clean tab on the factory `Default` takes its market as its baseline.
    /// The first settle on a named workspace is the reading its unnamed bar
    /// rules are told from, kept until the tab leaves the workspace: a later
    /// settle shows the tab's own edits, never what it opened with.
    pub fn settle_tab(&mut self, tab: u64, live: &WorkspaceRecipe) {
        let Some(state) = self.tabs.get(&tab) else {
            return;
        };
        let factory = self
            .recipes
            .get(&state.workspace)
            .is_some_and(WorkspaceRecipe::is_factory);
        let opens_on_market = factory && !self.is_modified(tab, state);
        let Some(state) = self.tabs.get_mut(&tab) else {
            return;
        };
        if opens_on_market {
            state.copy = live.clone();
            state.opened = Some(live.without_bars());
            state.settled = None;
        } else {
            state.copy.take_inherited_bars(live);
            if !factory && state.settled.is_none() {
                state.settled = Some(live.clone());
            }
        }
    }
}
