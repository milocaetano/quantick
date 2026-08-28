//! `indicators-state.toml` — the active indicator set, persisted.
//!
//! Same discipline as the drawing presets store: a versioned TOML next to
//! the config (override with `QUANTICK_INDICATORS_STATE`), loaded once at
//! startup, written debounced after changes (add/remove/hide/inputs —
//! rare, event-driven work, never on the frame path). Scripts are *files*:
//! the state references them by library name, embedded built-ins by kind,
//! so the file survives script edits and never embeds stale source text.
//!
//! An unknown version or unreadable file starts empty and says so —
//! half-reading a state file would resurrect half a workspace.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use quantick_indicators::{InputValue, Rgba8, SourceId};

use crate::indicator_style::PlotOverride;

/// Environment override for the state file location.
pub(crate) const STATE_ENV: &str = "QUANTICK_INDICATORS_STATE";
/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub(crate) const STATE_FILE: &str = "indicators-state.toml";
/// Bumped on breaking layout changes; unknown versions start empty.
const FORMAT_VERSION: u32 = 1;

/// Which constructor an entry restores through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SavedKind {
    /// The native EMA.
    NativeEma,
    /// The native CVD pane.
    NativeCvd,
    /// A library script, by its menu name (`zigzag.pine`).
    Script {
        /// The library entry name.
        name: String,
    },
}

/// One persisted input value. Sources are stored by name so the file stays
/// hand-readable and survives enum reordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
pub(crate) enum SavedInput {
    /// `input.int`
    Int(i64),
    /// `input.float`
    Float(f64),
    /// `input.bool`
    Bool(bool),
    /// `input.color`, RGBA channels.
    Color([u8; 4]),
    /// `input.string`
    Str(String),
    /// `input.source`, by its dialect name.
    Source(String),
}

impl SavedInput {
    pub(crate) fn from_value(value: &InputValue) -> Self {
        match value {
            InputValue::Int(v) => SavedInput::Int(*v),
            InputValue::Float(v) => SavedInput::Float(*v),
            InputValue::Bool(v) => SavedInput::Bool(*v),
            InputValue::Color(c) => SavedInput::Color([c.r, c.g, c.b, c.a]),
            InputValue::Str(s) => SavedInput::Str(s.clone()),
            InputValue::Source(s) => SavedInput::Source(s.as_str().to_owned()),
        }
    }

    /// Back to a runtime value. An unknown source name yields `None` — the
    /// worker then falls back to that input's declared default, which is
    /// the honest recovery for a file edited by hand.
    pub(crate) fn to_value(&self) -> Option<InputValue> {
        Some(match self {
            SavedInput::Int(v) => InputValue::Int(*v),
            SavedInput::Float(v) => InputValue::Float(*v),
            SavedInput::Bool(v) => InputValue::Bool(*v),
            SavedInput::Color([r, g, b, a]) => InputValue::Color(Rgba8::new(*r, *g, *b, *a)),
            SavedInput::Str(s) => InputValue::Str(s.clone()),
            SavedInput::Source(name) => InputValue::Source(
                SourceId::ALL
                    .into_iter()
                    .find(|source| source.as_str() == name)?,
            ),
        })
    }
}

/// One plot's persisted style layer, in plot order.
///
/// Every field optional and skipped when absent, so a plot the trader never
/// touched costs an empty table and restores as "whatever the indicator
/// declares" — including after the indicator's author changes that
/// declaration. Storing the resolved colour instead would freeze today's
/// palette into every user's file the first time they opened the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub(crate) struct SavedPlotStyle {
    /// Draw this plot at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// RGBA channels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 4]>,
    /// Stroke width in pixels, rounded to [`WIDTH_DECIMALS`] decimals.
    ///
    /// `f64` and rounded, not the runtime's raw `f32`: serialising `2.3f32`
    /// writes `width = 2.299999952316284` into a file meant to be read and
    /// hand-edited, and every save then rewrites that noise. Two decimals is
    /// finer than a stroke width anyone can see and survives the trip exactly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
}

/// Decimals kept for a persisted stroke width. See [`SavedPlotStyle::width`].
const WIDTH_DECIMALS: f64 = 100.0;

impl SavedPlotStyle {
    pub(crate) fn from_override(over: PlotOverride) -> Self {
        Self {
            visible: over.visible,
            color: over.color.map(|c| [c.r, c.g, c.b, c.a]),
            width: over
                .width
                .map(|px| (f64::from(px) * WIDTH_DECIMALS).round() / WIDTH_DECIMALS),
        }
    }

    pub(crate) fn to_override(self) -> PlotOverride {
        PlotOverride {
            visible: self.visible,
            color: self.color.map(|[r, g, b, a]| Rgba8::new(r, g, b, a)),
            #[allow(clippy::cast_possible_truncation)]
            width: self.width.map(|px| px as f32),
        }
    }
}

