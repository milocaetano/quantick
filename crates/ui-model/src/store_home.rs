//! Cockpit store membership and launch path policy over captured inputs.
//! Environment lookup and the documents-folder query belong to the caller.

use quantick_stores::home::*;
use quantick_workspace::write_refusal::guard_write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Platform resolvers keyed by store. Each retains its own test isolation
/// and captured per-run overrides.
pub struct StorePaths {
    pub alerts: fn() -> PathBuf,
    pub appearance: fn() -> PathBuf,
    pub ui_state: fn() -> PathBuf,
    pub indicators: fn() -> PathBuf,
    pub layouts: fn() -> PathBuf,
    pub workspaces: fn() -> PathBuf,
    pub market_settings: fn() -> PathBuf,
    pub indicator_presets: fn() -> PathBuf,
    pub bubble_assets: fn() -> PathBuf,
    pub chart_layers: fn() -> PathBuf,
    pub drawing_presets: fn() -> PathBuf,
    pub footprint_settings: fn() -> PathBuf,
    pub footprint_presets: fn() -> PathBuf,
    pub candle_presets: fn() -> PathBuf,
    pub paper_state: fn() -> PathBuf,
    pub symbols: fn() -> PathBuf,
}

/// The bundle writes these stores in this order. Every member belongs in the
/// durable home, including machine-local appearance and paper account state.
pub const fn cockpit_stores(paths: StorePaths) -> [CockpitStore; 16] {
    [
        CockpitStore {
            key: "alerts",
            file: quantick_stores::alerts::ALERTS_FILE,
            validate: quantick_stores::alerts::validate,
            path: paths.alerts,
            in_bundle: quantick_stores::alerts::BUNDLE_ENABLED,
            local_keys: &[],
        },
        CockpitStore {
            key: "appearance",
            file: quantick_stores::appearance::THEME_FILE,
            validate: quantick_stores::appearance::validate,
            path: paths.appearance,
            in_bundle: false,
            local_keys: &[],
        },
        CockpitStore {
            key: "ui_state",
            file: quantick_stores::ui_state::UI_STATE_FILE,
            validate: quantick_stores::ui_state::validate,
            path: paths.ui_state,
            in_bundle: true,
            local_keys: quantick_stores::ui_state::LOCAL_KEYS,
        },
        CockpitStore {
            key: "indicators",
            file: quantick_stores::indicator_state::STATE_FILE,
            validate: quantick_stores::indicator_state::validate,
            path: paths.indicators,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "layouts",
            file: quantick_stores::layouts::LAYOUTS_FILE,
            validate: quantick_stores::layouts::validate,
            path: paths.layouts,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "workspaces",
            file: quantick_stores::workspaces::WORKSPACES_FILE,
            validate: quantick_stores::workspaces::validate,
            path: paths.workspaces,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "market_settings",
            file: quantick_stores::market_settings::MARKET_SETTINGS_FILE,
            validate: quantick_stores::market_settings::validate,
            path: paths.market_settings,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "indicator_presets",
            file: quantick_stores::indicator_presets::PRESETS_FILE,
            validate: quantick_stores::indicator_presets::validate,
            path: paths.indicator_presets,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "bubble_assets",
            file: quantick_stores::bubble_assets::ASSETS_FILE,
            validate: quantick_stores::bubble_assets::validate,
            path: paths.bubble_assets,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "chart_layers",
            file: quantick_layers::LAYERS_FILE,
            validate: quantick_layers::validate,
            path: paths.chart_layers,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "drawing_presets",
            file: quantick_drawings::presets::PRESETS_FILE,
            validate: quantick_drawings::presets::validate,
            path: paths.drawing_presets,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "footprint_settings",
            file: quantick_stores::footprint_config::SETTINGS_FILE,
            validate: quantick_stores::footprint_config::validate_settings,
            path: paths.footprint_settings,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "footprint_presets",
            file: quantick_stores::footprint_presets::PRESETS_FILE,
            validate: quantick_stores::footprint_presets::validate,
            path: paths.footprint_presets,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "candle_presets",
            file: quantick_stores::candle_presets::PRESETS_FILE,
            validate: quantick_stores::candle_presets::validate,
            path: paths.candle_presets,
            in_bundle: true,
            local_keys: &[],
        },
        CockpitStore {
            key: "paper_state",
            file: quantick_paper::state::STATE_FILE,
            validate: quantick_paper::state::validate,
            path: paths.paper_state,
            in_bundle: false,
            local_keys: &[],
        },
        CockpitStore {
            key: "symbols",
            file: quantick_stores::symbols_file::SYMBOLS_FILE,
            validate: quantick_stores::symbols_file::validate,
            path: paths.symbols,
            in_bundle: true,
            local_keys: &[],
        },
    ]
}

