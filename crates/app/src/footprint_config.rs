//! The footprint layer's tunables: the signal thresholds the team consult
//! kept, nothing more.
//!
//! Four knobs and two switches (docs/footprint-design.md): the diagonal
//! imbalance ratio and its minimum-quantity floor, the stack length that
//! makes a zone, the POC line and the extreme ratio badges. The min-qty floor
//! defaults to *adaptive* — a percentile of what is actually printing —
//! because one fixed number cannot serve WIN contracts and BTC fractions at
//! once; the file can pin it for an instrument the trader knows better.
//!
//! Same resolution and tolerance discipline as the bubble presets: an env
//! override (`QUANTICK_FOOTPRINT`), then `./config/footprint.toml`, then the
//! built-in defaults. A missing file *is* the defaults — config presence
//! never switches the layer on — and an unreadable one is logged and
//! ignored, never fatal.

use std::path::{Path, PathBuf};

use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive as _, ToPrimitive as _};
use serde::{Deserialize, Serialize};

/// Environment override for the footprint config location.
const FOOTPRINT_ENV: &str = "QUANTICK_FOOTPRINT";
/// Default file, next to the working directory's config.
const FOOTPRINT_FILE: &str = "config/footprint.toml";
/// Environment override for where the in-app edits persist.
const SETTINGS_ENV: &str = "QUANTICK_FOOTPRINT_SETTINGS";
/// Where the in-app edits persist, next to the chart-layers file. Separate
/// from `config/footprint.toml` on purpose: that file is a hand-written,
/// commented preset the app must never rewrite; this one is app state.
const SETTINGS_FILE: &str = "footprint-settings.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
const SETTINGS_VERSION: u32 = 1;

/// The resolved tunables the render layer reads every frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FootprintConfig {
    /// Diagonal imbalance: one side must be at least this many times its
    /// diagonal neighbour. 3:1 is the industry's centre of gravity.
    pub imbalance_ratio: Decimal,
    /// Absolute floor on the imbalance difference. `None` (the default) means
    /// adaptive — the 60th percentile of per-row volume over the newest
    /// closed bars.
    pub imbalance_min_qty: Option<Decimal>,
    /// Consecutive same-side imbalances that make a stacked zone.
    pub stacked_count: usize,
    /// The POC line per bar.
    pub show_poc: bool,
    /// The aggression ratio badges at a bar's extremes (Detailed level only).
    pub extreme_ratio_badge: bool,
}

impl Default for FootprintConfig {
    fn default() -> Self {
        Self {
            imbalance_ratio: Decimal::from(3),
            imbalance_min_qty: None,
            stacked_count: 3,
            show_poc: true,
            extreme_ratio_badge: true,
        }
    }
}

/// The file's shape. Every field optional: an absent knob keeps its default,
/// so a one-line file tuning the ratio changes exactly the ratio.
#[derive(Debug, Default, Deserialize)]
struct FootprintFile {
    imbalance_ratio: Option<f64>,
    imbalance_min_qty: Option<f64>,
    stacked_count: Option<usize>,
    show_poc: Option<bool>,
    extreme_ratio_badge: Option<bool>,
}

/// The persisted shape of the in-app edits. Every knob explicit: this file
/// is written by the app, so "absent" would only ever mean a version skew.
#[derive(Debug, Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    imbalance_ratio: f64,
    imbalance_min_qty: Option<f64>,
    stacked_count: usize,
    show_poc: bool,
    extreme_ratio_badge: bool,
}

/// Where this run persists in-app edits. Under test, a scratch file per app
/// for the same reason the chart-layers store does it: many test apps in one
/// process must not restore one another's knobs.
#[must_use]
pub fn settings_path() -> PathBuf {
    if cfg!(test) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        return std::env::temp_dir().join(format!(
            "quantick-footprint-settings-{}-{}.toml",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    }
    std::env::var_os(SETTINGS_ENV).map_or_else(|| PathBuf::from(SETTINGS_FILE), PathBuf::from)
}

/// Resolve the config for this run: the preset file (env >
/// `config/footprint.toml` > defaults), then whatever the user last set in
/// the app on top — a knob touched in the menu outlives the restart, exactly
/// like a layer switch does. See the [module docs](self).
#[must_use]
pub fn load(settings: &Path) -> FootprintConfig {
    let base = load_preset();
    let Ok(text) = std::fs::read_to_string(settings) else {
        return base;
    };
    match toml::from_str::<SettingsFile>(&text) {
        Ok(file) if file.version == SETTINGS_VERSION => resolve(FootprintFile {
            imbalance_ratio: Some(file.imbalance_ratio),
            imbalance_min_qty: file.imbalance_min_qty,
            stacked_count: Some(file.stacked_count),
            show_poc: Some(file.show_poc),
            extreme_ratio_badge: Some(file.extreme_ratio_badge),
        }),
        Ok(file) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FOOTPRINT_SETTINGS_VERSION",
                path = %settings.display(),
                version = file.version,
                action = "keeping_preset_config",
                "footprint settings file is from an unknown version"
            );
            base
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FOOTPRINT_SETTINGS_UNREADABLE",
                path = %settings.display(),
                %error,
                action = "keeping_preset_config",
                "footprint settings file is unreadable"
            );
            base
        }
    }
}

