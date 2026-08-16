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
pub(crate) const SETTINGS_ENV: &str = "QUANTICK_FOOTPRINT_SETTINGS";
/// Where the in-app edits persist, next to the chart-layers file. Separate
/// from `config/footprint.toml` on purpose: that file is a hand-written,
/// commented preset the app must never rewrite; this one is app state.
pub(crate) const SETTINGS_FILE: &str = "footprint-settings.toml";
/// Bumped on breaking layout changes; unknown versions are ignored. v2
/// moved the knobs under a `[config]` table, the shape presets share.
const SETTINGS_VERSION: u32 = 2;

/// How a bar's ladder is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FootprintStyle {
    /// The default, after the boss's reference charts: a two-sided display
    /// inside the candle — total-volume profile on the right (neutral), a
    /// delta bar per row on the left colored by the winning side, delta
    /// numbers at deep zoom, yellow POC line.
    Split,
    /// The classic sell|buy ladder: two number columns per row.
    Ladder,
}

impl FootprintStyle {
    /// The token stored in files.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::Split => "split",
            Self::Ladder => "ladder",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        match id {
            "split" => Some(Self::Split),
            "ladder" => Some(Self::Ladder),
            _ => None,
        }
    }
}

/// The resolved tunables the render layer reads every frame.
#[derive(Debug, Clone, PartialEq)]
pub struct FootprintConfig {
    /// How the ladder is drawn (see [`FootprintStyle`]).
    pub style: FootprintStyle,
    /// Diagonal imbalance: one side must be at least this many times its
    /// diagonal neighbour. 3:1 is the industry's centre of gravity.
    pub imbalance_ratio: Decimal,
    /// Absolute floor on the imbalance difference. `None` (the default) means
    /// adaptive — the 60th percentile of per-row volume over the newest
    /// closed bars.
    pub imbalance_min_qty: Option<Decimal>,
    /// Consecutive same-side imbalances that make a stacked zone.
    pub stacked_count: usize,
    /// The thinnest a profile row may draw, in pixels. Lower = finer bands
    /// (more rows per candle before the display grouping merges them);
    /// clamped to 2–10 px — under 2 px a band is a moiré, over 10 the
    /// profile is a staircase. Text levels keep their own legibility floors.
    pub profile_row_px: f32,
    /// How early the ladder's levels arrive, as a multiplier on the
    /// candle-width each one needs.
    ///
    /// `1.0` is the typographic budget itself — the width at which that
    /// level's text is drawn at its smallest legible size. Below 1 the levels
    /// arrive at narrower candles and the numbers sit tighter; above 1 they
    /// wait for more room. Clamped to [`DETAIL_SCALE_RANGE`]: the low end
    /// still fits the digits it draws, and past the high end a trader may as
    /// well switch the layer off.
    pub detail_scale: f32,
    /// The POC line per bar.
    pub show_poc: bool,
    /// The aggression ratio badges at a bar's extremes (Detailed level only).
    pub extreme_ratio_badge: bool,
    /// Badges below this ratio are suppressed: "1.0x" is the absence of
    /// aggression, and a badge that always appears is anti-signal.
    pub badge_min_ratio: Decimal,
    /// The per-row numbers inside the candles. Off leaves the bars, chips,
    /// POC and badges — the shape of the fight without the digits, which is
    /// how some traders prefer to read a dense tape.
    pub show_numbers: bool,
    /// The per-bar delta totals along the chart's bottom edge.
    pub show_delta_totals: bool,
}

impl Default for FootprintConfig {
    fn default() -> Self {
        Self {
            style: FootprintStyle::Split,
            imbalance_ratio: Decimal::from(3),
            imbalance_min_qty: None,
            stacked_count: 3,
            profile_row_px: 4.0,
            detail_scale: 1.0,
            show_poc: true,
            extreme_ratio_badge: true,
            badge_min_ratio: Decimal::TWO,
            show_numbers: true,
            show_delta_totals: true,
        }
    }
}

/// The bounds [`FootprintConfig::profile_row_px`] is clamped to, shared by
/// the file guard and the settings window's slider.
pub const PROFILE_ROW_PX_RANGE: std::ops::RangeInclusive<f32> = 2.0..=10.0;

/// The bounds [`FootprintConfig::detail_scale`] is clamped to.
///
/// The floors it scales are already derived from the smallest font the ladder
/// draws, so `1.0` is where the numbers exactly fit their candle. Below it a
/// trader is deliberately trading room for earliness — the digits start to
/// crowd their neighbour — and 0.8 is as far as that stays a chart rather
/// than a smear. The ceiling is generous: waiting longer for detail costs
/// nothing but zoom.
pub const DETAIL_SCALE_RANGE: std::ops::RangeInclusive<f32> = 0.8..=2.0;

/// The file's shape, and the canonical serde form of a config: the
/// hand-written preset, each saved named preset and the settings file all
/// speak it. Every field optional — an absent knob keeps its default, so a
/// one-line file tuning the ratio changes exactly the ratio, and a file
/// written by an older build still loads.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct FootprintFile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) imbalance_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) imbalance_min_qty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stacked_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) profile_row_px: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) show_poc: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) extreme_ratio_badge: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) badge_min_ratio: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) show_numbers: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) show_delta_totals: Option<bool>,
}

