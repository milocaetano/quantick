//! The symbols a trader added by hand: the document is
//! `quantick_stores::symbols_file`; this is where its path is resolved.

use std::path::PathBuf;

pub use quantick_stores::symbols_file::*;

/// Where the file lives: `QUANTICK_SYMBOLS`, else the durable cockpit home —
/// see [`crate::store_home`] for why hand-added symbols used to vanish.
#[must_use]
pub fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(SYMBOLS_FILE);
    }
    crate::store_home::resolve(SYMBOLS_ENV, SYMBOLS_FILE)
}

crate::hooks::declare_hooks!["QUANTICK_SYMBOLS"];
