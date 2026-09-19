//! The strategy preset bank: the document is `quantick_strategy::presets`;
//! this is where its path is resolved.

use std::path::PathBuf;

pub(crate) use quantick_strategy::presets::*;

/// Resolve the bank file: the env override first, then the durable cockpit
/// home. Under test, a scratch file per store.
#[must_use]
pub fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(STRATEGIES_FILE);
    }
    crate::store_home::resolve(STRATEGIES_FILE)
}
