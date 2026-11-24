//! Revert: put a tab back on its workspace as saved, and take that back.
use super::{TabWorkspace, WorkspaceBook, WorkspaceRecipe, WorkspaceRefusal};

/// The edits a revert dropped, held for [`WorkspaceBook::undo_revert`] while
/// the tab stays on `workspace` and `workspace` still reads `base`: a save
/// or a switch ends them.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Reverted {
    workspace: String,
    base: WorkspaceRecipe,
    copy: WorkspaceRecipe,
}

/// What a revert, or its undo, did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevertOutcome {
    pub workspace: String,
    /// Whether the tab's arrangement moved; `false` for a tab with nothing
    /// to revert or undo, which changes nothing.
    pub changed: bool,
}

impl WorkspaceBook {
    /// Drop the tab's unsaved arrangement edits: its copy goes back to its
    /// workspace, each bar rule the workspace names none for back to the one
    /// the tab opened with. Drawings are not touched. The dropped copy is
    /// held for [`Self::undo_revert`]; each revert replaces the last.
    ///
    /// # Errors
    /// No such tab.
    pub fn revert(&mut self, tab: u64) -> Result<RevertOutcome, WorkspaceRefusal> {
        let TabWorkspace {
            workspace, copy, ..
        } = self.state(tab)?.clone();
        let base = self.recipes.get(&workspace).cloned();
        let changed = self.differs(tab) && self.discard_edits(tab);
        if let Some(base) = base.filter(|_| changed) {
            let held = Reverted {
                workspace: workspace.clone(),
                base,
                copy,
            };
            self.reverted.insert(tab, held);
        }
        Ok(RevertOutcome { workspace, changed })
    }

    /// Whether [`Self::undo_revert`] would bring edits back: a revert holds
    /// them and the tab has none of its own since.
    #[must_use]
    pub fn can_undo_revert(&self, tab: u64) -> bool {
        self.held_revert(tab).is_some() && !self.differs(tab)
    }

    /// Bring back the edits the tab's last revert dropped. A tab holding
    /// edits of its own since answers unchanged: a revert drops those first.
    ///
    /// # Errors
    /// No such tab.
    pub fn undo_revert(&mut self, tab: u64) -> Result<RevertOutcome, WorkspaceRefusal> {
        let workspace = self.state(tab)?.workspace.clone();
        if !self.can_undo_revert(tab) {
            return Ok(RevertOutcome {
                workspace,
                changed: false,
            });
        }
        let held = self.reverted.remove(&tab).expect("held, checked above");
        let changed = self.observe(tab, &held.copy)?;
        Ok(RevertOutcome { workspace, changed })
    }

    fn held_revert(&self, tab: u64) -> Option<&Reverted> {
        let held = self.reverted.get(&tab)?;
        let on = self.tabs.get(&tab)?.workspace == held.workspace;
        (on && self.recipes.get(&held.workspace) == Some(&held.base)).then_some(held)
    }
}
