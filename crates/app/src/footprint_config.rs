//! Footprint configuration: the document is `quantick_stores::footprint_config`;
//! this is where the environment reaches it.

use std::path::{Path, PathBuf};

pub use quantick_stores::footprint_config::*;

/// Environment override for the footprint config location.
#[cfg(any(feature = "scenario-harness", test))]
const FOOTPRINT_ENV: &str = "QUANTICK_FOOTPRINT";

/// Where this run persists in-app edits. Under test, a scratch file per app
/// for the same reason the chart-layers store does it: many test apps in one
/// process must not restore one another's knobs.
#[must_use]
pub fn settings_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(SETTINGS_FILE);
    }
    crate::store_home::resolve(SETTINGS_FILE)
}

/// The launch-time preset file: `QUANTICK_FOOTPRINT` (a capture pointing at
/// another preset; scenario harness only), else the tracked file in the
/// launch directory.
#[must_use]
pub fn preset_path() -> PathBuf {
    #[cfg(any(feature = "scenario-harness", test))]
    if let Some(explicit) = crate::hooks::captured::var(FOOTPRINT_ENV) {
        return PathBuf::from(explicit);
    }
    PathBuf::from(FOOTPRINT_FILE)
}

/// The settings overlaid on this run's preset: env > file > defaults.
#[must_use]
pub fn load(settings: &Path) -> FootprintConfig {
    quantick_stores::footprint_config::load(settings, &preset_path())
}

#[cfg(any(feature = "scenario-harness", test))]
crate::hooks::declare_hooks!["QUANTICK_FOOTPRINT"];
