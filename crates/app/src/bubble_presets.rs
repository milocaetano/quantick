//! Aggression-bubble presets: the document is `quantick_stores::bubble_presets`;
//! this is where the launch root's `QUANTICK_BUBBLES` reaches it.

use std::path::PathBuf;

pub use quantick_stores::bubble_presets::*;

/// Path the panel reads from and writes to.
#[must_use]
pub fn presets_path() -> PathBuf {
    crate::launch::operator_paths()
        .bubbles
        .clone()
        .map_or_else(|| PathBuf::from(PRESETS_PATH), PathBuf::from)
}

/// Where each asset's bubble settings are kept: the cockpit home.
#[must_use]
pub fn assets_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(quantick_stores::bubble_assets::ASSETS_FILE);
    }
    crate::store_home::resolve(quantick_stores::bubble_assets::ASSETS_FILE)
}

/// Load presets from the launch root's `QUANTICK_BUBBLES` or the working
/// directory, falling back to the embedded file ([`load_from`]).
#[must_use]
pub fn load() -> (BubblePresetFile, PresetSource, Option<String>) {
    load_from(
        crate::launch::operator_paths()
            .bubbles
            .clone()
            .map(PathBuf::from),
    )
}

/// Write `file` to [`presets_path`], returning where it landed.
///
/// # Errors
///
/// Returns a human-readable message when the file cannot be serialized or
/// written.
pub fn save(file: &BubblePresetFile) -> Result<PathBuf, String> {
    save_to(presets_path(), file)
}