/// One persisted indicator, in display order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SavedIndicator {
    /// How to reconstruct it.
    pub kind: SavedKind,
    /// Render-side eye toggle.
    #[serde(default)]
    pub hidden: bool,
    /// Bound input values, in declaration order.
    #[serde(default)]
    pub inputs: Vec<SavedInput>,
    /// Per-plot style overrides, in plot order.
    ///
    /// Added after the format shipped and therefore `default`: a file written
    /// before this existed loads with an empty layer and renders exactly as it
    /// used to, which is why this addition needs no version bump. Empty layers
    /// are skipped, so a workspace nobody styled writes the same file it
    /// always did.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plot_styles: Vec<SavedPlotStyle>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    #[serde(default)]
    indicators: Vec<SavedIndicator>,
}

/// The state file the app opens with and writes back to.
///
/// In the durable cockpit home rather than the launch directory — see
/// [`crate::store_home`] for why the indicator set used to vanish.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        return crate::store_home::test_path(STATE_FILE);
    }
    crate::store_home::resolve(STATE_ENV, STATE_FILE)
}

/// Parse an indicator-state file, reporting why it is not one. The gate a
/// bundle section goes through — see [`crate::workspace_bundle`].
pub(crate) fn validate(text: &str) -> Result<(), String> {
    let file: StateFile = toml::from_str(text).map_err(|error| error.to_string())?;
    if file.version == FORMAT_VERSION {
        Ok(())
    } else {
        Err(format!(
            "indicator-state format version {} (this build reads {FORMAT_VERSION})",
            file.version
        ))
    }
}

/// Load the saved set; empty when missing, unreadable or from an unknown
/// version (reported, never half-read).
#[must_use]
pub(crate) fn load(path: &std::path::Path) -> Vec<SavedIndicator> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    match toml::from_str::<StateFile>(&text) {
        Ok(file) if file.version == FORMAT_VERSION => file.indicators,
        Ok(file) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_STATE_VERSION",
                path = %path.display(),
                version = file.version,
                action = "starting_empty",
                "indicator state file is from an unknown version"
            );
            Vec::new()
        }
        Err(error) => {
            tracing::warn!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "INDICATOR_STATE_UNREADABLE",
                path = %path.display(),
                %error,
                action = "starting_empty",
                "indicator state file is unreadable"
            );
            Vec::new()
        }
    }
}

