//! The window arrangement: the document is `quantick_stores::ui_state`; this
//! is where its path is resolved, where the window's own vocabulary meets the
//! file's, and where the divider's clamp is handed to the restore.

use std::path::PathBuf;

use crate::config::AppConfig;
pub use quantick_stores::ui_state::*;

// Each twin converts both ways here, beside the vocabulary it belongs to,
// rather than in the app that happens to read it. It is the pattern
// `DeclaredLayout` → `CanvasLayout` already set (`tab.rs`), and it is what
// keeps adding a variant a one-file edit: the compiler then names every arm
// that has to grow, in the module that owns the names.
pub(crate) trait SavedFocusExt: Sized {
    #[must_use]
    fn from_side(side: crate::pane::PaneSide) -> (Self, usize);
    #[must_use]
    fn to_side(self, slot: usize) -> crate::pane::PaneSide;
}
impl SavedFocusExt for SavedFocus {
    /// The file's two words plus the slot, from the side the tab holds.
    ///
    /// Split into a word and a number rather than a third word per slot:
    /// `focus = "time"` is what every workspace written so far says, and a
    /// file that says it still opens on the top context chart — which is the
    /// only one those files could have meant.
    fn from_side(side: crate::pane::PaneSide) -> (Self, usize) {
        match side {
            crate::pane::PaneSide::Flow => (Self::Flow, 0),
            crate::pane::PaneSide::Time(slot) => (Self::Time, slot),
        }
    }

    /// The side a saved word and slot name. See [`Self::from_side`].
    fn to_side(self, slot: usize) -> crate::pane::PaneSide {
        match self {
            Self::Flow => crate::pane::PaneSide::Flow,
            Self::Time => crate::pane::PaneSide::Time(slot),
        }
    }
}

impl From<crate::toolrail::ToolboxDock> for SavedRailDock {
    fn from(dock: crate::toolrail::ToolboxDock) -> Self {
        match dock {
            crate::toolrail::ToolboxDock::Left => Self::Left,
            crate::toolrail::ToolboxDock::Top => Self::Top,
            crate::toolrail::ToolboxDock::Bottom => Self::Bottom,
        }
    }
}

impl From<SavedRailDock> for crate::toolrail::ToolboxDock {
    fn from(dock: SavedRailDock) -> Self {
        match dock {
            SavedRailDock::Left | SavedRailDock::Right => Self::Left,
            SavedRailDock::Top => Self::Top,
            SavedRailDock::Bottom => Self::Bottom,
        }
    }
}

impl From<crate::dock::DockTab> for SavedDockTab {
    fn from(tab: crate::dock::DockTab) -> Self {
        match tab {
            crate::dock::DockTab::L2 => Self::L2,
            crate::dock::DockTab::Bubbles => Self::Bubbles,
            crate::dock::DockTab::Session => Self::Session,
            crate::dock::DockTab::Trading => Self::Trading,
            crate::dock::DockTab::Trades => Self::Trades,
        }
    }
}

impl From<SavedDockTab> for crate::dock::DockTab {
    fn from(tab: SavedDockTab) -> Self {
        match tab {
            SavedDockTab::L2 => Self::L2,
            SavedDockTab::Bubbles => Self::Bubbles,
            SavedDockTab::Session => Self::Session,
            SavedDockTab::Trading => Self::Trading,
            SavedDockTab::Trades => Self::Trades,
        }
    }
}

/// Application configuration and persistence effects over the shared document.
pub(crate) trait WorkspaceExt: Sized {
    #[must_use]
    fn restore(self, config: &AppConfig) -> Self;
    /// The named arrangement called `name`, if the file has one — for the
    /// tests that assert what actually reached the disk.
    #[cfg(test)]
    #[must_use]
    fn named(&self, name: &str) -> Option<&NamedArrangement>;
}
impl WorkspaceExt for Workspace {
    /// The saved workspace with every tab the live `config` can no longer open
    /// removed, and every out-of-range value brought back inside the domain
    /// its control enforces — the divider's own clamp, not a second copy of
    /// its range.
    fn restore(self, config: &AppConfig) -> Self {
        restore(self, config, crate::pane::clamp_pane_fraction)
    }

    #[cfg(test)]
    fn named(&self, name: &str) -> Option<&NamedArrangement> {
        self.saved.iter().find(|entry| entry.name == name)
    }
}

/// The workspace file the app opens with and writes back to.
///
/// In the durable cockpit home rather than the launch directory — see
/// [`crate::store_home`] for why the arrangement used to vanish.
#[must_use]
pub fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(UI_STATE_FILE);
    }
    crate::store_home::resolve(UI_STATE_ENV, UI_STATE_FILE)
}

crate::hooks::declare_hooks!["QUANTICK_UI_STATE"];
