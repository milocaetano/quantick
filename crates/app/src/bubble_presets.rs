//! Named, versioned presets for the aggression-bubble panel.
//!
//! A preset is the "aggression bubbles" panel captured under a name: how prints
//! are clustered and every visual choice in [`BubbleStyle`]. They live in
//! [`PRESETS_PATH`] — `config/bubbles.toml`, a tracked file next to the rest of
//! the project's configuration — so a look that works on the mini index is
//! shared, reviewed and rolled back like code. See `config/README.md`.
//!
//! Resolution order mirrors [`crate::config`]: the [`PRESETS_ENV`] path, then
//! [`PRESETS_PATH`] relative to the working directory, then the built-in file
//! embedded at compile time.
//!
//! Unlike the feed configuration, a broken presets file is **not** fatal: the
//! chart keeps running on the embedded presets and the parse error is returned
//! to the caller, which shows it in the panel and logs it. Losing the tape
//! because a colour tuple has three commas would be the worse failure — but the
//! error is never swallowed either.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::orderflow::{BubbleStyle, HeatmapConfig, config::DEFAULT_BUBBLE_CLUSTER_MS};

/// Environment variable naming an explicit presets file.
pub const PRESETS_ENV: &str = "QUANTICK_BUBBLES";

/// Tracked presets file, read and written relative to the working directory.
///
/// Under `config/` rather than the repository root: the name alone should say
/// what the file is, and project configuration belongs in one obvious place.
pub const PRESETS_PATH: &str = "config/bubbles.toml";

/// Built-in presets, compiled in so the app works with no external file.
const EMBEDDED_DEFAULT: &str = include_str!("../config/bubbles.toml");

const fn default_cluster_ms() -> i64 {
    DEFAULT_BUBBLE_CLUSTER_MS
}

/// One named snapshot of the aggression-bubble panel's *appearance*.
///
/// Deliberately not a switch: whether the layer draws at all stays a live
/// decision in the UI, so opening the chart with a presets file present can
/// never turn capture on by itself.
///
/// Field order matters: TOML requires plain values before tables, so
/// [`bubbles`](Self::bubbles) stays last.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BubblePreset {
    /// Unique, human-chosen name shown in the picker.
    pub name: String,
    /// Temporal window used to cluster compatible prints, in milliseconds.
    #[serde(default = "default_cluster_ms")]
    pub cluster_ms: i64,
    /// Every visual choice for the bubbles themselves.
    #[serde(default)]
    pub bubbles: BubbleStyle,
}

impl BubblePreset {
    /// Capture the current panel state under `name`.
    #[must_use]
    pub fn capture(name: impl Into<String>, config: &HeatmapConfig) -> Self {
        Self {
            name: name.into(),
            cluster_ms: config.bubble_cluster_ms,
            bubbles: config.bubbles.clone(),
        }
    }

    /// Apply this preset over `config`, touching nothing outside the panel —
    /// and never the layer's own switch.
    pub fn apply_to(&self, config: &mut HeatmapConfig) {
        config.bubble_cluster_ms = self.cluster_ms;
        config.bubbles = self.bubbles.clone();
    }
}

/// The presets file as a whole.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct BubblePresetFile {
    /// Name of the preset the panel opens on. Empty means "none".
    #[serde(default)]
    pub active: String,
    /// Every stored preset, in file order.
    #[serde(default)]
    pub presets: Vec<BubblePreset>,
}

impl BubblePresetFile {
    /// The preset stored under `name`, if any.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&BubblePreset> {
        self.presets.iter().find(|preset| preset.name == name)
    }

    /// Store `preset`, replacing any existing one with the same name.
    pub fn upsert(&mut self, preset: BubblePreset) {
        match self
            .presets
            .iter_mut()
            .find(|stored| stored.name == preset.name)
        {
            Some(stored) => *stored = preset,
            None => self.presets.push(preset),
        }
    }

    /// Remove `name`, reporting whether it existed.
    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.presets.len();
        self.presets.retain(|preset| preset.name != name);
        if self.active == name {
            self.active.clear();
        }
        self.presets.len() != before
    }

    /// Drop unusable entries and clamp every stored style.
    ///
    /// Nameless or duplicate presets are the only things discarded; a preset
    /// with out-of-range numbers is kept and clamped, so hand-editing the file
    /// never costs the whole entry.
    fn sanitize(&mut self) {
        let mut seen: Vec<String> = Vec::with_capacity(self.presets.len());
        self.presets.retain_mut(|preset| {
            preset.name = preset.name.trim().to_owned();
            if preset.name.is_empty() || seen.contains(&preset.name) {
                return false;
            }
            seen.push(preset.name.clone());
            preset.cluster_ms = preset
                .cluster_ms
                .clamp(0, crate::orderflow::config::MAX_BUBBLE_CLUSTER_MS);
            preset.bubbles.sanitize();
            true
        });
        let active = std::mem::take(&mut self.active).trim().to_owned();
        if self.get(&active).is_some() {
            self.active = active;
        }
    }
}