/// Write the saved set (the debounced end of the save path).
///
/// Test-only since layouts took over the file: the app reads it once, to
/// migrate, and never writes it again. Tests write it to prove the migration.
#[cfg(test)]
pub(crate) fn save(path: &std::path::Path, indicators: &[SavedIndicator]) {
    let file = StateFile {
        version: FORMAT_VERSION,
        indicators: indicators.to_vec(),
    };
    match toml::to_string_pretty(&file) {
        Ok(text) => {
            // Temp sibling + rename: `fs::write` truncates first, so a crash
            // or a power loss mid-write left a half file, and `load` then
            // reports it unreadable and starts empty — the whole workspace
            // gone rather than one stale entry.
            let temp = path.with_extension("toml.tmp");
            let written = std::fs::write(&temp, text).and_then(|()| std::fs::rename(&temp, path));
            if let Err(error) = written {
                let _ = std::fs::remove_file(&temp);
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_STATE_WRITE_FAILED",
                    path = %path.display(),
                    %error,
                    action = "state_not_saved",
                    "could not save the indicator state"
                );
            }
        }
        Err(error) => tracing::warn!(
            target: "quantick::app",
            schema_version = 1_u8,
            event_code = "INDICATOR_STATE_WRITE_FAILED",
            %error,
            action = "state_not_saved",
            "could not serialize the indicator state"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<SavedIndicator> {
        vec![
            SavedIndicator {
                kind: SavedKind::NativeEma,
                hidden: false,
                inputs: vec![SavedInput::Int(21), SavedInput::Source("delta".to_owned())],
                plot_styles: Vec::new(),
            },
            SavedIndicator {
                kind: SavedKind::Script {
                    name: "zigzag.pine".to_owned(),
                },
                hidden: true,
                inputs: vec![SavedInput::Int(5)],
                plot_styles: vec![SavedPlotStyle {
                    visible: Some(false),
                    color: Some([1, 2, 3, 255]),
                    width: Some(2.5),
                }],
            },
        ]
    }

    #[test]
    fn the_state_round_trips_through_disk() {
        let dir = std::env::temp_dir().join("quantick-indicator-state-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("indicators-state.toml");
        let saved = sample();
        save(&path, &saved);
        assert_eq!(load(&path), saved);
        std::fs::remove_file(&path).ok();
    }

    /// Criterion 4: the style layer was added to a format already on users'
    /// disks, so a file written before it existed has to load — with an empty
    /// layer, which renders exactly as it did — rather than be rejected as an
    /// unknown shape and take the whole workspace with it.
    #[test]
    fn a_file_written_before_styles_existed_still_loads() {
        let dir = std::env::temp_dir().join("quantick-indicator-state-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pre-style-state.toml");
        std::fs::write(
            &path,
            "version = 1\n\n[[indicators]]\nkind = \"native_ema\"\nhidden = false\n\n\
             [[indicators.inputs]]\ntype = \"int\"\nvalue = 21\n",
        )
        .unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 1, "the entry survived: {loaded:?}");
        assert_eq!(loaded[0].inputs, vec![SavedInput::Int(21)]);
        assert!(
            loaded[0].plot_styles.is_empty(),
            "no layer means the indicator's own declaration, unchanged"
        );
        std::fs::remove_file(&path).ok();
    }

    /// And nothing new is written for a workspace nobody styled: the file a
    /// user had before this change is the file they have after it.
    #[test]
    fn an_unstyled_workspace_writes_no_style_tables() {
        let dir = std::env::temp_dir().join("quantick-indicator-state-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("unstyled-state.toml");
        save(
            &path,
            &[SavedIndicator {
                kind: SavedKind::NativeEma,
                hidden: false,
                inputs: vec![SavedInput::Int(21)],
                plot_styles: Vec::new(),
            }],
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("plot_styles"), "written: {text}");
        std::fs::remove_file(&path).ok();
    }

    /// An override survives the disk in both directions, field by field, and a
    /// plot the trader left alone stays absent rather than being frozen at
    /// today's declared colour.
    #[test]
    fn plot_styles_round_trip_and_untouched_plots_stay_absent() {
        let over = PlotOverride {
            visible: Some(false),
            color: Some(Rgba8::new(9, 8, 7, 200)),
            width: Some(3.5),
        };
        let saved = SavedPlotStyle::from_override(over);
        assert_eq!(saved.to_override(), over);

        let untouched = SavedPlotStyle::from_override(PlotOverride::default());
        assert!(untouched.to_override().is_default());
        assert_eq!(
            toml::to_string(&untouched).unwrap().trim(),
            "",
            "an empty override serialises to nothing at all"
        );
    }

    /// A state file is meant to be read and hand-edited, and it is written
    /// back on every change — so a width that cannot survive the trip turns
    /// `2.3` into `2.299999952316284` and then rewrites that noise forever.
    /// The runtime keeps `f32`; what lands on disk is rounded.
    #[test]
    fn a_persisted_width_is_written_the_way_it_was_chosen() {
        for (chosen, expected) in [(2.3_f32, "2.3"), (1.1, "1.1"), (0.78, "0.78"), (4.0, "4.0")] {
            let saved = SavedPlotStyle::from_override(PlotOverride {
                width: Some(chosen),
                ..PlotOverride::default()
            });
            let text = toml::to_string(&saved).unwrap();
            assert_eq!(
                text.trim(),
                format!("width = {expected}"),
                "{chosen} must not land on disk as float noise"
            );
            let back = saved.to_override().width.expect("a width came back");
            assert!(
                (back - chosen).abs() < 0.005,
                "and it comes back as the width it was: {back} vs {chosen}"
            );
        }
    }

    #[test]
    fn unknown_versions_and_garbage_start_empty() {
        let dir = std::env::temp_dir().join("quantick-indicator-state-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad-state.toml");
        std::fs::write(&path, "version = 99\n").unwrap();
        assert!(load(&path).is_empty(), "unknown version starts empty");
        std::fs::write(&path, "not even toml [").unwrap();
        assert!(load(&path).is_empty(), "garbage starts empty");
        std::fs::remove_file(&path).ok();
        assert!(load(&dir.join("missing.toml")).is_empty());
    }

    #[test]
    fn inputs_round_trip_and_unknown_sources_fall_back() {
        let all = [
            InputValue::Int(9),
            InputValue::Float(1.5),
            InputValue::Bool(true),
            InputValue::Color(Rgba8::new(1, 2, 3, 4)),
            InputValue::Str("hi".to_owned()),
            InputValue::Source(SourceId::Cvd),
        ];
        for value in &all {
            let saved = SavedInput::from_value(value);
            assert_eq!(saved.to_value().as_ref(), Some(value));
        }
        assert_eq!(
            SavedInput::Source("no_such_series".to_owned()).to_value(),
            None,
            "hand-edited nonsense falls back to the declared default"
        );
    }
}
