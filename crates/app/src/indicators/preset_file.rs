//! Named input setups the trader saves per indicator and picks again.
//!
//! A tuned Copilot is a stance the same way a footprint config is: the
//! choppy-day tolerance and the trend-day aggression bar are different
//! numbers, and retyping sixteen of them is how a calibration gets lost.
//! A preset names the whole input set of one indicator *kind* — a Copilot
//! preset never offers itself to an EMA.
//!
//! Same store discipline as [`crate::footprint_presets`]: a versioned TOML
//! (override with `QUANTICK_INDICATOR_PRESETS`), read once at startup,
//! written on every change, temp-file-and-rename so a crash mid-write
//! cannot leave half a file. Anything unreadable is ignored entirely.
//! Values persist as [`SavedInput`]s in declaration order, exactly like the
//! state file — the built-in "Default" preset is not stored here at all,
//! it is the script's own declared defaults.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::state_file::{SavedInput, SavedKind};

/// Environment override for the presets file location.
const PRESETS_ENV: &str = "QUANTICK_INDICATOR_PRESETS";
/// Default file, beside the other app state.
const PRESETS_FILE: &str = "indicator-presets.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;
/// Most presets one indicator kind keeps; same menu-not-database bound as
/// the footprint store.
pub(crate) const MAX_PRESETS_PER_KIND: usize = 24;
/// Longest a preset name may be.
pub(crate) const MAX_NAME_LEN: usize = 40;

/// One saved setup: which indicator it belongs to, its name, its values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct PresetEntry {
    kind: SavedKind,
    name: String,
    inputs: Vec<SavedInput>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PresetsFile {
    version: u32,
    #[serde(default)]
    presets: Vec<PresetEntry>,
}

/// The named setups this run knows about, across every indicator kind.
#[derive(Debug, Default)]
pub(crate) struct PresetStore {
    /// Sorted by (kind's TOML form, name) at load and on insert, so the
    /// picker's order is stable whatever wrote the file.
    presets: Vec<PresetEntry>,
}

impl PresetStore {
    /// Load from `path`; empty when the file is missing, unreadable or from
    /// an unknown version.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        match toml::from_str::<PresetsFile>(&text) {
            Ok(file) if file.version == FORMAT_VERSION => {
                let mut store = Self {
                    presets: file.presets,
                };
                store.sort();
                store
            }
            Ok(file) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_PRESETS_VERSION",
                    path = %path.display(),
                    version = file.version,
                    action = "starting_with_no_presets",
                    "indicator presets file is from an unknown version"
                );
                Self::default()
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "INDICATOR_PRESETS_UNREADABLE",
                    path = %path.display(),
                    %error,
                    action = "starting_with_no_presets",
                    "indicator presets file is unreadable"
                );
                Self::default()
            }
        }
    }

    /// Every preset name saved for `kind`, in stable (alphabetical) order.
    pub fn names_for<'a>(&'a self, kind: &'a SavedKind) -> impl Iterator<Item = &'a str> {
        self.presets
            .iter()
            .filter(move |entry| entry.kind == *kind)
            .map(|entry| entry.name.as_str())
    }

    /// The values saved for `kind` under `name`.
    #[must_use]
    pub fn get(&self, kind: &SavedKind, name: &str) -> Option<&[SavedInput]> {
        self.presets
            .iter()
            .find(|entry| entry.kind == *kind && entry.name == name)
            .map(|entry| entry.inputs.as_slice())
    }

    /// Whether another preset would fit for `kind`.
    #[must_use]
    pub fn is_full(&self, kind: &SavedKind) -> bool {
        self.names_for(kind).count() >= MAX_PRESETS_PER_KIND
    }

    /// Save `inputs` for `kind` under `name`, replacing a preset of that
    /// name. Returns whether it landed: a blank name has nothing to pick
    /// later, and a full store refuses rather than silently evicting.
    pub fn insert(&mut self, kind: &SavedKind, name: &str, inputs: Vec<SavedInput>) -> bool {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return false;
        }
        if let Some(existing) = self
            .presets
            .iter_mut()
            .find(|entry| entry.kind == *kind && entry.name == name)
        {
            existing.inputs = inputs;
            return true;
        }
        if self.is_full(kind) {
            return false;
        }
        self.presets.push(PresetEntry {
            kind: kind.clone(),
            name: name.to_owned(),
            inputs,
        });
        self.sort();
        true
    }

    /// Forget `name` for `kind`.
    pub fn remove(&mut self, kind: &SavedKind, name: &str) -> bool {
        let before = self.presets.len();
        self.presets
            .retain(|entry| !(entry.kind == *kind && entry.name == name));
        self.presets.len() != before
    }

    /// Write the store. See the [module docs](self) for the discipline.
    pub fn save(&self, path: &Path) {
        let file = PresetsFile {
            version: FORMAT_VERSION,
            presets: self.presets.clone(),
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
                event_code = "INDICATOR_PRESETS_WRITE_FAILED",
                path = %path.display(),
                %error,
                action = "presets_not_saved",
                "could not save the indicator presets"
            );
        }
    }

    /// One order whoever wrote the file: by kind's serialized form, then
    /// name. `SavedKind` has no `Ord` of its own; its TOML text is the
    /// stable key the file already exposes.
    fn sort(&mut self) {
        self.presets.sort_by(|a, b| {
            let ka = toml::to_string(&a.kind).unwrap_or_default();
            let kb = toml::to_string(&b.kind).unwrap_or_default();
            ka.cmp(&kb).then_with(|| a.name.cmp(&b.name))
        });
    }
}

