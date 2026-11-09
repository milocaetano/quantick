//! Persistent storage of custom drawing presets.
//!
//! One versioned TOML file holds, per tool id, the named payload exports and
//! the explicit "default for new objects" choice. Reading a file from a
//! future version leaves the store empty rather than guessing — the file on
//! disk is never rewritten until the user saves something again.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{DrawingStyle, PresetHost};

/// The file's name inside the durable cockpit home; the window resolves where
/// that is and hands the path in.
pub const PRESETS_FILE: &str = "quantick-drawing-presets.toml";
/// Version this build writes and the only one it reads.
const STORE_FORMAT_VERSION: u32 = 1;

/// The common style, as it goes to disk. Written out field by field rather
/// than deriving on `egui::Color32` so the file stays something a human can
/// read and edit — the reason it is tracked at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoredStyle {
    /// `#rrggbb`, the notation every other config in this repo uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<String>,
    /// Exact renderer bytes for translucent styles; never mislabel them as straight RGB.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color_premultiplied: Option<[u8; 4]>,
    width_px: f32,
    fill_alpha: u8,
}

impl StoredStyle {
    fn from_style(style: DrawingStyle) -> Self {
        let [red, green, blue, alpha] = style.color.to_array();
        Self {
            color: (alpha == 255).then(|| format!("#{red:02x}{green:02x}{blue:02x}")),
            color_premultiplied: (alpha != 255).then_some(style.color.to_array()),
            width_px: style.width_px,
            fill_alpha: style.fill_alpha,
        }
    }

