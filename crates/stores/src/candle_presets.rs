//! Named chart appearances the trader saves and picks again, beside the
//! built-in candle presets.
//!
//! Same store discipline as the footprint presets: a versioned TOML (override
//! with `QUANTICK_CANDLE_PRESETS`), read once at startup, written on every
//! change, temp-file-and-rename, and a file that cannot be read is ignored
//! rather than costing the trader a chart.
//!
//! A built-in preset changes the candles only, so applying one never repaints
//! the trader's background. A saved preset is the whole look the trader saw
//! when they saved it — candles and canvas — because that is what "save
//! this" means; the two are told apart by name, and a custom name may not
//! shadow a built-in one.
//!
//! The document is a mirror of the style, not the style itself: the chart
//! crate stays free of serde, and a renamed field in memory cannot silently
//! change what the file means.

use std::collections::BTreeMap;
use std::path::Path;

use quantick_chart::style::{
    CandleBodyMode, CandlePreset, CandleStyle, CanvasStyle, ChartStyle, WickColorMode,
};
use serde::{Deserialize, Serialize};

/// The file's name inside the durable cockpit home.
pub const PRESETS_FILE: &str = "candle-presets.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;
/// Most saved looks one store keeps — a menu, not a database.
pub const MAX_PRESETS: usize = 24;
/// Longest a preset name may be.
pub const MAX_NAME_LEN: usize = 40;

/// Why a name was not saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveRefusal {
    /// Blank, or longer than [`MAX_NAME_LEN`].
    BadName,
    /// The name is a built-in preset's; one name, one meaning.
    BuiltInName,
    /// [`MAX_PRESETS`] already, and the name is new: nothing is evicted.
    Full,
}

impl SaveRefusal {
    /// The sentence a trader or an operator is shown.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::BadName => "a preset name must be 1 to 40 characters",
            Self::BuiltInName => "that name belongs to a built-in preset",
            Self::Full => "the preset list is full; delete one first",
        }
    }
}

/// What a preset name resolves to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Resolved {
    /// A built-in: the candles change, the canvas stays.
    BuiltIn(CandlePreset),
    /// A saved look: candles and canvas.
    Custom(ChartStyle),
}

impl Resolved {
    /// The style `current` becomes when this preset is applied to it.
    #[must_use]
    pub fn apply_to(self, current: &ChartStyle) -> ChartStyle {
        match self {
            Self::BuiltIn(preset) => ChartStyle {
                candles: preset.style(),
                canvas: current.canvas,
            },
            Self::Custom(style) => style,
        }
    }
}

/// The saved looks this run knows about.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CandlePresetStore {
    presets: BTreeMap<String, ChartStyle>,
}

impl CandlePresetStore {
    /// Load from `path`; empty when the file is missing, unreadable or from
    /// an unknown version.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str::<PresetsFile>(&text) {
            Ok(file) if file.version == FORMAT_VERSION => Self {
                presets: file
                    .presets
                    .into_iter()
                    .filter(|(name, _)| CandlePreset::from_name(name).is_none())
                    .take(MAX_PRESETS)
                    .map(|(name, style)| (name, style.into()))
                    .collect(),
            },
            _ => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "CANDLE_PRESETS_UNREADABLE",
                    path = %path.display(),
                    action = "starting_with_no_presets",
                    "candle presets file is unreadable or from an unknown version"
                );
                Self::default()
            }
        }
    }

    /// Every saved name, alphabetically.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.presets.keys().map(String::as_str)
    }

    /// The look saved under `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ChartStyle> {
        self.presets.get(name.trim())
    }

    /// A built-in preset by label or log value, else a saved one by name.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Option<Resolved> {
        CandlePreset::from_name(name)
            .map(Resolved::BuiltIn)
            .or_else(|| self.get(name).copied().map(Resolved::Custom))
    }

    /// The name the chart's current look answers to: a built-in whose candles
    /// match (canvas aside), else a saved look that matches whole.
    #[must_use]
    pub fn matching(&self, style: &ChartStyle) -> Option<&str> {
        CandlePreset::detect(&style.candles)
            .map(CandlePreset::label)
            .or_else(|| {
                self.presets
                    .iter()
                    .find(|(_, saved)| *saved == style)
                    .map(|(name, _)| name.as_str())
            })
    }

    /// Save `style` under `name`, replacing a look of that name. Returns the
    /// trimmed name it landed under.
    pub fn insert(&mut self, name: &str, style: ChartStyle) -> Result<String, SaveRefusal> {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return Err(SaveRefusal::BadName);
        }
        if CandlePreset::from_name(name).is_some() {
            return Err(SaveRefusal::BuiltInName);
        }
        if self.presets.len() >= MAX_PRESETS && !self.presets.contains_key(name) {
            return Err(SaveRefusal::Full);
        }
        self.presets.insert(name.to_owned(), style);
        Ok(name.to_owned())
    }

    /// Forget `name`; answers whether there was one.
    pub fn remove(&mut self, name: &str) -> bool {
        self.presets.remove(name.trim()).is_some()
    }

    /// Write the store. See the [module docs](self) for the discipline.
    pub fn save(&self, path: &Path) {
        if quantick_workspace::write_refusal::guard_write(path).is_err() {
            return;
        }
        let file = PresetsFile {
            version: FORMAT_VERSION,
            presets: self
                .presets
                .iter()
                .map(|(name, style)| (name.clone(), StyleFile::from(style)))
                .collect(),
        };
        let Ok(text) = toml::to_string_pretty(&file) else {
            return;
        };
        let temp = path.with_extension("toml.tmp");
        if let Err(error) = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path))
        {
            let _ = std::fs::remove_file(&temp);
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "CANDLE_PRESETS_WRITE_FAILED",
                path = %path.display(),
                %error,
                action = "presets_not_saved",
                "could not save the candle presets"
            );
        }
    }
}

