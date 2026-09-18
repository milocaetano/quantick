//! Indicator presets: the document is `quantick_stores::indicator_presets`;
//! this is where its path is resolved.

use std::path::PathBuf;

pub(crate) use quantick_stores::indicator_presets::*;

/// Where this run keeps its presets. Under test, a scratch file per store,
/// for the same reason every other app-state path does it.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(PRESETS_FILE);
    }
    crate::store_home::resolve(PRESETS_ENV, PRESETS_FILE)
}

crate::hooks::declare_hooks!["QUANTICK_INDICATOR_PRESETS"];