/// A config as the canonical serde shape — the inverse of [`resolve`], for
/// whoever writes one down (the settings file, a named preset).
#[must_use]
pub(crate) fn to_file(config: &FootprintConfig) -> FootprintFile {
    FootprintFile {
        style: Some(config.style.id().to_owned()),
        imbalance_ratio: config.imbalance_ratio.to_f64(),
        imbalance_min_qty: config.imbalance_min_qty.and_then(|qty| qty.to_f64()),
        stacked_count: Some(config.stacked_count),
        profile_row_px: Some(f64::from(config.profile_row_px)),
        detail_scale: Some(f64::from(config.detail_scale)),
        show_poc: Some(config.show_poc),
        extreme_ratio_badge: Some(config.extreme_ratio_badge),
        badge_min_ratio: config.badge_min_ratio.to_f64(),
        show_numbers: Some(config.show_numbers),
        show_delta_totals: Some(config.show_delta_totals),
    }
}

/// The persisted shape of the in-app edits: a version and the config in the
/// same vocabulary every other footprint file speaks.
#[derive(Debug, Serialize, Deserialize)]
struct SettingsFile {
    version: u32,
    config: FootprintFile,
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
    crate::store_home::resolve(SETTINGS_ENV, SETTINGS_FILE)
}

/// Parse a footprint-settings file, reporting why it is not one. The gate a
/// bundle section goes through — see [`crate::workspace_bundle`].
pub(crate) fn validate_settings(text: &str) -> Result<(), String> {
    let file: SettingsFile = toml::from_str(text).map_err(|error| error.to_string())?;
    if file.version == SETTINGS_VERSION {
        Ok(())
    } else {
        Err(format!(
            "footprint-settings format version {} (this build reads {SETTINGS_VERSION})",
            file.version
        ))
    }
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
        Ok(file) if file.version == SETTINGS_VERSION => resolve(file.config),
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
        config: to_file(config),
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
pub(crate) fn resolve(file: FootprintFile) -> FootprintConfig {
    let defaults = FootprintConfig::default();
    FootprintConfig {
        // An unknown style token (a file from a newer build) keeps the
        // default rather than failing the whole config.
        style: file
            .style
            .as_deref()
            .and_then(FootprintStyle::from_id)
            .unwrap_or(defaults.style),
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
        profile_row_px: file
            .profile_row_px
            .map(|px| (px as f32).clamp(*PROFILE_ROW_PX_RANGE.start(), *PROFILE_ROW_PX_RANGE.end()))
            .filter(|px| px.is_finite())
            .unwrap_or(defaults.profile_row_px),
        detail_scale: file
            .detail_scale
            .map(|scale| {
                (scale as f32).clamp(*DETAIL_SCALE_RANGE.start(), *DETAIL_SCALE_RANGE.end())
            })
            .filter(|scale| scale.is_finite())
            .unwrap_or(defaults.detail_scale),
        show_poc: file.show_poc.unwrap_or(defaults.show_poc),
        extreme_ratio_badge: file
            .extreme_ratio_badge
            .unwrap_or(defaults.extreme_ratio_badge),
        badge_min_ratio: file
            .badge_min_ratio
            .and_then(Decimal::from_f64)
            .filter(|ratio| *ratio >= Decimal::ONE)
            .unwrap_or(defaults.badge_min_ratio),
        show_numbers: file.show_numbers.unwrap_or(defaults.show_numbers),
        show_delta_totals: file.show_delta_totals.unwrap_or(defaults.show_delta_totals),
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

    /// The split look is the default; the ladder round-trips as a choice;
    /// an unknown style token from a newer build keeps the default.
    /// The numbers switch off and on through the file like every other
    /// knob, and an absent entry keeps them on.
    #[test]
    fn the_number_switches_round_trip_and_default_on() {
        assert!(FootprintConfig::default().show_numbers);
        assert!(FootprintConfig::default().show_delta_totals);
        let config =
            resolve(toml::from_str("show_numbers = false\nshow_delta_totals = false").unwrap());
        assert!(!config.show_numbers);
        assert!(!config.show_delta_totals);
    }

    #[test]
    fn style_defaults_to_split_and_round_trips() {
        assert_eq!(FootprintConfig::default().style, FootprintStyle::Split);
        let config = resolve(toml::from_str("style = \"ladder\"").unwrap());
        assert_eq!(config.style, FootprintStyle::Ladder);
        let config = resolve(toml::from_str("style = \"from_the_future\"").unwrap());
        assert_eq!(config.style, FootprintStyle::Split);
    }

    /// In-app edits round-trip through disk and win over the preset file's
    /// defaults on the next load.
    #[test]
    fn saved_settings_round_trip_and_overlay_the_preset() {
        let path = settings_path();
        let edited = FootprintConfig {
            style: FootprintStyle::Ladder,
            imbalance_ratio: Decimal::from(5),
            imbalance_min_qty: Some(Decimal::from(40)),
            stacked_count: 4,
            profile_row_px: 2.5,
            detail_scale: 1.4,
            show_poc: false,
            extreme_ratio_badge: false,
            badge_min_ratio: Decimal::from(3),
            show_numbers: false,
            show_delta_totals: false,
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
