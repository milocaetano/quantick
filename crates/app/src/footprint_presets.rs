//! Named tape-reading setups the trader saves and picks again.
//!
//! A footprint config is a reading *stance*: the scalper's 4:1 imbalance
//! with numbers off reads nothing like the swing trader's coarse bands with
//! every mark on. Retuning five knobs each time you switch stance is how a
//! feature stops being used, so a stance gets a name and one click.
//!
//! Same store discipline as the drawing presets and the chart-layer state:
//! a versioned TOML (override with `QUANTICK_FOOTPRINT_PRESETS`), read once
//! at startup, written on every change, temp-file-and-rename so a crash
//! mid-write cannot leave half a file. Anything unreadable is ignored
//! entirely — a broken presets file must never cost a trader their chart.
//!
//! Presets store *appearance and thresholds only*, exactly like the bubble
//! presets: picking one never switches the layer on, never touches which
//! pane draws what, and never reaches the tape.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::footprint_config::{self, FootprintConfig};

/// Environment override for the presets file location.
const PRESETS_ENV: &str = "QUANTICK_FOOTPRINT_PRESETS";
/// Default file, beside the other app state.
const PRESETS_FILE: &str = "footprint-presets.toml";
/// Bumped on breaking layout changes; unknown versions are ignored.
const FORMAT_VERSION: u32 = 1;
/// Most presets one store keeps. A name list is a menu, not a database;
/// past this the picker is worse than retuning by hand.
pub const MAX_PRESETS: usize = 24;
/// Longest a preset name may be — enough to say "scalp WIN numbers off".
pub const MAX_NAME_LEN: usize = 40;

#[derive(Debug, Serialize, Deserialize)]
struct PresetsFile {
    version: u32,
    /// By name. `BTreeMap` so the picker's order is the file's order is
    /// alphabetical — one answer, whatever wrote it.
    #[serde(default)]
    presets: BTreeMap<String, footprint_config::FootprintFile>,
}

/// The named setups this run knows about.
#[derive(Debug, Default)]
pub struct PresetStore {
    presets: BTreeMap<String, FootprintConfig>,
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
            Ok(file) if file.version == FORMAT_VERSION => Self {
                presets: file
                    .presets
                    .into_iter()
                    .take(MAX_PRESETS)
                    // Through the same validation a hand-written file gets:
                    // a preset with a meaning-breaking ratio degrades to the
                    // default for that knob instead of drawing nonsense.
                    .map(|(name, data)| (name, footprint_config::resolve(data)))
                    .collect(),
            },
            Ok(file) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "FOOTPRINT_PRESETS_VERSION",
                    path = %path.display(),
                    version = file.version,
                    action = "starting_with_no_presets",
                    "footprint presets file is from an unknown version"
                );
                Self::default()
            }
            Err(error) => {
                tracing::warn!(
                    target: "quantick::app",
                    schema_version = 1_u8,
                    event_code = "FOOTPRINT_PRESETS_UNREADABLE",
                    path = %path.display(),
                    %error,
                    action = "starting_with_no_presets",
                    "footprint presets file is unreadable"
                );
                Self::default()
            }
        }
    }

    /// Every preset name, alphabetically.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.presets.keys().map(String::as_str)
    }

    /// The config saved under `name`.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&FootprintConfig> {
        self.presets.get(name)
    }

    /// Whether another preset would fit.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.presets.len() >= MAX_PRESETS
    }

    /// Save `config` under `name`, replacing a preset of that name. Returns
    /// whether it landed: a blank name has nothing to pick later, and a full
    /// store refuses rather than silently evicting someone's stance.
    pub fn insert(&mut self, name: &str, config: &FootprintConfig) -> bool {
        let name = name.trim();
        if name.is_empty() || name.len() > MAX_NAME_LEN {
            return false;
        }
        if self.is_full() && !self.presets.contains_key(name) {
            return false;
        }
        self.presets.insert(name.to_owned(), config.clone());
        true
    }

    /// Forget `name`.
    pub fn remove(&mut self, name: &str) -> bool {
        self.presets.remove(name).is_some()
    }

    /// Write the store. See the [module docs](self) for the discipline.
    pub fn save(&self, path: &Path) {
        let file = PresetsFile {
            version: FORMAT_VERSION,
            presets: self
                .presets
                .iter()
                .map(|(name, config)| (name.clone(), footprint_config::to_file(config)))
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
                event_code = "FOOTPRINT_PRESETS_WRITE_FAILED",
                path = %path.display(),
                %error,
                action = "presets_not_saved",
                "could not save the footprint presets"
            );
        }
    }
}

/// Where this run keeps its presets. Under test, a scratch file per store,
/// for the same reason every other app-state path does it.
#[must_use]
pub fn default_path() -> PathBuf {
    if cfg!(test) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        return std::env::temp_dir().join(format!(
            "quantick-footprint-presets-{}-{}.toml",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    }
    std::env::var_os(PRESETS_ENV).map_or_else(|| PathBuf::from(PRESETS_FILE), PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint_config::FootprintStyle;
    use rust_decimal::Decimal;

    fn stance(ratio: i64, numbers: bool) -> FootprintConfig {
        FootprintConfig {
            imbalance_ratio: Decimal::from(ratio),
            show_numbers: numbers,
            style: FootprintStyle::Split,
            ..FootprintConfig::default()
        }
    }

    #[test]
    fn presets_round_trip_through_disk() {
        let path = default_path();
        let mut store = PresetStore::default();
        assert!(store.insert("scalp", &stance(4, false)));
        assert!(store.insert("swing", &stance(2, true)));
        store.save(&path);

        let loaded = PresetStore::load(&path);
        assert_eq!(loaded.names().collect::<Vec<_>>(), ["scalp", "swing"]);
        assert_eq!(loaded.get("scalp"), Some(&stance(4, false)));
        assert_eq!(loaded.get("swing"), Some(&stance(2, true)));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_or_broken_file_costs_no_chart() {
        assert_eq!(PresetStore::load(&default_path()).names().count(), 0);
        let path = default_path();
        std::fs::write(&path, "not even toml [").unwrap();
        assert_eq!(PresetStore::load(&path).names().count(), 0);
        std::fs::write(&path, "version = 99\n[presets]\n").unwrap();
        assert_eq!(PresetStore::load(&path).names().count(), 0);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn names_are_trimmed_and_bounded_and_the_store_refuses_to_evict() {
        let mut store = PresetStore::default();
        assert!(!store.insert("   ", &stance(3, true)), "blank name");
        assert!(
            !store.insert(&"x".repeat(MAX_NAME_LEN + 1), &stance(3, true)),
            "a name too long to show"
        );
        assert!(store.insert("  spaced  ", &stance(3, true)));
        assert_eq!(store.names().collect::<Vec<_>>(), ["spaced"]);

        for i in 0..MAX_PRESETS {
            store.insert(&format!("p{i}"), &stance(3, true));
        }
        assert!(store.is_full());
        assert!(!store.insert("one more", &stance(3, true)), "no eviction");
        // Overwriting an existing name still works when full.
        assert!(store.insert("p0", &stance(5, false)));
        assert_eq!(store.get("p0"), Some(&stance(5, false)));
        assert!(store.remove("p0"));
        assert!(!store.remove("p0"));
    }
}