/// Persist the in-app edits. Temp sibling + rename, the store discipline
/// every state file here follows.
pub fn save(settings: &Path, config: &FootprintConfig) {
    let file = SettingsFile {
        version: SETTINGS_VERSION,
        imbalance_ratio: config.imbalance_ratio.to_f64().unwrap_or(3.0),
        imbalance_min_qty: config.imbalance_min_qty.and_then(|qty| qty.to_f64()),
        stacked_count: config.stacked_count,
        show_poc: config.show_poc,
        extreme_ratio_badge: config.extreme_ratio_badge,
    };
    let Ok(text) = toml::to_string_pretty(&file) else {
        return;
    };
    let temp = settings.with_extension("toml.tmp");
    if let Err(error) = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, settings))
    {
        let _ = std::fs::remove_file(&temp);
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "FOOTPRINT_SETTINGS_WRITE_FAILED",
            path = %settings.display(),
            %error,
            action = "settings_not_saved",
            "could not save the footprint settings"
        );
    }
}

/// The preset half of [`load`]: env > file > defaults, tolerant.
fn load_preset() -> FootprintConfig {
    let path = std::env::var_os(FOOTPRINT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(FOOTPRINT_FILE));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return FootprintConfig::default();
    };
    match toml::from_str::<FootprintFile>(&text) {
        Ok(file) => resolve(file),
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "FOOTPRINT_CONFIG_UNREADABLE",
                path = %path.display(),
                %error,
                action = "keeping_default_footprint_config",
                "footprint config is unreadable"
            );
            FootprintConfig::default()
        }
    }
}

/// Apply the file over the defaults, refusing values that would break the
/// signals' meaning (a ratio under 1 inverts the test; a stack of 1 is not a
/// stack) rather than letting them through to draw nonsense.
fn resolve(file: FootprintFile) -> FootprintConfig {
    let defaults = FootprintConfig::default();
    FootprintConfig {
        imbalance_ratio: file
            .imbalance_ratio
            .and_then(Decimal::from_f64)
            .filter(|ratio| *ratio >= Decimal::ONE)
            .unwrap_or(defaults.imbalance_ratio),
        imbalance_min_qty: file
            .imbalance_min_qty
            .and_then(Decimal::from_f64)
            .filter(|qty| *qty >= Decimal::ZERO),
        stacked_count: file
            .stacked_count
            .filter(|count| *count >= 2)
            .unwrap_or(defaults.stacked_count),
        show_poc: file.show_poc.unwrap_or(defaults.show_poc),
        extreme_ratio_badge: file
            .extreme_ratio_badge
            .unwrap_or(defaults.extreme_ratio_badge),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    #[test]
    fn absent_knobs_keep_their_defaults() {
        let config = resolve(toml::from_str("imbalance_ratio = 4.0").unwrap());
        assert_eq!(config.imbalance_ratio, Decimal::from(4));
        assert_eq!(
            config,
            FootprintConfig {
                imbalance_ratio: Decimal::from(4),
                ..FootprintConfig::default()
            }
        );
    }

    #[test]
    fn a_pinned_min_qty_overrides_the_adaptive_floor() {
        let config = resolve(toml::from_str("imbalance_min_qty = 20.0").unwrap());
        assert_eq!(
            config.imbalance_min_qty,
            Some(Decimal::from_str("20").unwrap())
        );
        assert_eq!(FootprintConfig::default().imbalance_min_qty, None);
    }

    #[test]
    fn meaning_breaking_values_are_refused() {
        let config = resolve(
            toml::from_str("imbalance_ratio = 0.5\nstacked_count = 1\nimbalance_min_qty = -5.0")
                .unwrap(),
        );
        assert_eq!(config, FootprintConfig::default());
    }

    /// In-app edits round-trip through disk and win over the preset file's
    /// defaults on the next load.
    #[test]
    fn saved_settings_round_trip_and_overlay_the_preset() {
        let path = settings_path();
        let edited = FootprintConfig {
            imbalance_ratio: Decimal::from(5),
            imbalance_min_qty: Some(Decimal::from(40)),
            stacked_count: 4,
            show_poc: false,
            extreme_ratio_badge: false,
        };
        save(&path, &edited);
        assert_eq!(load(&path), edited);
        std::fs::remove_file(&path).ok();
    }

    /// A missing settings file keeps the preset resolution; an unknown
    /// version or garbage degrades to it instead of failing.
    #[test]
    fn settings_degrade_to_the_preset_instead_of_failing() {
        let missing = settings_path();
        assert_eq!(load(&missing), FootprintConfig::default());

        let path = settings_path();
        std::fs::write(&path, "version = 99\nimbalance_ratio = 9.0\nstacked_count = 2\nshow_poc = false\nextreme_ratio_badge = false\n").unwrap();
        assert_eq!(load(&path), FootprintConfig::default());
        std::fs::write(&path, "not even toml [").unwrap();
        assert_eq!(load(&path), FootprintConfig::default());
        std::fs::remove_file(&path).ok();
    }

    /// The saved file still refuses meaning-breaking values on the way back
    /// in — hand-edits to the settings file get the same guard the preset
    /// has.
    #[test]
    fn saved_settings_are_validated_on_load() {
        let path = settings_path();
        std::fs::write(
            &path,
            "version = 1\nimbalance_ratio = 0.2\nstacked_count = 1\nshow_poc = true\nextreme_ratio_badge = true\n",
        )
        .unwrap();
        assert_eq!(load(&path), FootprintConfig::default());
        std::fs::remove_file(&path).ok();
    }
}
