//! Persistent storage of custom drawing presets.
//!
//! One versioned TOML file holds, per tool id, the named payload exports and
//! the explicit "default for new objects" choice. Reading a file from a
//! future version leaves the store empty rather than guessing — the file on
//! disk is never rewritten until the user saves something again.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{DrawingStyle, PresetHost};

/// Environment override for the preset file location.
pub const PRESETS_ENV: &str = "QUANTICK_DRAWING_PRESETS";
/// The file's name inside the durable cockpit home. See [`crate::store_home`].
pub const PRESETS_FILE: &str = "quantick-drawing-presets.toml";
/// Version this build writes and the only one it reads.
const STORE_FORMAT_VERSION: u32 = 1;

/// The common style, as it goes to disk. Written out field by field rather
/// than deriving on `egui::Color32` so the file stays something a human can
/// read and edit — the reason it is tracked at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredStyle {
    /// `#rrggbb`, the notation every other config in this repo uses.
    color: String,
    width_px: f32,
    fill_alpha: u8,
}

impl StoredStyle {
    fn from_style(style: DrawingStyle) -> Self {
        let [red, green, blue, _] = style.color.to_array();
        Self {
            color: format!("#{red:02x}{green:02x}{blue:02x}"),
            width_px: style.width_px,
            fill_alpha: style.fill_alpha,
        }
    }

    /// A style the file could not express is no style at all: a hand-edited
    /// colour that does not parse falls back to the built-in start rather
    /// than to a silently different one.
    fn to_style(&self) -> Option<DrawingStyle> {
        let hex = self.color.strip_prefix('#')?;
        if hex.len() != 6 {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        Some(DrawingStyle {
            color: super::egui::Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?),
            width_px: self
                .width_px
                .clamp(super::MIN_DRAWING_WIDTH_PX, super::MAX_DRAWING_WIDTH_PX),
            fill_alpha: self.fill_alpha.min(super::MAX_DRAWING_FILL_ALPHA),
        })
    }
}

/// Per-tool slot: named presets, the optional default-for-new choice, and the
/// colour/width/fill new objects of this tool open with.
/// `BTreeMap` keeps the file diff-stable across saves.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct ToolPresets {
    #[serde(default)]
    presets: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
    /// Added after the format shipped, so it is optional and absent by
    /// default: a file written before it existed reads clean, and a build
    /// without it ignores the key instead of refusing the file. That is why
    /// the version does not move.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    style: Option<StoredStyle>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    #[serde(default)]
    tools: BTreeMap<String, ToolPresets>,
}

/// Parse a drawing-presets file, reporting why it is not one. The gate a
/// bundle section goes through — see `crate::workspace_bundle`.
pub(crate) fn validate(text: &str) -> Result<(), String> {
    let file: StoreFile = toml::from_str(text).map_err(|error| error.to_string())?;
    if file.version == STORE_FORMAT_VERSION {
        Ok(())
    } else {
        Err(format!(
            "drawing-presets format version {} (this build reads {STORE_FORMAT_VERSION})",
            file.version
        ))
    }
}

/// The app-side [`PresetHost`]: an in-memory copy of the preset file that
/// writes itself back after every mutation (preset edits are rare,
/// event-driven work — never on the frame path).
#[derive(Debug)]
pub struct PresetStore {
    path: PathBuf,
    tools: BTreeMap<String, ToolPresets>,
}

impl PresetStore {
    /// Resolve the preset file: the env override first, then the durable
    /// cockpit home — see [`crate::store_home`] for why the tool colours used
    /// to vanish when the app was launched from elsewhere.
    #[must_use]
    pub fn default_path() -> PathBuf {
        if cfg!(test) {
            return crate::store_home::test_path(PRESETS_FILE);
        }
        crate::store_home::resolve(PRESETS_ENV, PRESETS_FILE)
    }

