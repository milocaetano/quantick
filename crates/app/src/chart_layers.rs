//! Bind the layer document to the desktop's durable home and registered layers.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(crate) use quantick_layers::{ChartLayer, LAYERS_FILE};
pub(crate) use quantick_stores::chart_layers::save;
mod session;
pub(crate) mod shortcuts;
pub(crate) use session::{maintain, restore};

const EMBEDDED_DEFAULT: &str = include_str!("../config/chart-layers.toml");

/// Tests use a private store; the desktop resolves its configured durable home.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(LAYERS_FILE);
    }
    crate::store_home::resolve(LAYERS_FILE)
}

#[must_use]
pub(crate) fn load(path: &Path) -> BTreeMap<ChartLayer, bool> {
    quantick_stores::chart_layers::load(path, EMBEDDED_DEFAULT, &crate::pane::registered_layers())
}

#[cfg(test)]
mod tests;
