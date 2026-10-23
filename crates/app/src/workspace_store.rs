//! Resolved cockpit paths and desktop storage ports.
//!
//! LayoutSession, LayoutCommitPolicy and WorkspaceCommitSession own the
//! documents, membership and persistence decisions below the app. This shell
//! holds their handles alongside resolved paths, layer state and pending OS
//! dialogs. It neither serializes documents nor resolves environment overrides.
//! The recent-on-disk menu projection stays here because filesystem existence
//! is a shell observation, refreshed only on adoption or a recent-file visit.

use std::path::{Path, PathBuf};

use quantick_workspace::workspace_commit::WorkspaceCommitSession;

pub(crate) use quantick_workspace::layout_commit::LayoutSave;

pub(crate) use quantick_workspace::layout_store::LayoutStore;

pub(crate) use quantick_stores::paths::StorePaths;

use quantick_layers::SavedLayers;

/// The app's one handle on where the workspace lives and whether it is saved.
///
/// One field on `QuantickApp` where there were twenty-one.
///
/// Four named parts stay separate inside because they answer different
/// questions and have different lifetimes — a path is fixed for the run, the
/// layout rule changes every edit, the layer baseline changes per tab, the
/// session state changes per menu action — and folding them into one flat bag
/// would lose exactly the structure that makes the layout rule guardable.
///
/// Three fields sit beside those parts rather than inside one, because they
/// belong to no store in particular: the two in-flight OS file dialogs, which
/// are answers on their way back from outside the process, and the trades
/// folder, which is a machine fact this window hands to every tab it opens.
pub(crate) struct WorkspaceStore {
    paths: StorePaths,
    layouts: LayoutStore,
    layers: SavedLayers,
    session: WorkspaceCommitSession,
    recent_on_disk: Vec<PathBuf>,
    /// The native file dialog, while one is open, and what it is for. One at a
    /// time, and off the UI thread — the OS dialog never blocks a frame.
    picker: crate::workspace_picker::WorkspacePickerHost,
    /// Where trades save this run — resolved once at boot (environment > the
    /// user's stored pick > config) and updated by the panel's folder picker;
    /// new tabs journal here too.
    trades_dir: PathBuf,
    /// The in-flight trades-folder dialog, if any. One at a time.
    trades_dir_picker: Option<std::sync::mpsc::Receiver<Option<PathBuf>>>,
    /// Every asset's bubble settings, which every tab binds to.
    bubble_assets: quantick_stores::bubble_asset_store::SharedAssetBubbles,
}

#[cfg(test)]
pub(crate) use crate::workspace_picker::WorkspacePick;

impl WorkspaceStore {
    /// Build the handle from paths each store has already resolved for itself.
    ///
    /// Every argument arrives resolved. Nothing in this module reads an
    /// environment variable or calls [`crate::store_home::resolve`], so a
    /// store pointed at a scratch file by `QUANTICK_*` still writes there.
    pub(crate) fn new(paths: StorePaths, layouts: LayoutStore, trades_dir: PathBuf) -> Self {
        Self {
            layouts,
            layers: SavedLayers::default(),
            session: WorkspaceCommitSession::default(),
            recent_on_disk: Vec::new(),
            picker: crate::workspace_picker::WorkspacePickerHost::default(),
            trades_dir,
            trades_dir_picker: None,
            bubble_assets: quantick_stores::bubble_asset_store::AssetBubblesStore::load(
                paths.bubble_assets.clone(),
            )
            .shared(),
            paths,
        }
    }

    /// Every asset's bubble settings, which every tab binds to.
    pub(crate) fn bubble_assets(&self) -> &quantick_stores::bubble_asset_store::SharedAssetBubbles {
        &self.bubble_assets
    }

    /// Where the picker's added instruments persist.
    pub(crate) fn symbols_path(&self) -> &Path {
        &self.paths.symbols
    }

    /// Where layer visibility persists.
    pub(crate) fn chart_layers_path(&self) -> &Path {
        &self.paths.chart_layers
    }

    /// Where the footprint's live edits persist.
    pub(crate) fn footprint_settings_path(&self) -> &Path {
        &self.paths.footprint_settings
    }

    /// Where the settings dialog's named input setups persist.
    pub(crate) fn indicator_presets_path(&self) -> &Path {
        &self.paths.indicator_presets
    }

    /// Where the workspace persists.
    pub(crate) fn ui_state_path(&self) -> &Path {
        &self.paths.ui_state
    }

    /// The layout book and the rule that guards it.
    pub(crate) fn layouts(&self) -> &LayoutStore {
        &self.layouts
    }

