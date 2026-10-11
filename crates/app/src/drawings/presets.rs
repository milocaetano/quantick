//! Where the drawing-preset file lives. The store itself is headless, in
//! `quantick_drawings::presets`; the window only knows the cockpit home.

use std::path::PathBuf;

pub use quantick_drawings::presets::*;

/// Resolve the preset file: the env override first, then the durable cockpit
/// home — see [`crate::store_home`] for why the tool colours used to vanish
/// when the app was launched from elsewhere.
#[must_use]
pub fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(PRESETS_FILE);
    }
    crate::store_home::resolve(PRESETS_FILE)
}
