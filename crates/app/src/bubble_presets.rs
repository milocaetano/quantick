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

use crate::orderflow::{
    BubbleStyle, HeatmapConfig, LiveLaneStyle,
    config::{DEFAULT_BUBBLE_CLUSTER_MS, DEFAULT_BUBBLE_DUST_MERGE_MS},
};

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

const fn default_dust_merge_ms() -> i64 {
    DEFAULT_BUBBLE_DUST_MERGE_MS
}

/// One named snapshot of the aggression-bubble panel's *appearance*.
///
/// Deliberately not a switch: whether the layer draws at all stays a live
/// decision in the UI, so opening the chart with a presets file present can
/// never turn capture on by itself.
///
/// Field order matters: TOML requires plain values before tables, so the two
/// tables — [`bubbles`](Self::bubbles) and [`live_lane`](Self::live_lane) —
/// stay at the end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BubblePreset {
    /// Unique, human-chosen name shown in the picker.
    pub name: String,
    /// Temporal window used to cluster compatible prints, in milliseconds.
    #[serde(default = "default_cluster_ms")]
    pub cluster_ms: i64,
    /// Window over which prints too small to read fold together, in
    /// milliseconds. Part of the look: how crowded the tape is allowed to get.
    #[serde(default = "default_dust_merge_ms")]
    pub dust_merge_ms: i64,
    /// Whether closed bars are folded into one two-sided bubble per price
    /// range. As much a look as the radius range: it decides whether history
    /// reads as a tape or as a summary.
    #[serde(default)]
    pub candle_summary: bool,
    /// Every visual choice for the bubbles themselves.
    #[serde(default)]
    pub bubbles: BubbleStyle,
    /// The live lane's own width, clustering and radius scale.
    #[serde(default)]
    pub live_lane: LiveLaneStyle,
}

impl BubblePreset {
    /// Capture the current panel state under `name`.
    #[must_use]
    pub fn capture(name: impl Into<String>, config: &HeatmapConfig) -> Self {
        Self {
            name: name.into(),
            cluster_ms: config.bubble_cluster_ms,
            dust_merge_ms: config.bubble_dust_merge_ms,
            candle_summary: config.bubble_candle_summary,
            bubbles: config.bubbles.clone(),
            live_lane: config.live_lane.clone(),
        }
    }