    /// The same, for the edits and the debounced save.
    pub(crate) fn layouts_mut(&mut self) -> &mut LayoutStore {
        &mut self.layouts
    }

    /// What the chart-layer file already says.
    #[cfg(test)]
    pub(crate) fn layers(&self) -> &SavedLayers {
        &self.layers
    }

    /// The same, for the per-frame compare that writes a switch down.
    pub(crate) fn layers_mut(&mut self) -> &mut SavedLayers {
        &mut self.layers
    }

    /// What the Workspace menu knows without asking the filesystem.
    pub(crate) fn session(&self) -> &WorkspaceCommitSession {
        &self.session
    }

    /// The same, for the menu actions that change it.
    pub(crate) fn session_mut(&mut self) -> &mut WorkspaceCommitSession {
        &mut self.session
    }

    pub(crate) fn recent_on_disk(&self) -> &[PathBuf] {
        &self.recent_on_disk
    }
    pub(crate) fn set_recent_on_disk(&mut self, existing: Vec<PathBuf>) {
        self.recent_on_disk = existing;
    }
    pub(crate) fn commit_parts(&mut self) -> (&mut WorkspaceCommitSession, &Path) {
        (&mut self.session, &self.paths.ui_state)
    }

    #[cfg(test)]
    /// Whether a workspace file dialog is already open. One at a time.
    pub(crate) fn picker_open(&self) -> bool {
        self.picker.is_open()
    }
    pub(crate) fn picker_mut(&mut self) -> &mut crate::workspace_picker::WorkspacePickerHost {
        &mut self.picker
    }

    /// Point a store at a scratch file for the length of one test.
    ///
    /// Test-only, and deliberately so: in a running app a store's path is
    /// resolved once at construction and never moves, which is what makes
    /// "where does this write?" answerable from the launch alone. A test needs
    /// the opposite — it builds an app, then redirects one store at the
    /// temporary file it is about to inspect — and that is the only reason a
    /// setter exists at all. It compiles out of the shipped binary.
    #[cfg(test)]
    pub(crate) fn set_ui_state_path(&mut self, path: PathBuf) {
        self.paths.ui_state = path;
    }

    /// See [`Self::set_ui_state_path`].
    #[cfg(test)]
    pub(crate) fn set_symbols_path(&mut self, path: PathBuf) {
        self.paths.symbols = path;
    }

    /// See [`Self::set_ui_state_path`].
    #[cfg(test)]
    pub(crate) fn set_chart_layers_path(&mut self, path: PathBuf) {
        self.paths.chart_layers = path;
    }

    /// See [`Self::set_ui_state_path`].
    #[cfg(test)]
    pub(crate) fn set_layouts_path(&mut self, path: PathBuf) {
        self.layouts.set_path(path);
    }

    /// Where trades save this run.
    pub(crate) fn trades_dir(&self) -> &Path {
        &self.trades_dir
    }

    /// The panel's folder picker chose a new one.
    pub(crate) fn set_trades_dir(&mut self, dir: PathBuf) {
        self.trades_dir = dir;
    }

    /// Whether a trades-folder dialog is already open. One at a time.
    pub(crate) fn trades_dir_picker_open(&self) -> bool {
        self.trades_dir_picker.is_some()
    }

    /// Hand the in-flight folder dialog over, so its poll can read the
    /// channel.
    pub(crate) fn trades_dir_picker(&self) -> Option<&std::sync::mpsc::Receiver<Option<PathBuf>>> {
        self.trades_dir_picker.as_ref()
    }

    /// A folder dialog just opened.
    pub(crate) fn open_trades_dir_picker(
        &mut self,
        receiver: std::sync::mpsc::Receiver<Option<PathBuf>>,
    ) {
        self.trades_dir_picker = Some(receiver);
    }

    /// The folder dialog answered, or the channel died.
    pub(crate) fn close_trades_dir_picker(&mut self) {
        self.trades_dir_picker = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tab_switch_rebaselines_the_layers_rather_than_recording_a_switch() {
        let mut layers = SavedLayers::new(1, 0b101);
        layers.rebaseline(2, 0b010);
        assert_eq!(layers.tab(), 2);
        assert_eq!(
            layers.mask(),
            0b010,
            "the other tab's opinion becomes the baseline, not an edit to write"
        );
        layers.record(0b011);
        assert_eq!(layers.mask(), 0b011);
        assert_eq!(
            layers.tab(),
            2,
            "recording a mask does not change whose it is"
        );
    }
}