/// Where a loaded presets file came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetSource {
    /// An explicit path from [`PRESETS_ENV`].
    EnvPath(PathBuf),
    /// [`PRESETS_PATH`] relative to the working directory.
    WorkingDir(PathBuf),
    /// The built-in presets embedded in the binary.
    Embedded,
}

impl std::fmt::Display for PresetSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EnvPath(path) | Self::WorkingDir(path) => write!(f, "{}", path.display()),
            Self::Embedded => write!(f, "<built-in>"),
        }
    }
}

/// Parse presets from TOML text, sanitizing what survives.
///
/// # Errors
///
/// Returns the parser's message when the text is not a valid presets file.
pub fn parse(text: &str) -> Result<BubblePresetFile, String> {
    let mut file: BubblePresetFile = toml::from_str(text).map_err(|error| error.to_string())?;
    file.sanitize();
    Ok(file)
}

/// The built-in presets. Validated by a test, so this never fails in practice.
#[must_use]
pub fn embedded() -> BubblePresetFile {
    parse(EMBEDDED_DEFAULT).unwrap_or_default()
}

/// Path the panel reads from and writes to.
#[must_use]
pub fn presets_path() -> PathBuf {
    std::env::var_os(PRESETS_ENV).map_or_else(|| PathBuf::from(PRESETS_PATH), PathBuf::from)
}

/// Load presets, falling back to the embedded file.
///
/// Returns the presets, where they came from, and — when an external file
/// exists but could not be read or parsed — the error to surface. In that case
/// the returned presets are the embedded ones, and the source says so.
#[must_use]
pub fn load() -> (BubblePresetFile, PresetSource, Option<String>) {
    let (path, source): (PathBuf, fn(PathBuf) -> PresetSource) = match std::env::var_os(PRESETS_ENV)
    {
        Some(raw) => (PathBuf::from(raw), PresetSource::EnvPath),
        None => (PathBuf::from(PRESETS_PATH), PresetSource::WorkingDir),
    };
    if !Path::new(&path).is_file() {
        return (embedded(), PresetSource::Embedded, None);
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => match parse(&text) {
            Ok(file) => (file, source(path), None),
            Err(message) => (
                embedded(),
                PresetSource::Embedded,
                Some(format!("{}: {message}", path.display())),
            ),
        },
        Err(error) => (
            embedded(),
            PresetSource::Embedded,
            Some(format!("cannot read {}: {error}", path.display())),
        ),
    }
}

/// Write `file` to [`presets_path`], returning where it landed.
///
/// Creates the parent directory when missing, so saving works in a fresh
/// checkout or when the app runs from somewhere without a `config/` yet.
///
/// # Errors
///
/// Returns a human-readable message when the file cannot be serialized or
/// written.
pub fn save(file: &BubblePresetFile) -> Result<PathBuf, String> {
    let path = presets_path();
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let text = render(file)?;
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(path)
}

/// Render the presets file as the exact text a save writes.
///
/// Split out of [`save`] so the bytes that land in a tracked file can be
/// asserted in a test without touching the filesystem: this file only earns
/// being in git if a save produces a diff a human can read.
fn render(file: &BubblePresetFile) -> Result<String, String> {
    let body = toml::to_string(file).map_err(|error| error.to_string())?;
    Ok(format!("{PRESET_FILE_HEADER}{body}"))
}

const PRESET_FILE_HEADER: &str = "\
# quantick — aggression bubble presets.
#
# Written by the \"aggression bubbles\" panel (save), and safe to edit by hand:
# this file is the versioned record of how the tape is read. See README.md in
# this folder.
# `active` names the preset the panel opens on. Colour overrides are optional
# `[r, g, b]` triples; leave them out to follow the chart theme.

