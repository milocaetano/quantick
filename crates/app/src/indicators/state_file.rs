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

/// Environment override for the state file location.
const STATE_ENV: &str = "QUANTICK_INDICATORS_STATE";
/// Default file, next to the working directory's config.
const STATE_FILE: &str = "indicators-state.toml";
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
}

#[derive(Debug, Serialize, Deserialize)]
struct StateFile {
    version: u32,
    #[serde(default)]
    indicators: Vec<SavedIndicator>,
}

/// The state file the app opens with and writes back to.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    std::env::var_os(STATE_ENV).map_or_else(|| PathBuf::from(STATE_FILE), PathBuf::from)
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
pub(crate) fn save(path: &std::path::Path, indicators: &[SavedIndicator]) {
    let file = StateFile {
        version: FORMAT_VERSION,
        indicators: indicators.to_vec(),
    };
    match toml::to_string_pretty(&file) {
        Ok(text) => {
            if let Err(error) = std::fs::write(path, text) {
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
            },
            SavedIndicator {
                kind: SavedKind::Script {
                    name: "zigzag.pine".to_owned(),
                },
                hidden: true,
                inputs: vec![SavedInput::Int(5)],
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
