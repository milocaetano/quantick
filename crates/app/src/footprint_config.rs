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

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use serde::Deserialize;

/// Environment override for the footprint config location.
const FOOTPRINT_ENV: &str = "QUANTICK_FOOTPRINT";
/// Default file, next to the working directory's config.
const FOOTPRINT_FILE: &str = "config/footprint.toml";

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

/// Resolve the config for this run. See the [module docs](self).
#[must_use]
pub fn load() -> FootprintConfig {
    let path = std::env::var_os(FOOTPRINT_ENV)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(FOOTPRINT_FILE));
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
}