/// Where this run keeps its presets. Under test, a scratch file per store,
/// for the same reason every other app-state path does it.
#[must_use]
pub(crate) fn default_path() -> PathBuf {
    if cfg!(test) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        return std::env::temp_dir().join(format!(
            "quantick-indicator-presets-{}-{}.toml",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    }
    std::env::var_os(PRESETS_ENV).map_or_else(|| PathBuf::from(PRESETS_FILE), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn copilot() -> SavedKind {
        SavedKind::Script {
            name: "copilot.pine".to_owned(),
        }
    }

    fn choppy() -> Vec<SavedInput> {
        vec![SavedInput::Int(120), SavedInput::Float(4.5)]
    }

    #[test]
    fn presets_round_trip_through_disk() {
        let path = default_path();
        let mut store = PresetStore::default();
        assert!(store.insert(&copilot(), "choppy day", choppy()));
        assert!(store.insert(&SavedKind::NativeEma, "fast", vec![SavedInput::Int(9)]));
        store.save(&path);

        let loaded = PresetStore::load(&path);
        assert_eq!(
            loaded.names_for(&copilot()).collect::<Vec<_>>(),
            ["choppy day"],
            "a Copilot preset never offers itself to an EMA"
        );
        assert_eq!(loaded.get(&copilot(), "choppy day"), Some(&choppy()[..]));
        assert_eq!(
            loaded.get(&SavedKind::NativeEma, "fast"),
            Some(&[SavedInput::Int(9)][..])
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_or_broken_file_costs_no_chart() {
        assert_eq!(
            PresetStore::load(&default_path())
                .names_for(&copilot())
                .count(),
            0
        );
        let path = default_path();
        std::fs::write(&path, "not even toml [").unwrap();
        assert_eq!(PresetStore::load(&path).names_for(&copilot()).count(), 0);
        std::fs::write(&path, "version = 99\n").unwrap();
        assert_eq!(PresetStore::load(&path).names_for(&copilot()).count(), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn names_are_trimmed_and_bounded_and_the_store_refuses_to_evict() {
        let mut store = PresetStore::default();
        assert!(!store.insert(&copilot(), "   ", choppy()), "blank name");
        assert!(
            !store.insert(&copilot(), &"x".repeat(MAX_NAME_LEN + 1), choppy()),
            "a name too long to show"
        );
        assert!(store.insert(&copilot(), "  spaced  ", choppy()));
        assert_eq!(store.names_for(&copilot()).collect::<Vec<_>>(), ["spaced"]);

        for i in 0..MAX_PRESETS_PER_KIND {
            store.insert(&copilot(), &format!("p{i}"), choppy());
        }
        assert!(store.is_full(&copilot()));
        assert!(
            !store.insert(&copilot(), "one more", choppy()),
            "no eviction"
        );
        assert!(
            store.insert(&copilot(), "p0", vec![SavedInput::Int(1)]),
            "overwriting an existing name still works when full"
        );
        assert!(
            store.insert(&SavedKind::NativeEma, "ema", vec![SavedInput::Int(9)]),
            "another kind's shelf is not full"
        );
        assert!(store.remove(&copilot(), "p0"));
        assert!(!store.remove(&copilot(), "p0"));
    }
}
