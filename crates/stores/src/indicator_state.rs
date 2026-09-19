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

use serde::{Deserialize, Serialize};

use quantick_indicators::{InputValue, Rgba8, SourceId};

use quantick_chart::indicator_style::PlotOverride;

/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub const STATE_FILE: &str = "indicators-state.toml";
/// Bumped on breaking layout changes; unknown versions start empty.
const FORMAT_VERSION: u32 = 1;

pub use quantick_workspace::indicator_document::{
    SavedIndicator, SavedInput, SavedKind, SavedPlotStyle,
};

pub trait SavedInputExt {
    fn from_value(value: &InputValue) -> Self;
    fn to_value(&self) -> Option<InputValue>;
}
pub trait SavedPlotStyleExt {
    fn from_override(over: PlotOverride) -> Self;
    fn to_override(self) -> PlotOverride;
}
const WIDTH_DECIMALS: f64 = 100.0;
impl SavedInputExt for SavedInput {
    fn from_value(value: &InputValue) -> Self {
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
    fn to_value(&self) -> Option<InputValue> {
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
impl SavedPlotStyleExt for SavedPlotStyle {
    fn from_override(over: PlotOverride) -> Self {
        Self {
            visible: over.visible,
            color: over.color.map(|c| [c.r, c.g, c.b, c.a]),
            width: over
                .width
                .map(|px| (f64::from(px) * WIDTH_DECIMALS).round() / WIDTH_DECIMALS),
        }
    }

    fn to_override(self) -> PlotOverride {
        PlotOverride {
            visible: self.visible,
            color: self.color.map(|[r, g, b, a]| Rgba8::new(r, g, b, a)),
            #[allow(clippy::cast_possible_truncation)]
            width: self.width.map(|px| px as f32),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    #[serde(default)]
    indicators: Vec<SavedIndicator>,
}

/// Parse an indicator-state file, reporting why it is not one. The gate a
/// bundle section goes through — see [`crate::workspace_bundle`].
pub fn validate(text: &str) -> Result<(), String> {
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
pub fn load(path: &std::path::Path) -> Vec<SavedIndicator> {
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
/// Since layouts took over the file the app reads it once, to migrate, and
/// never writes it again; the migration tests, here and in the window, write
/// it to prove the migration.
pub fn save(path: &std::path::Path, indicators: &[SavedIndicator]) {
    if quantick_workspace::write_refusal::guard_write(path).is_err() {
        return;
    }
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
                kind: SavedKind::native("native.ema"),
                hidden: false,
                mouse_vertical_line: false,
                inputs: vec![SavedInput::Int(21), SavedInput::Source("delta".to_owned())],
                plot_styles: Vec::new(),
            },
            SavedIndicator {
                kind: SavedKind::Script {
                    name: "zigzag.pine".to_owned(),
                },
                hidden: true,
                mouse_vertical_line: true,
                inputs: vec![SavedInput::Int(5)],
                plot_styles: vec![SavedPlotStyle {
                    visible: Some(false),
                    color: Some([1, 2, 3, 255]),
                    width: Some(2.5),
                }],
            },
        ]
    }

    /// A workspace saved by any build before the native catalog names its
    /// natives with the unit-variant spelling `native_ema` / `native_cvd`.
    /// Those files are on traders' disks and must open forever: the fixture
    /// here is the exact text today's serializer produced for that set.
    #[test]
    fn a_workspace_saved_before_the_catalog_still_restores_both_natives() {
        let dir = crate::scratch::ScratchDir::new("indicator-state");
        let path = dir.join("pre-catalog-state.toml");
        std::fs::write(
            &path,
            "version = 1

             [[indicators]]
kind = \"native_ema\"
hidden = false

             [[indicators.inputs]]
type = \"int\"
value = 21

             [[indicators.inputs]]
type = \"source\"
value = \"delta\"

             [[indicators]]
kind = \"native_cvd\"
hidden = true

             [[indicators]]
hidden = false

             [indicators.kind.script]
name = \"zigzag.pine\"
",
        )
        .unwrap();

        let loaded = load(&path);
        assert_eq!(loaded.len(), 3, "every entry survived: {loaded:?}");
        assert_eq!(
            loaded[0].kind,
            SavedKind::native("native.ema"),
            "the old EMA spelling names the catalog's EMA"
        );
        assert_eq!(
            loaded[0].inputs,
            vec![SavedInput::Int(21), SavedInput::Source("delta".to_owned())],
            "a tuned length and source come back with it"
        );
        assert_eq!(loaded[1].kind, SavedKind::native("native.cvd"));
        assert!(
            loaded[1].hidden,
            "the hidden flag is unrelated and survived"
        );
        assert_eq!(
            loaded[2].kind,
            SavedKind::Script {
                name: "zigzag.pine".to_owned()
            },
            "scripts never had a spelling change and must not have acquired one"
        );
        std::fs::remove_file(&path).ok();
    }

    /// The migration is read-side and one-way, on purpose: this build writes
    /// the catalog spelling, so a file converts the first time it is saved.
    /// Asserted on the text rather than on a round trip, because a round trip
    /// would pass just as well if nothing had changed.
    #[test]
    fn this_build_writes_the_catalog_spelling() {
        let dir = crate::scratch::ScratchDir::new("indicator-state");
        let path = dir.join("catalog-spelling-state.toml");
        save(
            &path,
            &[SavedIndicator {
                kind: SavedKind::native("native.ema"),
                hidden: false,
                mouse_vertical_line: false,
                inputs: Vec::new(),
                plot_styles: Vec::new(),
            }],
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("native.ema"),
            "the id is what identifies the native now: {text}"
        );
        assert!(
            !text.contains("native_ema"),
            "the pre-catalog spelling is read, never written: {text}"
        );
        std::fs::remove_file(&path).ok();
    }

    /// An id no build ships is carried, not corrected. A workspace written by
    /// a newer build, or one naming a withdrawn native, must reach the worker
    /// intact so it can say what is missing — the loader silently rewriting it
    /// to a native that does exist is the failure this whole change removes.
    #[test]
    fn an_unknown_native_id_survives_the_file_unchanged() {
        let dir = crate::scratch::ScratchDir::new("indicator-state");
        let path = dir.join("unknown-native-state.toml");
        std::fs::write(
            &path,
            "version = 1

[[indicators]]
hidden = false

             [indicators.kind.native]
id = \"native.from.the.future\"
",
        )
        .unwrap();

        let loaded = load(&path);
        assert_eq!(
            loaded.first().map(|entry| &entry.kind),
            Some(&SavedKind::native("native.from.the.future")),
            "loaded: {loaded:?}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn the_state_round_trips_through_disk() {
        let dir = crate::scratch::ScratchDir::new("indicator-state");
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
        let dir = crate::scratch::ScratchDir::new("indicator-state");
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
        let dir = crate::scratch::ScratchDir::new("indicator-state");
        let path = dir.join("unstyled-state.toml");
        save(
            &path,
            &[SavedIndicator {
                kind: SavedKind::native("native.ema"),
                hidden: false,
                mouse_vertical_line: false,
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
        let dir = crate::scratch::ScratchDir::new("indicator-state");
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
