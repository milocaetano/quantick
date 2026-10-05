//! Bubble settings per asset.
//!
//! An asset is what [`FeedConfig::bubble_asset`](crate::config::FeedConfig::bubble_asset)
//! names: a symbol, or a family of dated contracts one config key declares
//! (`WIN*`). Each asset keeps its own aggression-bubble settings — the panel's
//! look, the tape mode, the tape's and the candles' opening-burst scale and
//! the flow pane's aggression bubbles and candle aggression switches — so a
//! change made while one market is on
//! screen never dresses another.
//!
//! The config declares the look an asset opens on; this store keeps what the
//! trader changed from there, one entry per asset, in [`ASSETS_FILE`] in the
//! cockpit home. An entry exists only while it differs from the declared look,
//! so a preset updated in `bubbles.toml` still reaches every asset nobody
//! tuned by hand. The running app holds the store in memory and binds each
//! view to its asset through [`crate::bubble_asset_store`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use quantick_orderflow::HeatmapConfig;

use crate::bubble_presets::BubblePreset;

/// The store's file name in the cockpit home.
pub const ASSETS_FILE: &str = "bubble-assets.toml";

/// The store's format version; a file from another version is not read.
pub const FORMAT_VERSION: u32 = 1;

const fn format_version() -> u32 {
    FORMAT_VERSION
}

/// The aggression bubbles layer's default (`bubbles`, Ctrl+B): what an asset
/// opens with, and what an entry written before the asset owned the switch
/// reads. `ChartLayer::Bubbles` in `quantick-layers` declares it; the app
/// pins the two equal, as this crate does not depend on that one.
pub const BUBBLES_LAYER_DEFAULT: bool = true;

const fn bubbles_layer_default() -> bool {
    BUBBLES_LAYER_DEFAULT
}

/// One asset's bubble settings.
///
/// Field order matters: TOML requires plain values before the `look` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetBubbles {
    /// The flow pane draws the aggression bubbles: the chart layer's switch,
    /// the asset's own like candle aggression.
    #[serde(default = "bubbles_layer_default")]
    pub bubbles: bool,
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
            bubbles: BUBBLES_LAYER_DEFAULT,
            candle_aggression,
            flow_ignore_opening: false,
            look: BubblePreset::capture(&preset.name, &config),
        }
    }
}

/// The store as a whole.
///
/// Field order matters: TOML requires plain values before the `assets`
/// tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetBubblesFile {
    #[serde(default = "format_version")]
    pub version: u32,
    /// The assets whose "Save changes for this asset" is off: a change made
    /// while one is on screen stays on that screen for the session and is
    /// never stored. Saving is on for every asset not listed.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub save_changes_off: BTreeSet<String>,
    /// Settings by asset key, in key order.
    #[serde(default)]
    pub assets: BTreeMap<String, AssetBubbles>,
}

impl Default for AssetBubblesFile {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            save_changes_off: BTreeSet::new(),
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

    /// Whether a change to `asset`'s settings is stored.
    #[must_use]
    pub fn saves_changes(&self, asset: &str) -> bool {
        !self.save_changes_off.contains(asset)
    }

    /// Switch storing `asset`'s changes on or off. Reports whether the
    /// switch moved.
    pub fn set_save_changes(&mut self, asset: &str, on: bool) -> bool {
        if on {
            self.save_changes_off.remove(asset)
        } else {
            self.save_changes_off.insert(asset.to_owned())
        }
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
# an entry and that asset opens on its declared look again. An asset listed
# in `save_changes_off` keeps what is changed on screen for the session only.

";