#[cfg(any(feature = "scenario-harness", test))]
const CAPTURE_OVERRIDES: &[(&str, &str)] = &[
    (
        quantick_stores::bubble_assets::ASSETS_FILE,
        "QUANTICK_BUBBLE_ASSETS",
    ),
    (
        quantick_stores::candle_presets::PRESETS_FILE,
        "QUANTICK_CANDLE_PRESETS",
    ),
    (quantick_layers::LAYERS_FILE, "QUANTICK_CHART_LAYERS"),
    (
        quantick_drawings::presets::PRESETS_FILE,
        "QUANTICK_DRAWING_PRESETS",
    ),
    (
        quantick_stores::footprint_config::SETTINGS_FILE,
        "QUANTICK_FOOTPRINT_SETTINGS",
    ),
    (
        quantick_stores::footprint_presets::PRESETS_FILE,
        "QUANTICK_FOOTPRINT_PRESETS",
    ),
    (
        quantick_stores::indicator_presets::PRESETS_FILE,
        "QUANTICK_INDICATOR_PRESETS",
    ),
    (
        quantick_stores::indicator_state::STATE_FILE,
        "QUANTICK_INDICATORS_STATE",
    ),
    (
        quantick_paper::state::INSTRUMENTS_FILE,
        "QUANTICK_INSTRUMENTS",
    ),
    (quantick_stores::layouts::LAYOUTS_FILE, "QUANTICK_LAYOUTS"),
    (
        quantick_stores::market_settings::MARKET_SETTINGS_FILE,
        "QUANTICK_MARKET_SETTINGS",
    ),
    (quantick_paper::state::STATE_FILE, "QUANTICK_PAPER_STATE"),
    (
        quantick_strategy::presets::STRATEGIES_FILE,
        "QUANTICK_STRATEGY_PRESETS",
    ),
    (
        quantick_stores::symbols_file::SYMBOLS_FILE,
        "QUANTICK_SYMBOLS",
    ),
    (
        quantick_stores::ui_state::UI_STATE_FILE,
        "QUANTICK_UI_STATE",
    ),
];

#[cfg(any(feature = "scenario-harness", test))]
pub fn capture_override(
    file: &str,
    lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    CAPTURE_OVERRIDES
        .iter()
        .find(|(store, _)| *store == file)
        .and_then(|(_, env)| lookup(env))
}

#[cfg(not(any(feature = "scenario-harness", test)))]
fn capture_override(
    _: &str,
    _: impl Fn(&str) -> Option<std::ffi::OsString>,
) -> Option<std::ffi::OsString> {
    None
}

/// Process-scoped home and rescue notice; a caller supplies the platform shelf.
#[derive(Default)]
pub struct StoreHome {
    home: OnceLock<Option<PathBuf>>,
    notice: OnceLock<Option<String>>,
}

impl StoreHome {
    pub const fn new() -> Self {
        Self {
            home: OnceLock::new(),
            notice: OnceLock::new(),
        }
    }

    /// Cache the platform shelf once, including failure to create it.
    pub fn home(&self, shelf: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
        self.home.get_or_init(|| {
            let shelf = shelf()?;
            match std::fs::create_dir_all(&shelf) {
                Ok(()) => Some(shelf),
                Err(error) => {
                    tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                        event_code = "COCKPIT_HOME_UNAVAILABLE", path = %shelf.display(), %error,
                        action = "keeping_launch_directory",
                        "the documents folder cannot hold the cockpit; falling back to the launch directory");
                    None
                }
            }
        }).clone()
    }

    /// A per-run override wins over the shelf, then the launch-relative path.
    pub fn resolve(
        &self,
        file: &str,
        shelf: impl FnOnce() -> Option<PathBuf>,
        lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
    ) -> PathBuf {
        if let Some(explicit) = capture_override(file, lookup) {
            return PathBuf::from(explicit);
        }
        resolve_in(self.home(shelf), file)
    }

    /// Rescue copies missing files only. A completed rescue is marked in the
    /// durable home, so changing the launch directory cannot replace a copy.
    pub fn consolidate_once(
        &self,
        skip: bool,
        shelf: impl FnOnce() -> Option<PathBuf>,
        stores: &[CockpitStore],
        lookup: impl Fn(&str) -> Option<std::ffi::OsString>,
        theme_overridden: impl Fn() -> bool,
    ) -> Option<RescueSummary> {
        // Test launches must never query, copy into or stamp a real home.
        if skip {
            return None;
        }
        let home = self.home(shelf)?;
        guard_write(&home).ok()?;
        let summary = rescue_into(&home, Path::new("."), stores, &|file| {
            capture_override(file, &lookup).is_some()
                || (file == quantick_stores::appearance::THEME_FILE && theme_overridden())
        })?;
        let _ = self.notice.set(rescue_toast(&summary, &home));
        Some(summary)
    }

    pub fn rescue_notice(&self) -> Option<String> {
        self.notice.get().cloned().flatten()
    }
}

#[cfg(test)]
mod tests;