    /// Load the store, empty when the file is missing, unreadable or from an
    /// unknown version (reported, never silently half-read).
    #[must_use]
    pub fn load_from(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let tools = match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<StoreFile>(&text) {
                Ok(file) if file.version == STORE_FORMAT_VERSION => file.tools,
                Ok(file) => {
                    tracing::warn!(
                        path = %path.display(),
                        version = file.version,
                        "drawing preset file is from an unknown version; starting empty"
                    );
                    BTreeMap::new()
                }
                Err(error) => {
                    tracing::warn!(
                        path = %path.display(),
                        %error,
                        "drawing preset file is unreadable; starting empty"
                    );
                    BTreeMap::new()
                }
            },
            Err(_) => BTreeMap::new(),
        };
        Self { path, tools }
    }

    #[cfg(test)]
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn persist(&self) {
        let file = StoreFile {
            version: STORE_FORMAT_VERSION,
            tools: self.tools.clone(),
        };
        match toml::to_string_pretty(&file) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&self.path, text) {
                    tracing::warn!(
                        path = %self.path.display(),
                        %error,
                        "could not save drawing presets"
                    );
                }
            }
            Err(error) => {
                tracing::warn!(%error, "could not serialize drawing presets");
            }
        }
    }
}

impl PresetHost for PresetStore {
    fn custom_preset_names(&self, tool_id: &str) -> Vec<String> {
        self.tools
            .get(tool_id)
            .map(|slot| slot.presets.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn load_custom_preset(&self, tool_id: &str, name: &str) -> Option<toml::Value> {
        self.tools.get(tool_id)?.presets.get(name).cloned()
    }

    fn save_custom_preset(
        &mut self,
        tool_id: &str,
        name: &str,
        value: toml::Value,
        overwrite: bool,
    ) -> bool {
        let slot = self.tools.entry(tool_id.to_owned()).or_default();
        if slot.presets.contains_key(name) && !overwrite {
            return false;
        }
        slot.presets.insert(name.to_owned(), value);
        self.persist();
        true
    }

    fn delete_custom_preset(&mut self, tool_id: &str, name: &str) {
        if let Some(slot) = self.tools.get_mut(tool_id) {
            slot.presets.remove(name);
            // Deleting the default preset falls back to the built-in start;
            // objects that already copied its values keep them.
            if slot.default.as_deref() == Some(name) {
                slot.default = None;
            }
            self.persist();
        }
    }

    fn default_preset(&self, tool_id: &str) -> Option<String> {
        let slot = self.tools.get(tool_id)?;
        let name = slot.default.clone()?;
        // A dangling default (preset deleted by hand in the file) is no
        // default at all.
        slot.presets.contains_key(&name).then_some(name)
    }

    fn set_default_preset(&mut self, tool_id: &str, name: Option<String>) {
        let slot = self.tools.entry(tool_id.to_owned()).or_default();
        slot.default = name;
        self.persist();
    }

    fn default_style(&self, tool_id: &str) -> Option<DrawingStyle> {
        self.tools.get(tool_id)?.style.as_ref()?.to_style()
    }

    fn set_default_style(&mut self, tool_id: &str, style: Option<DrawingStyle>) {
        let slot = self.tools.entry(tool_id.to_owned()).or_default();
        slot.style = style.map(StoredStyle::from_style);
        self.persist();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "quantick-preset-test-{name}-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    fn sample_value(marker: &str) -> toml::Value {
        toml::Value::Table(
            [(String::from("marker"), toml::Value::String(marker.into()))]
                .into_iter()
                .collect(),
        )
    }

    #[test]
    fn custom_presets_survive_a_restart_via_the_versioned_file() {
        let path = scratch_path("roundtrip");
        {
            let mut store = PresetStore::load_from(&path);
            assert!(store.save_custom_preset("fib-retracement", "mine", sample_value("a"), false));
            store.set_default_preset("fib-retracement", Some("mine".into()));
        }
        // A fresh load — the "restart".
        let store = PresetStore::load_from(&path);
        assert_eq!(store.custom_preset_names("fib-retracement"), ["mine"]);
        assert_eq!(
            store.load_custom_preset("fib-retracement", "mine"),
            Some(sample_value("a"))
        );
        assert_eq!(
            store.default_preset("fib-retracement"),
            Some("mine".to_owned())
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn overwriting_needs_explicit_consent() {
        let path = scratch_path("overwrite");
        let mut store = PresetStore::load_from(&path);
        assert!(store.save_custom_preset("fib-retracement", "mine", sample_value("a"), false));
        assert!(
            !store.save_custom_preset("fib-retracement", "mine", sample_value("b"), false),
            "an existing name is not silently replaced"
        );
        assert_eq!(
            store.load_custom_preset("fib-retracement", "mine"),
            Some(sample_value("a"))
        );
        assert!(store.save_custom_preset("fib-retracement", "mine", sample_value("b"), true));
        assert_eq!(
            store.load_custom_preset("fib-retracement", "mine"),
            Some(sample_value("b"))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn deleting_the_default_preset_returns_to_the_builtin_start() {
        let path = scratch_path("delete-default");
        let mut store = PresetStore::load_from(&path);
        store.save_custom_preset("fib-extension", "targets+", sample_value("x"), false);
        store.set_default_preset("fib-extension", Some("targets+".into()));
        store.delete_custom_preset("fib-extension", "targets+");
        assert_eq!(store.default_preset("fib-extension"), None);
        assert!(store.custom_preset_names("fib-extension").is_empty());
        let _ = std::fs::remove_file(&path);
    }

    /// The chore this removes: setting the colour on every single object.
    #[test]
    fn a_default_style_survives_a_restart() {
        let path = scratch_path("default-style");
        let mine = DrawingStyle {
            color: super::super::egui::Color32::from_rgb(0xFF, 0xA0, 0x10),
            width_px: 2.5,
            fill_alpha: 40,
        };
        {
            let mut store = PresetStore::load_from(&path);
            assert_eq!(store.default_style("trend-line"), None);
            store.set_default_style("trend-line", Some(mine));
        }
        let store = PresetStore::load_from(&path);
        assert_eq!(store.default_style("trend-line"), Some(mine));
        assert_eq!(
            store.default_style("rectangle"),
            None,
            "the choice is per tool, not a global repaint"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// The file is tracked so a human can read and edit it; a colour that
    /// does not parse must fall back to the built-in start, never to a
    /// silently different colour.
    #[test]
    fn a_hand_broken_colour_is_no_default_rather_than_a_wrong_one() {
        let path = scratch_path("broken-colour");
        std::fs::write(
            &path,
            "version = 1
[tools.\"trend-line\".style]
color = \"not-a-colour\"
width_px = 1.0
fill_alpha = 10
",
        )
        .unwrap();
        let store = PresetStore::load_from(&path);
        assert_eq!(store.default_style("trend-line"), None);
        let _ = std::fs::remove_file(&path);
    }

    /// A width or opacity edited past the slider's range is clamped, not
    /// trusted: the file is editable, and an out-of-range value must not
    /// produce a drawing the UI cannot represent.
    #[test]
    fn hand_edited_values_are_clamped_to_what_the_sliders_allow() {
        let path = scratch_path("clamp");
        std::fs::write(
            &path,
            "version = 1
[tools.\"rectangle\".style]
color = \"#8ab4f8\"
width_px = 99.0
fill_alpha = 255
",
        )
        .unwrap();
        let store = PresetStore::load_from(&path);
        let style = store.default_style("rectangle").expect("a readable style");
        assert_eq!(style.width_px, super::super::MAX_DRAWING_WIDTH_PX);
        assert_eq!(style.fill_alpha, super::super::MAX_DRAWING_FILL_ALPHA);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unknown_file_version_starts_empty_instead_of_guessing() {
        let path = scratch_path("future-version");
        std::fs::write(&path, "version = 99\n[tools]\n").unwrap();
        let store = PresetStore::load_from(&path);
        assert!(store.custom_preset_names("fib-retracement").is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