    /// A style the file could not express is no style at all: a hand-edited
    /// colour that does not parse falls back to the built-in start rather
    /// than to a silently different one.
    fn to_style(&self) -> Option<DrawingStyle> {
        let color = if let Some(rgba) = self.color_premultiplied {
            quantick_appearance::PremultipliedRgba::from_premultiplied(rgba)
        } else {
            let hex = self.color.as_ref()?.strip_prefix('#')?;
            if hex.len() != 6 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return None;
            }
            let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
            quantick_appearance::PremultipliedRgba::from_rgb(channel(0)?, channel(2)?, channel(4)?)
        };
        Some(DrawingStyle {
            color,
            width_px: self
                .width_px
                .clamp(crate::MIN_DRAWING_WIDTH_PX, crate::MAX_DRAWING_WIDTH_PX),
            fill_alpha: self.fill_alpha.min(crate::MAX_DRAWING_FILL_ALPHA),
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
    /// The tool's own configuration for new objects — a payload export,
    /// exactly the shape a named preset holds. Added the same way and for
    /// the same reason as `style`: absent in an older file, ignored by an
    /// older build, so the format version stays put.
    ///
    /// Stored as the tool's opaque export rather than parsed here: this file
    /// knows about tools, not about Fib levels, and the tool that wrote the
    /// table is the one that reads it back through `import_preset` — which
    /// already refuses a table it does not recognise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config: Option<toml::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    #[serde(default)]
    tools: BTreeMap<String, ToolPresets>,
}

/// Parse a drawing-presets file, reporting why it is not one. The gate a
/// bundle section goes through — see the window's `workspace_bundle`.
pub fn validate(text: &str) -> Result<(), String> {
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
    theme: Option<(u64, quantick_appearance::ResolvedTheme)>,
    path: PathBuf,
    tools: BTreeMap<String, ToolPresets>,
}

impl PresetStore {
    /// Revision-gated snapshot; never changes existing objects or saved defaults.
    pub fn sync_theme(&mut self, appearance: &quantick_appearance::Appearance) {
        if self.theme.as_ref().map(|(revision, _)| *revision) != Some(appearance.revision()) {
            self.theme = Some((appearance.revision(), appearance.resolved().clone()));
        }
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
        Self {
            path,
            tools,
            theme: None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    fn persist(&self) {
        if quantick_workspace::write_refusal::guard_write(&self.path).is_err() {
            return;
        }
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
    fn theme(&self) -> Option<&quantick_appearance::ResolvedTheme> {
        self.theme.as_ref().map(|(_, theme)| theme)
    }
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

    fn default_config(&self, tool_id: &str) -> Option<toml::Value> {
        self.tools.get(tool_id)?.config.clone()
    }

    fn set_default_config(&mut self, tool_id: &str, value: Option<toml::Value>) {
        let slot = self.tools.entry(tool_id.to_owned()).or_default();
        slot.config = value;
        self.persist();
    }

    fn has_default_config(&self, tool_id: &str) -> bool {
        self.tools
            .get(tool_id)
            .is_some_and(|slot| slot.config.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A preset file of this test's own. `thread_dir` rather than a
    /// `ScratchFile`, because a `PresetStore` outlives the expression that
    /// built it and keeps writing to the path.
    fn scratch_path(name: &str) -> PathBuf {
        crate::scratch::thread_dir("preset-test").join(format!("{name}.toml"))
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

    /// The chore this removes is the one the trader actually complained
    /// about: a Fib whose levels, per-level colours and labels had to be
    /// rebuilt every session, because the only thing that survived a restart
    /// was the outline colour.
    #[test]
    fn a_default_configuration_survives_a_restart() {
        let path = scratch_path("default-config");
        let mine = sample_value("my-levels");
        {
            let mut store = PresetStore::load_from(&path);
            assert_eq!(store.default_config("fib-retracement"), None);
            store.set_default_config("fib-retracement", Some(mine.clone()));
        }
        let store = PresetStore::load_from(&path);
        assert_eq!(store.default_config("fib-retracement"), Some(mine));
        assert_eq!(
            store.default_config("fib-extension"),
            None,
            "one tool's configuration is not every tool's"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// Clearing it is a real state, not an absence the next save has to fix:
    /// the reset button writes `None` and the file must come back empty.
    #[test]
    fn resetting_the_configuration_writes_the_absence_through() {
        let path = scratch_path("reset-config");
        {
            let mut store = PresetStore::load_from(&path);
            store.set_default_config("rectangle", Some(sample_value("boxy")));
            store.set_default_config("rectangle", None);
        }
        let store = PresetStore::load_from(&path);
        assert_eq!(store.default_config("rectangle"), None);
        let _ = std::fs::remove_file(&path);
    }

    /// A file written before the configuration slot existed still reads —
    /// the key is optional and the format version does not move for it.
    #[test]
    fn a_file_without_the_configuration_slot_still_loads() {
        let path = scratch_path("older-file");
        std::fs::write(
            &path,
            "version = 1

[tools.\"trend-line\".style]
color = \"#ff0000\"
width_px = 2.0
fill_alpha = 12
",
        )
        .expect("the scratch file is writable");
        let store = PresetStore::load_from(&path);
        assert!(
            store.default_style("trend-line").is_some(),
            "the older file must still be read, not rejected"
        );
        assert_eq!(store.default_config("trend-line"), None);
        let _ = std::fs::remove_file(&path);
    }

    /// The chore this removes: setting the colour on every single object.
    #[test]
    fn a_default_style_survives_a_restart() {
        let path = scratch_path("default-style");
        let mine = DrawingStyle {
            color: quantick_chart::style::PremultipliedRgba::from_rgb(0xFF, 0xA0, 0x10),
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
        assert_eq!(style.width_px, crate::MAX_DRAWING_WIDTH_PX);
        assert_eq!(style.fill_alpha, crate::MAX_DRAWING_FILL_ALPHA);
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

#[cfg(test)]
mod appearance_tests {
    use super::*;
    use crate::{DrawingPayload, FontFamily, TextPayload, new_drawing_from_defaults};

    #[test]
    fn light_creates_slate_drawings_and_blue_gray_profiles_without_recoloring_saved_styles() {
        let path = crate::scratch::thread_dir("light-drawing-defaults").join(PRESETS_FILE);
        let mut store = PresetStore::load_from(&path);
        let mut appearance = quantick_appearance::Appearance::default();
        store.sync_theme(&appearance);
        let original: Vec<_> = crate::DRAWING_TOOLS
            .into_iter()
            .map(|tool| new_drawing_from_defaults(&store, tool))
            .collect();
        appearance.apply(quantick_appearance::LIGHT).unwrap();
        store.sync_theme(&appearance);
        for (tool, existing) in crate::DRAWING_TOOLS.into_iter().zip(original) {
            let fresh = new_drawing_from_defaults(&store, tool);
            // Every tool is born in the slate default but those whose colour
            // is their meaning: the buy and sell marks, the VWAP and the
            // profile.
            let expected = match tool.id() {
                "fixed-range-profile" => [122, 142, 194, 255],
                "anchored-vwap" => [36, 93, 177, 255],
                "arrow-mark-up" => [31, 122, 52, 255],
                "arrow-mark-down" => [17, 17, 17, 255],
                _ => [43, 54, 64, 255],
            };
            assert_eq!(fresh.style.color.to_array(), expected, "{}", tool.id());
            assert_eq!(fresh.style.width_px, existing.style.width_px);
            assert_eq!(fresh.style.fill_alpha, existing.style.fill_alpha);
            assert_eq!(existing.style, tool.default_style());
        }
        let line = crate::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "trend-line")
            .unwrap();
        let legacy: StoredStyle =
            toml::from_str("color = '#8ab4f8'\nwidth_px = 1.0\nfill_alpha = 20").unwrap();
        let authored = legacy.to_style().unwrap();
        store.set_default_style(line.id(), Some(authored));
        let mut loaded = PresetStore::load_from(&path);
        loaded.sync_theme(&appearance);
        assert_eq!(new_drawing_from_defaults(&loaded, line).style, authored);
        let profile = crate::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "fixed-range-profile")
            .unwrap();
        loaded.set_default_style(profile.id(), Some(authored));
        let mut restored = PresetStore::load_from(&path);
        restored.sync_theme(&appearance);
        assert_eq!(
            new_drawing_from_defaults(&restored, profile).style,
            authored
        );
        assert_ne!(
            appearance
                .resolved()
                .color(quantick_appearance::Token::Accent),
            appearance
                .resolved()
                .color(quantick_appearance::Token::DrawingDefault)
        );
    }

    #[test]
    fn every_registered_tool_keeps_its_historical_dark_default() {
        use crate::ToolCatalog;
        let dark = quantick_appearance::ResolvedTheme::default();
        for tool in crate::DRAWING_TOOLS {
            assert_eq!(
                tool.themed_style(Some(&dark)),
                tool.default_style(),
                "{}",
                tool.id()
            );
        }
    }

    #[test]
    fn new_objects_follow_the_theme_and_saved_defaults_roundtrip_as_authored() {
        let path = crate::scratch::thread_dir("appearance-drawing").join(PRESETS_FILE);
        let _ = std::fs::remove_file(&path);
        let mut store = PresetStore::load_from(&path);
        let mut appearance = quantick_appearance::Appearance::default();
        store.sync_theme(&appearance);
        let text = crate::DRAWING_TOOLS
            .into_iter()
            .find(|tool| tool.id() == "text")
            .unwrap();
        let original = new_drawing_from_defaults(&store, text);
        let source = "version = 1\nname = 'new defaults'\n[colors]\ndrawing_default = '#12345680'\n[typography.drawing]\nfamily = 'monospace'\nsize = 32.0";
        appearance.apply(source).unwrap();
        store.sync_theme(&appearance);
        let themed = new_drawing_from_defaults(&store, text);
        assert_ne!(themed.style.color, original.style.color);
        assert_eq!(original.style, text.default_style());
        let payload = themed
            .payload
            .as_any()
            .downcast_ref::<TextPayload>()
            .unwrap();
        assert_eq!(
            (payload.size_px, payload.family),
            (32.0, FontFamily::Monospace)
        );
        store.set_default_style(text.id(), Some(themed.style));
        store.set_default_config(text.id(), payload.export_preset());
        let mut loaded = PresetStore::load_from(&path);
        loaded.sync_theme(&quantick_appearance::Appearance::default());
        let saved = new_drawing_from_defaults(&loaded, text);
        assert_eq!(
            saved.style, themed.style,
            "translucent premultiplied bytes roundtrip exactly"
        );
        assert!(saved.payload.eq_dyn(themed.payload.as_ref()));
        // An explicit saved color equal to the old factory default is still authored.
        loaded.set_default_style(text.id(), Some(original.style));
        let mut loaded = PresetStore::load_from(&path);
        loaded.sync_theme(&appearance);
        assert_eq!(
            new_drawing_from_defaults(&loaded, text).style,
            original.style
        );
        // Legacy config had only a size: it captures the historical proportional face.
        loaded.set_default_config(text.id(), Some(toml::from_str("size_px = 18.0").unwrap()));
        let fresh = new_drawing_from_defaults(&loaded, text);
        let payload = fresh
            .payload
            .as_any()
            .downcast_ref::<TextPayload>()
            .unwrap();
        assert_eq!(
            (payload.size_px, payload.family),
            (18.0, FontFamily::Proportional)
        );
        let legacy: StoredStyle =
            toml::from_str("color = '#8ab4f8'\nwidth_px = 1.0\nfill_alpha = 20").unwrap();
        assert_eq!(legacy.to_style().unwrap().color, original.style.color);
    }
}