";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orderflow::BubbleSizeReference;

    #[test]
    fn the_embedded_file_parses_and_carries_a_valid_active_preset() {
        let file = parse(EMBEDDED_DEFAULT).expect("embedded presets");
        assert!(!file.presets.is_empty());
        assert!(
            file.get(&file.active).is_some(),
            "active preset '{}' must exist",
            file.active
        );
        for preset in &file.presets {
            assert!(preset.bubbles.max_radius >= preset.bubbles.min_radius);
        }
    }

    #[test]
    fn a_preset_round_trips_through_the_panel_state() {
        let mut config = HeatmapConfig {
            bubble_cluster_ms: 50,
            bubbles: BubbleStyle {
                side_offset: 9.0,
                size_reference: BubbleSizeReference::VisibleMax,
                sell_color: Some([10, 20, 30]),
                ..BubbleStyle::default()
            },
            ..HeatmapConfig::default()
        };
        let preset = BubblePreset::capture("mine", &config);

        let mut other = HeatmapConfig::default();
        preset.apply_to(&mut other);
        assert_eq!(other.bubbles, config.bubbles);
        assert_eq!(other.bubble_cluster_ms, 50);
        // Nothing outside the bubble panel moves — least of all the switch that
        // would start recording trades.
        assert_eq!(
            other.show_aggressions,
            HeatmapConfig::default().show_aggressions
        );
        assert_eq!(other.retention_ms, HeatmapConfig::default().retention_ms);
        assert_eq!(other.gamma, HeatmapConfig::default().gamma);

        // And a full round trip through the real write path — the one `save`
        // uses — preserves it byte-for-byte in meaning.
        let mut file = BubblePresetFile::default();
        file.upsert(preset.clone());
        file.active = "mine".to_owned();
        let text = render(&file).expect("render");
        assert_eq!(parse(&text).expect("reparse"), file);

        config.bubbles.side_offset = 0.0;
        assert_ne!(BubblePreset::capture("mine", &config), preset);
    }

    #[test]
    fn upsert_replaces_by_name_and_remove_clears_the_active_selection() {
        let mut file = BubblePresetFile::default();
        file.upsert(BubblePreset::capture("a", &HeatmapConfig::default()));
        file.upsert(BubblePreset::capture("b", &HeatmapConfig::default()));
        let mut louder = HeatmapConfig::default();
        louder.bubbles.max_radius = 40.0;
        file.upsert(BubblePreset::capture("a", &louder));
        file.active = "a".to_owned();

        assert_eq!(
            file.presets.len(),
            2,
            "same name replaces, never duplicates"
        );
        assert_eq!(file.get("a").unwrap().bubbles.max_radius, 40.0);
        assert!(file.remove("a"));
        assert!(!file.remove("a"));
        assert!(file.active.is_empty());
    }

    #[test]
    fn sanitizing_drops_nameless_and_duplicate_entries_but_clamps_the_rest() {
        let text = r#"
            active = "ghost"

            [[presets]]
            name = "  "

            [[presets]]
            name = "loud"
            cluster_ms = 999999
            [presets.bubbles]
            max_radius = 5000.0

            [[presets]]
            name = "loud"
            [presets.bubbles]
            max_radius = 8.0
        "#;
        let file = parse(text).expect("parse");
        assert_eq!(file.presets.len(), 1);
        assert_eq!(file.presets[0].name, "loud");
        assert_eq!(
            file.presets[0].bubbles.max_radius,
            crate::orderflow::MAX_BUBBLE_MAX_RADIUS
        );
        assert!(file.presets[0].cluster_ms <= crate::orderflow::config::MAX_BUBBLE_CLUSTER_MS);
        assert!(
            file.active.is_empty(),
            "an active name that resolves to nothing is cleared"
        );
    }

    #[test]
    fn a_save_writes_a_file_a_human_can_review() {
        // The file is tracked in git so a look that reads the tape well can be
        // reviewed and rolled back like code — which only holds if a save
        // produces a readable diff. TOML floats are f64, so an f32 written
        // straight through prints its binary expansion (0.78 becomes
        // 0.7799999713897705) and every save turns into noise.
        let mut file = BubblePresetFile::default();
        file.upsert(BubblePreset {
            name: "default".to_owned(),
            cluster_ms: DEFAULT_BUBBLE_CLUSTER_MS,
            bubbles: BubbleStyle {
                front_color: Some([255, 246, 205]),
                ..BubbleStyle::default()
            },
        });
        file.active = "default".to_owned();

        let text = render(&file).expect("render");
        assert!(
            text.starts_with(PRESET_FILE_HEADER),
            "the explanatory header survives a save:\n{text}"
        );
        assert!(
            text.contains("opacity = 0.78"),
            "floats stay short:\n{text}"
        );
        assert!(
            text.contains("front_length_scale = 2.1"),
            "floats stay short:\n{text}"
        );
        assert!(!text.contains("0.7799"), "no binary expansions:\n{text}");
        assert!(
            text.contains("front_color = [255, 246, 205]"),
            "colour triples stay on one line, as config/README.md documents:\n{text}"
        );
        assert_eq!(
            parse(&text).expect("reparse"),
            file,
            "and it still means exactly what it meant"
        );
    }

    #[test]
    fn a_malformed_file_is_reported_rather_than_parsed() {
        let error = parse("presets = [").expect_err("malformed");
        assert!(!error.is_empty());
    }
}
