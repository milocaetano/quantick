//! Bubble settings per asset.
//!
//! An asset is what [`FeedConfig::bubble_asset`](crate::config::FeedConfig::bubble_asset)
//! names: a symbol, or a family of dated contracts one config key declares
//! (`WIN*`). Each asset keeps its own aggression-bubble settings — the panel's
//! look, the tape mode, the tape's and the candles' opening-burst scale and
//! the flow pane's candle aggression — so a change made while one market is on
//! screen never dresses another.
//!
//! The config declares the look an asset opens on; this store keeps what the
//! trader changed from there, one entry per asset, in [`ASSETS_FILE`] in the
//! cockpit home. An entry exists only while it differs from the declared look,
//! so a preset updated in `bubbles.toml` still reaches every asset nobody
//! tuned by hand.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use quantick_orderflow::HeatmapConfig;

use crate::bubble_presets::{BubblePreset, BubblePresetFile};
use crate::config::BubbleAsset;

/// The store's file name in the cockpit home.
pub const ASSETS_FILE: &str = "bubble-assets.toml";

/// The store's format version; a file from another version is not read.
pub const FORMAT_VERSION: u32 = 1;

const fn format_version() -> u32 {
    FORMAT_VERSION
}

/// One asset's bubble settings.
///
/// Field order matters: TOML requires plain values before the `look` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetBubbles {
    /// The flow pane draws one aggression bubble per tick candle.
    #[serde(default)]
    pub candle_aggression: bool,
    /// The candles' regional FLOW bubbles leave the opening burst out of
    /// their scale.
    #[serde(default)]
    pub flow_ignore_opening: bool,
    /// Every panel value, named after the preset it started from: the look,
    /// the tape mode (`live_lane.native_tape`, `live_lane.tape_only`) and the
    /// tape's opening-burst scale.
    pub look: BubblePreset,
}

impl AssetBubbles {
    /// The settings `preset` opens an asset on, before anything is changed.
    #[must_use]
    pub fn declared(preset: &BubblePreset, candle_aggression: bool) -> Self {
        let mut config = HeatmapConfig::default();
        preset.apply_to(&mut config);
        Self {
            candle_aggression,
            flow_ignore_opening: false,
            look: BubblePreset::capture(&preset.name, &config),
        }
    }
}

/// The store as a whole.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetBubblesFile {
    #[serde(default = "format_version")]
    pub version: u32,
    /// Settings by asset key, in key order.
    #[serde(default)]
    pub assets: BTreeMap<String, AssetBubbles>,
}

impl Default for AssetBubblesFile {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            assets: BTreeMap::new(),
        }
    }
}

impl AssetBubblesFile {
    /// The settings stored for `asset`, if the trader changed any.
    #[must_use]
    pub fn get(&self, asset: &str) -> Option<&AssetBubbles> {
        self.assets.get(asset)
    }

    /// Store `current` for `asset`, or forget the entry when `current` is
    /// the look the asset is declared to open on. Reports whether the file
    /// changed.
    pub fn record(&mut self, asset: &str, current: &AssetBubbles, declared: &AssetBubbles) -> bool {
        if current == declared {
            return self.assets.remove(asset).is_some();
        }
        if self.assets.get(asset) == Some(current) {
            return false;
        }
        self.assets.insert(asset.to_owned(), current.clone());
        true
    }
}

/// Where an asset's open settings came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetSource {
    /// The trader's own settings for this asset, from the store.
    Stored,
    /// The preset the feed config declares for this asset.
    Declared,
    /// The presets file's active look: nothing is declared or stored.
    Default,
}

impl AssetSource {
    /// The wire name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Declared => "declared",
            Self::Default => "default",
        }
    }
}

/// One view's binding to the asset it shows: which asset, where its settings
/// came from, and what was last filed for it.
#[derive(Debug, Clone, PartialEq)]
pub struct AssetTrack {
    key: String,
    /// Where [`Self::declared`] came from.
    declared_source: AssetSource,
    declared: AssetBubbles,
    filed: AssetBubbles,
}

impl AssetTrack {
    /// Resolve the settings `asset` opens on: its stored entry, else the
    /// preset declared for it, else the presets file's active look, else the
    /// code defaults. An unknown declared name falls through to the active
    /// look; the caller reports it.
    #[must_use]
    pub fn resolve(
        asset: BubbleAsset,
        presets: &BubblePresetFile,
        store: &AssetBubblesFile,
        candle_aggression: bool,
    ) -> (Self, AssetBubbles) {
        let declared_preset = asset.preset.as_deref().and_then(|name| presets.get(name));
        let (preset, source) = match declared_preset {
            Some(preset) => (preset.clone(), AssetSource::Declared),
            None => (
                presets.get(&presets.active).cloned().unwrap_or_else(|| {
                    BubblePreset::capture(String::new(), &HeatmapConfig::default())
                }),
                AssetSource::Default,
            ),
        };
        let declared = AssetBubbles::declared(&preset, candle_aggression);
        let settings = store.get(&asset.key).unwrap_or(&declared).clone();
        let track = Self {
            key: asset.key,
            declared_source: source,
            declared,
            filed: settings.clone(),
        };
        (track, settings)
    }

