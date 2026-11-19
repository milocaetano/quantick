//! The bar rules a settled tab can capture on an explicit workspace save.
use super::{DEFAULT_WORKSPACE, WorkspaceBook, WorkspaceRecipe};

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
        } else {
            state.copy.take_inherited_bars(live);
        }
    }
}
