//! The indicator set: the document is `quantick_stores::indicator_state`;
//! this is where its path is resolved.

use std::path::PathBuf;

pub(crate) use quantick_stores::indicator_state::*;

/// The state file the app opens with and writes back to.
///
/// In the durable cockpit home rather than the launch directory — see
/// [`crate::store_home`] for why the indicator set used to vanish.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(STATE_FILE);
    }
    crate::store_home::resolve(STATE_ENV, STATE_FILE)
}

crate::hooks::declare_hooks!["QUANTICK_INDICATORS_STATE"];