/// Parse a candle-presets file, reporting why it is not one — the gate a
/// bundle section goes through.
pub fn validate(text: &str) -> Result<(), String> {
    let file: PresetsFile = toml::from_str(text).map_err(|error| error.to_string())?;
    if file.version == FORMAT_VERSION {
        Ok(())
    } else {
        Err(format!(
            "candle-presets format version {} (this build reads {FORMAT_VERSION})",
            file.version
        ))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct PresetsFile {
    version: u32,
    #[serde(default)]
    presets: BTreeMap<String, StyleFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StyleFile {
    candles: CandlesFile,
    canvas: CanvasFile,
}

#[derive(Debug, Serialize, Deserialize)]
struct CandlesFile {
    outline_only: bool,
    bull_fill: [u8; 3],
    bear_fill: [u8; 3],
    bull_outline: [u8; 3],
    bear_outline: [u8; 3],
    fill_opacity: f32,
    outline_opacity: f32,
    outline_width: f32,
    show_wicks: bool,
    custom_wick_color: bool,
    wick: [u8; 3],
    wick_opacity: f32,
    wick_width: f32,
    body_width_frac: f32,
    gap_px: f32,
    corner_radius: f32,
    min_body_height: f32,
    forming_opacity: f32,
}

#[derive(Debug, Serialize, Deserialize)]
struct CanvasFile {
    background_enabled: bool,
    background: [u8; 3],
    grid_enabled: bool,
    grid: [u8; 3],
    grid_opacity: f32,
}

impl From<&ChartStyle> for StyleFile {
    fn from(style: &ChartStyle) -> Self {
        let c = &style.candles;
        let v = &style.canvas;
        Self {
            candles: CandlesFile {
                outline_only: c.body_mode == CandleBodyMode::OutlineOnly,
                bull_fill: c.bull_fill,
                bear_fill: c.bear_fill,
                bull_outline: c.bull_outline,
                bear_outline: c.bear_outline,
                fill_opacity: c.fill_opacity,
                outline_opacity: c.outline_opacity,
                outline_width: c.outline_width,
                show_wicks: c.show_wicks,
                custom_wick_color: c.wick_color_mode == WickColorMode::Custom,
                wick: c.wick,
                wick_opacity: c.wick_opacity,
                wick_width: c.wick_width,
                body_width_frac: c.body_width_frac,
                gap_px: c.gap_px,
                corner_radius: c.corner_radius,
                min_body_height: c.min_body_height,
                forming_opacity: c.forming_opacity,
            },
            canvas: CanvasFile {
                background_enabled: v.background_enabled,
                background: v.background,
                grid_enabled: v.grid_enabled,
                grid: v.grid,
                grid_opacity: v.grid_opacity,
            },
        }
    }
}

impl From<StyleFile> for ChartStyle {
    fn from(file: StyleFile) -> Self {
        let c = file.candles;
        let v = file.canvas;
        Self {
            candles: CandleStyle {
                body_mode: if c.outline_only {
                    CandleBodyMode::OutlineOnly
                } else {
                    CandleBodyMode::Filled
                },
                bull_fill: c.bull_fill,
                bear_fill: c.bear_fill,
                bull_outline: c.bull_outline,
                bear_outline: c.bear_outline,
                fill_opacity: c.fill_opacity,
                outline_opacity: c.outline_opacity,
                outline_width: c.outline_width,
                show_wicks: c.show_wicks,
                wick_color_mode: if c.custom_wick_color {
                    WickColorMode::Custom
                } else {
                    WickColorMode::MatchOutline
                },
                wick: c.wick,
                wick_opacity: c.wick_opacity,
                wick_width: c.wick_width,
                body_width_frac: c.body_width_frac,
                gap_px: c.gap_px,
                corner_radius: c.corner_radius,
                min_body_height: c.min_body_height,
                forming_opacity: c.forming_opacity,
            },
            canvas: CanvasStyle {
                background_enabled: v.background_enabled,
                background: v.background,
                grid_enabled: v.grid_enabled,
                grid: v.grid,
                grid_opacity: v.grid_opacity,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> std::path::PathBuf {
        crate::scratch::thread_dir("candle-presets").join(PRESETS_FILE)
    }

    fn look() -> ChartStyle {
        let mut style = ChartStyle {
            candles: CandlePreset::OutlineOnly.style(),
            ..ChartStyle::default()
        };
        style.candles.wick_color_mode = WickColorMode::Custom;
        style.candles.gap_px = 3.5;
        style.canvas.grid_enabled = false;
        style.canvas.background = [1, 2, 3];
        style
    }

    #[test]
    fn a_saved_look_round_trips_through_disk_whole() {
        let path = path();
        let mut store = CandlePresetStore::default();
        assert_eq!(store.insert("  night  ", look()), Ok("night".to_owned()));
        store
            .insert("day", ChartStyle::default())
            .expect("a fresh name");
        store.save(&path);

        let loaded = CandlePresetStore::load(&path);
        assert_eq!(loaded, store);
        assert_eq!(loaded.names().collect::<Vec<_>>(), ["day", "night"]);
        assert_eq!(loaded.get("night"), Some(&look()));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_or_broken_file_costs_no_chart() {
        let path = path();
        std::fs::remove_file(&path).ok();
        assert_eq!(CandlePresetStore::load(&path).names().count(), 0);
        std::fs::write(&path, "not toml [").unwrap();
        assert_eq!(CandlePresetStore::load(&path).names().count(), 0);
        std::fs::write(&path, "version = 99\n[presets]\n").unwrap();
        assert_eq!(CandlePresetStore::load(&path).names().count(), 0);
        assert!(validate("version = 99\n").is_err());
        assert!(validate("version = 1\n").is_ok());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn names_are_bounded_unshadowed_and_never_evict() {
        let mut store = CandlePresetStore::default();
        assert_eq!(store.insert("  ", look()), Err(SaveRefusal::BadName));
        assert_eq!(
            store.insert(&"x".repeat(MAX_NAME_LEN + 1), look()),
            Err(SaveRefusal::BadName)
        );
        assert_eq!(store.insert("glass", look()), Err(SaveRefusal::BuiltInName));
        for index in 0..MAX_PRESETS {
            store.insert(&format!("p{index}"), look()).unwrap();
        }
        assert_eq!(store.insert("one more", look()), Err(SaveRefusal::Full));
        assert!(
            store.insert("p0", ChartStyle::default()).is_ok(),
            "overwrite"
        );
        assert!(store.remove("p0"));
        assert!(!store.remove("p0"));
    }

    #[test]
    fn a_built_in_keeps_the_canvas_and_a_saved_look_brings_its_own() {
        let mut store = CandlePresetStore::default();
        store.insert("night", look()).unwrap();
        let current = ChartStyle::default();
        let classic = store.resolve("Classic").unwrap().apply_to(&look());
        assert_eq!(classic.candles, CandlePreset::Classic.style());
        assert_eq!(classic.canvas, look().canvas, "the canvas stayed");
        assert_eq!(store.resolve("night").unwrap().apply_to(&current), look());
        assert_eq!(store.resolve("nobody"), None);
        assert_eq!(store.matching(&look()), Some("night"));
        assert_eq!(store.matching(&current), Some("Order flow"));
        let mut own = look();
        own.candles.gap_px = 5.0;
        store.insert("own", own).unwrap();
        assert_eq!(store.matching(&own), Some("own"));
    }
}
