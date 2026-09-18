//! Footprint configuration: the document is `quantick_stores::footprint_config`;
//! this is where the environment reaches it.

use std::path::{Path, PathBuf};

pub use quantick_stores::footprint_config::*;

/// Where this run persists in-app edits. Under test, a scratch file per app
/// for the same reason the chart-layers store does it: many test apps in one
/// process must not restore one another's knobs.
#[must_use]
pub fn settings_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(SETTINGS_FILE);
    }
    crate::store_home::resolve(SETTINGS_ENV, SETTINGS_FILE)
}

/// The launch-time preset file: `QUANTICK_FOOTPRINT`, else the tracked file
/// in the launch directory.
#[must_use]
pub fn preset_path() -> PathBuf {
    std::env::var_os(FOOTPRINT_ENV).map_or_else(|| PathBuf::from(FOOTPRINT_FILE), PathBuf::from)
}

/// The settings overlaid on this run's preset: env > file > defaults.
#[must_use]
pub fn load(settings: &Path) -> FootprintConfig {
    quantick_stores::footprint_config::load(settings, &preset_path())
}

crate::hooks::declare_hooks!["QUANTICK_FOOTPRINT", "QUANTICK_FOOTPRINT_SETTINGS"];
