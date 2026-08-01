//! Persistent storage of custom drawing presets.
//!
//! One versioned TOML file holds, per tool id, the named payload exports and
//! the explicit "default for new objects" choice. Reading a file from a
//! future version leaves the store empty rather than guessing — the file on
//! disk is never rewritten until the user saves something again.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::PresetHost;

/// Environment override for the preset file location.
pub const PRESETS_ENV: &str = "QUANTICK_DRAWING_PRESETS";
/// Default preset file, next to the working directory's quantick.toml.
pub const PRESETS_FILE: &str = "quantick-drawing-presets.toml";
/// Version this build writes and the only one it reads.
const STORE_FORMAT_VERSION: u32 = 1;

/// Per-tool slot: named presets plus the optional default-for-new choice.
/// `BTreeMap` keeps the file diff-stable across saves.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct ToolPresets {
    #[serde(default)]
    presets: BTreeMap<String, toml::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    #[serde(default)]
    tools: BTreeMap<String, ToolPresets>,
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
    /// Resolve the preset file the same way the app config resolves:
    /// the env override first, then the working directory.
    #[must_use]
    pub fn default_path() -> PathBuf {
        std::env::var_os(PRESETS_ENV).map_or_else(|| PathBuf::from(PRESETS_FILE), PathBuf::from)
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

    #[test]
    fn an_unknown_file_version_starts_empty_instead_of_guessing() {
        let path = scratch_path("future-version");
        std::fs::write(&path, "version = 99\n[tools]\n").unwrap();
        let store = PresetStore::load_from(&path);
        assert!(store.custom_preset_names("fib-retracement").is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
