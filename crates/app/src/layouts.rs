//! Where the layouts and market files live, and the names the app reads them by.
//! The documents and their rules are `quantick_workspace` and `quantick_stores`.

use std::path::PathBuf;

#[cfg(test)]
pub(crate) use quantick_drawings::SavedDrawingExt;

#[cfg(test)]
pub(crate) use quantick_stores::layouts::save;
pub(crate) use quantick_stores::layouts::{LAYOUTS_FILE, load_or_starter, validate};
#[cfg(test)]
pub(crate) use quantick_stores::layouts::{Loaded, load};
pub(crate) use quantick_stores::market_settings::MARKET_SETTINGS_FILE;
#[cfg(test)]
use quantick_workspace::layout_document::parse;
pub use quantick_workspace::layout_document::{
    ChartLayout, LayoutBook, LayoutError, LayoutId, MAX_LAYOUT_NAME, MAX_LAYOUTS, SavedOrigin,
    clean_name,
};
#[cfg(test)]
pub(crate) use quantick_workspace::layout_document::{SavedBand, SavedDrawing, SavedPoint};
pub use quantick_workspace::market_settings::DrawingKey;
#[cfg(test)]
pub(crate) use quantick_workspace::market_settings::PaneRole;

/// The layouts file this cockpit opens with and writes back to.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(LAYOUTS_FILE);
    }
    crate::store_home::resolve(LAYOUTS_FILE)
}

/// The workspaces file, which lives beside the layouts file.
#[must_use]
pub(crate) fn workspaces_path() -> PathBuf {
    quantick_stores::workspaces::path_beside(&default_path())
}

/// The market store this cockpit opens with. A run pointed at a scratch layouts
/// file and no market file stays beside it: its migration must never write
/// into the trader's home.
#[must_use]
pub(crate) fn market_settings_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(MARKET_SETTINGS_FILE);
    }
    #[cfg(feature = "scenario-harness")]
    if crate::hooks::captured::var("QUANTICK_MARKET_SETTINGS").is_none()
        && let Some(layouts) = crate::hooks::captured::var("QUANTICK_LAYOUTS")
    {
        return PathBuf::from(layouts).with_file_name(MARKET_SETTINGS_FILE);
    }
    crate::store_home::resolve(MARKET_SETTINGS_FILE)
}

#[cfg(test)]
mod tests;