    /// The asset's key in the store.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Where the settings last filed came from: the asset's own once they
    /// differ from the declared look.
    #[must_use]
    pub fn source(&self) -> AssetSource {
        if self.filed == self.declared {
            self.declared_source
        } else {
            AssetSource::Stored
        }
    }

    /// Whether `current` differs from what was last filed for this asset.
    #[must_use]
    pub fn changed(&self, current: &AssetBubbles) -> bool {
        &self.filed != current
    }

    /// File `current` for this asset in `store`, reporting whether the store
    /// changed and must be written.
    pub fn file(&mut self, current: &AssetBubbles, store: &mut AssetBubblesFile) -> bool {
        self.filed = current.clone();
        store.record(&self.key, current, &self.declared)
    }
}

/// [`AssetTrack::resolve`] against the store at `path`, with the reason the
/// store could not be read, if it could not.
#[must_use]
pub fn resolve_at(
    path: &Path,
    asset: BubbleAsset,
    presets: &BubblePresetFile,
    candle_aggression: bool,
) -> (AssetTrack, AssetBubbles, Option<String>) {
    let (store, error) = load(path);
    let (track, settings) = AssetTrack::resolve(asset, presets, &store, candle_aggression);
    (track, settings, error)
}

/// File `current` for `track`'s asset in the store at `path`, writing only
/// when the store changed. An unreadable store is left for the trader to
/// see, never replaced.
///
/// # Errors
///
/// Returns why the store could not be read or written.
pub fn file_at(path: &Path, track: &mut AssetTrack, current: &AssetBubbles) -> Result<(), String> {
    let (mut store, error) = load(path);
    if let Some(error) = error {
        return Err(error);
    }
    if track.file(current, &mut store) {
        save_to(path, &store)?;
    }
    Ok(())
}

/// Parse the store, sanitizing every look the way the presets file does.
///
/// # Errors
///
/// Returns the parser's message when the text is not a store of this version.
pub fn parse(text: &str) -> Result<AssetBubblesFile, String> {
    let mut file: AssetBubblesFile = toml::from_str(text).map_err(|error| error.to_string())?;
    if file.version != FORMAT_VERSION {
        return Err(format!(
            "bubble assets file version {} is not {FORMAT_VERSION}",
            file.version
        ));
    }
    for settings in file.assets.values_mut() {
        settings.look.sanitize();
    }
    Ok(file)
}

/// Whether `text` is a store this build reads — the bundle import gate.
///
/// # Errors
///
/// Returns why it is not.
pub fn validate(text: &str) -> Result<(), String> {
    parse(text).map(|_| ())
}

/// Render the store as the exact text a save writes.
///
/// # Errors
///
/// Returns the serializer's message.
pub fn render(file: &AssetBubblesFile) -> Result<String, String> {
    let body = toml::to_string(file).map_err(|error| error.to_string())?;
    Ok(format!("{FILE_HEADER}{body}"))
}

/// Read the store at `path`: empty when there is none, and the reason
/// alongside an empty store when it cannot be read.
#[must_use]
pub fn load(path: &Path) -> (AssetBubblesFile, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok(file) => (file, None),
            Err(message) => (
                AssetBubblesFile::default(),
                Some(format!("{}: {message}", path.display())),
            ),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (AssetBubblesFile::default(), None)
        }
        Err(error) => (
            AssetBubblesFile::default(),
            Some(format!("cannot read {}: {error}", path.display())),
        ),
    }
}

/// Write the store to `path` through a temporary sibling, creating its
/// folder when missing.
///
/// # Errors
///
/// Returns a human-readable message when it cannot be written.
pub fn save_to(path: &Path, file: &AssetBubblesFile) -> Result<(), String> {
    quantick_workspace::write_refusal::guard_write(path)?;
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, render(file)?)
        .and_then(|()| std::fs::rename(&temp, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temp);
            format!("{}: {error}", path.display())
        })
}

const FILE_HEADER: &str = "\
# quantick — bubble settings per asset, written by the app.
#
# One entry per asset whose bubble settings were changed from the look the
# feed config declares for it (`symbol_bubble_presets` in feeds.toml). Delete
# an entry and that asset opens on its declared look again.

";
