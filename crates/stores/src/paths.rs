//! Resolved workspace store locations supplied by the process shell.
use std::path::PathBuf;

/// Where the cockpit's stores live this run.
///
/// Five paths, each resolved by its own module and handed in. The sixth — the
/// layouts file — is not here but on [`quantick_workspace::layout_store::LayoutStore`], beside the rule that
/// decides when it is written: a path and the question "may I write it yet?"
/// are one subject, and splitting them is what this module exists to undo.
///
/// A carrier, deliberately without behaviour: the moment it grew a `resolve`
/// it would become a second answer to a question the application home resolver already
/// answers, and the two would drift.
pub struct StorePaths {
    /// Where the picker's added instruments persist. See
    /// [`crate::symbols_file`].
    pub symbols: PathBuf,
    /// Where layer visibility persists.
    pub chart_layers: PathBuf,
    /// Where the footprint's live edits persist.
    pub footprint_settings: PathBuf,
    /// Where the settings dialog's named input setups persist.
    pub indicator_presets: PathBuf,
    /// Where the workspace persists.
    pub ui_state: PathBuf,
    /// Where each asset's bubble settings persist.
    pub bubble_assets: PathBuf,
}