    /// Apply this preset over `config`, touching nothing outside the panel —
    /// and never the layer's own switch.
    pub fn apply_to(&self, config: &mut HeatmapConfig) {
        config.bubble_cluster_ms = self.cluster_ms;
        config.bubble_dust_merge_ms = self.dust_merge_ms;
        config.bubble_candle_summary = self.candle_summary;
        config.bubbles = self.bubbles.clone();
        config.live_lane = self.live_lane.clone();
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
            preset.live_lane.sanitize();
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

/// Keys a presets file may still carry from before a setting was replaced,
/// each paired with what took the job over.
///
/// Serde ignoring an unknown key is what lets an old file keep loading, and
/// that is the behaviour we want — but *silently* is not the same as
/// *honestly*. Someone who wrote `show_consumption_front` asked for a specific
/// mark by name, and the chart now draws a different one. Saying so once at
/// load is the difference between a documented default change and what reads
/// from the outside like the setting broke.
const RETIRED_KEYS: [(&str, &str); 1] = [("show_consumption_front", "consumption_mark")];

/// Retired keys actually *set* by this presets text, as
/// `(retired, replacement)`.
///
/// A line scan rather than a second parse: this runs once at startup, and the
/// shape being matched — a key at the start of a line followed by `=` — is
/// exactly what TOML assignment looks like. It deliberately does not match a
/// mention inside a comment, because the shipped file documents the migration
/// by name and would otherwise warn about itself on every launch.
#[must_use]
pub fn retired_keys(text: &str) -> Vec<(&'static str, &'static str)> {
    let assigns = |key: &str| {
        text.lines().any(|line| {
            let statement = line.split('#').next().unwrap_or_default().trim();
            statement
                .strip_prefix(key)
                .is_some_and(|rest| rest.trim_start().starts_with('='))
        })
    };
    RETIRED_KEYS
        .into_iter()
        .filter(|(retired, _)| assigns(retired))
        .collect()
}

/// Log every retired key a presets file still carries.
fn report_retired_keys(text: &str, path: &Path) {
    for (retired, replacement) in retired_keys(text) {
        tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "BUBBLE_PRESET_KEY_RETIRED",
            file = %path.display(),
            retired,
            replacement,
            action = "using_replacement_default",
            "a bubble preset still sets a retired key; the setting that replaced it \
             is at its default"
        );
    }
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
            Ok(file) => {
                report_retired_keys(&text, &path);
                (file, source(path), None)
            }
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
    use crate::orderflow::{BubbleSizeReference, ConsumptionMark, INV_PHI};

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
    fn the_shipped_default_preset_is_the_code_default() {
        // The file a new user reads and the behaviour a fresh install gets are
        // the same thing said twice, so they are asserted to agree. Without
        // this, changing one default in code silently leaves the published
        // preset describing a chart the app no longer draws.
        let shipped = embedded();
        assert_eq!(
            shipped.active, "default",
            "a fresh install opens on the shipped default"
        );
        let preset = shipped
            .get("default")
            .expect("the shipped file carries a preset named 'default'");
        assert_eq!(preset.bubbles, BubbleStyle::default());
    }

    #[test]
    fn a_retired_key_is_named_rather_than_ignored_in_silence() {
        // The file still loads — that is the point of ignoring unknown keys —
        // but a preset that asked for the vertical front by name now draws a
        // crown, and nothing else in the app would ever say so.
        let legacy = "
            active = \"mine\"
            [[presets]]
            name = \"mine\"
            [presets.bubbles]
            show_consumption_front = true
        ";
        assert_eq!(
            retired_keys(legacy),
            vec![("show_consumption_front", "consumption_mark")]
        );
        let parsed = parse(legacy).expect("a legacy file still parses");
        assert_eq!(
            parsed.get("mine").expect("preset").bubbles.consumption_mark,
            ConsumptionMark::Crown
        );

        // And a file written today says nothing, so the warning stays rare
        // enough to mean something — even though the shipped file documents
        // the migration by name in a comment, which must not count as setting
        // it.
        assert!(retired_keys(EMBEDDED_DEFAULT).is_empty());
        assert!(
            EMBEDDED_DEFAULT.contains("show_consumption_front"),
            "the shipped file explains the migration, so this test is really \
             asserting that a comment is not an assignment"
        );
        assert!(retired_keys("# show_consumption_front = true").is_empty());
    }

    #[test]
    fn the_shipped_default_aggregates_before_it_draws() {
        // The calm the visual language buys is spent immediately if every
        // print still gets its own mark. These three windows are what keep the
        // count down, and they were once lost by moving `active` to a preset
        // that inherited the bare code defaults — the layer stayed correct and
        // became unreadable. Pinned so that cannot happen quietly again.
        let preset = embedded()
            .get("default")
            .cloned()
            .expect("the shipped default");
        assert!(
            preset.cluster_ms >= 500,
            "compatible prints inside half a second must become one bubble, not {}",
            preset.cluster_ms
        );
        assert!(
            preset.dust_merge_ms > 0,
            "prints too small to read must fold into one bubble per price range"
        );
        assert!(
            preset.candle_summary,
            "a closed bar must summarize into pies; without it history is a wall of specks"
        );
    }

    #[test]
    fn every_shipped_preset_keeps_its_radii_on_the_golden_ladder() {
        for preset in embedded().presets {
            let bubbles = &preset.bubbles;
            let name = &preset.name;
            assert!(
                bubbles.min_radius < bubbles.detail_min_radius
                    && bubbles.detail_min_radius < bubbles.readable_min_radius
                    && bubbles.readable_min_radius < bubbles.max_radius,
                "'{name}' has rungs out of order: {} {} {} {}",
                bubbles.min_radius,
                bubbles.detail_min_radius,
                bubbles.readable_min_radius,
                bubbles.max_radius
            );
            // One ladder, every preset, whatever its radius range. The
            // readability floor hangs two steps under the top — the rung
            // between them, `max/φ`, is where the halo switches on and is not
            // a stored field — and each rung below is the one above it over φ.
            for (label, expected, actual) in [
                (
                    "readable",
                    bubbles.max_radius * INV_PHI * INV_PHI,
                    bubbles.readable_min_radius,
                ),
                (
                    "detail",
                    bubbles.readable_min_radius * INV_PHI,
                    bubbles.detail_min_radius,
                ),
                (
                    "min",
                    bubbles.detail_min_radius * INV_PHI,
                    bubbles.min_radius,
                ),
            ] {
                assert!(
                    (actual - expected).abs() <= 0.001,
                    "'{name}' {label} rung {actual} is not {expected}"
                );
            }
        }
    }

    #[test]
    fn no_shipped_preset_smears_the_book_or_hollows_a_print() {
        for preset in embedded().presets {
            let bubbles = &preset.bubbles;
            let name = &preset.name;
            assert!(
                !bubbles.hollow_small_buys,
                "'{name}' still draws small buys as an open ring"
            );
            // One preset is allowed a trail, and it is the one named for the
            // marks: a slow tape where the question is which levels got eaten.
            if bubbles.trail_length > 0.0 {
                assert_eq!(
                    name, "consumption focus",
                    "'{name}' ships a trail; only the consumption preset may"
                );
            }
            // Nothing ships a near-black consumption colour: over this canvas
            // that mark is invisible, and over the heat map it is a hole.
            if let Some([r, g, b]) = bubbles.front_color {
                assert!(
                    u16::from(r) + u16::from(g) + u16::from(b) > 180,
                    "'{name}' paints its consumption mark almost black: {r},{g},{b}"
                );
            }
        }
    }

    #[test]
    fn the_embedded_fallback_matches_the_tracked_presets_file() {
        // The panel reads and writes `config/bubbles.toml` at the repository
        // root; the embedded copy is what a user gets running from anywhere
        // else. They drifted apart once — the tracked file gained a preset the
        // binary never saw — so the sync is asserted, not assumed. Comments
        // may differ between the two files; meaning may not.
        let tracked = include_str!("../../../config/bubbles.toml");
        assert_eq!(
            parse(tracked).expect("tracked presets file"),
            parse(EMBEDDED_DEFAULT).expect("embedded presets"),
        );
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
            dust_merge_ms: DEFAULT_BUBBLE_DUST_MERGE_MS,
            candle_summary: false,
            bubbles: BubbleStyle {
                front_color: Some([255, 246, 205]),
                ..BubbleStyle::default()
            },
            live_lane: LiveLaneStyle::default(),
        });
        file.active = "default".to_owned();

        let text = render(&file).expect("render");
        assert!(
            text.starts_with(PRESET_FILE_HEADER),
            "the explanatory header survives a save:\n{text}"
        );
        assert!(text.contains("opacity = 0.9"), "floats stay short:\n{text}");
        assert!(
            text.contains("front_length_scale = 0.618"),
            "floats stay short:\n{text}"
        );
        assert!(
            !text.contains("0.61803398"),
            "no binary expansions:\n{text}"
        );
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
