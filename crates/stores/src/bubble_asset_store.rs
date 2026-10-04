//! Every asset's bubble settings while the app runs, and each view's binding
//! to the asset it shows.
//!
//! One [`AssetBubblesStore`] per app, shared by every view: an edit filed by
//! one tab reaches every other tab on the same asset ([`AssetBinding::adoption`])
//! instead of being overwritten by their stale copy. The store is read once
//! and written once per change ([`AssetBubblesStore::flush`]), never per
//! frame, and it says when the file does not hold what is on screen
//! ([`AssetBubblesStore::unsaved`]). The document and its I/O are
//! [`crate::bubble_assets`].
//!
//! A setting is something the trader set: the panel, a menu, a control call,
//! a layer switch. Wheeling or dragging the tape's window or width moves the
//! view, not the asset: the lane's width and window are filed only when set
//! on purpose ([`AssetBinding::note_edit`]).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use quantick_orderflow::HeatmapConfig;

use crate::bubble_assets::{self, AssetBubbles, AssetBubblesFile, AssetSource};
use crate::bubble_presets::{BubblePreset, BubblePresetFile};
use crate::config::{AppConfig, BubbleAsset};

/// The store every view of one app shares.
pub type SharedAssetBubbles = Rc<RefCell<AssetBubblesStore>>;

/// Every asset's own bubble settings, in memory, and whether the file holds
/// them.
#[derive(Debug, Default)]
pub struct AssetBubblesStore {
    /// Where the store is written; `None` keeps it in memory only.
    path: Option<PathBuf>,
    file: AssetBubblesFile,
    /// Why the file could not be read. While set it is never written over:
    /// edits stay in memory and every asset reports them unsaved.
    unreadable: Option<String>,
    /// A change the file does not hold yet.
    unwritten: bool,
    /// A change waiting for [`Self::flush`]. The attempt clears it, so a
    /// failing disk is tried once per change, not once per frame.
    pending: bool,
    /// Why the last write failed.
    write_error: Option<String>,
    /// Counts changes; an asset's revision is the count at its last one.
    changes: u64,
    /// The count at the last reload, which changed every asset at once.
    reloaded_at: u64,
    revisions: BTreeMap<String, u64>,
}

impl AssetBubblesStore {
    /// Read the store at `path`; a missing file is an empty store.
    #[must_use]
    pub fn load(path: PathBuf) -> Self {
        let mut store = Self {
            path: Some(path),
            ..Self::default()
        };
        store.read();
        store
    }

    /// The store, shared by every view of one app.
    #[must_use]
    pub fn shared(self) -> SharedAssetBubbles {
        Rc::new(RefCell::new(self))
    }

    /// Read the file again — a workspace import wrote it — and make every
    /// view adopt its asset's settings from it at its next
    /// [`AssetBinding::adoption`]. What memory held unsaved gives way: the
    /// file is the newer word.
    pub fn reload(&mut self) {
        self.read();
        self.changes += 1;
        self.reloaded_at = self.changes;
    }

    fn read(&mut self) {
        let Some(path) = &self.path else {
            return;
        };
        let (file, unreadable) = bubble_assets::load(path);
        if let Some(error) = &unreadable {
            tracing::error!(target: "quantick::app", schema_version = 1_u8,
                event_code = "BUBBLE_ASSETS_UNREADABLE", error = error.as_str(),
                action = "keep_edits_in_memory_only", "bubble asset settings could not be read");
        }
        self.file = file;
        self.unreadable = unreadable;
        self.unwritten = false;
        self.pending = false;
        self.write_error = None;
    }

    /// The settings stored for `key`, if the trader changed any.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&AssetBubbles> {
        self.file.get(key)
    }

    fn revision(&self, key: &str) -> u64 {
        self.revisions
            .get(key)
            .copied()
            .unwrap_or(0)
            .max(self.reloaded_at)
    }

    /// Store `current` as `key`'s settings, or forget the entry when it is
    /// `declared`. Reports whether the store changed.
    pub fn record(&mut self, key: &str, current: &AssetBubbles, declared: &AssetBubbles) -> bool {
        if !self.file.record(key, current, declared) {
            return false;
        }
        self.changes += 1;
        self.revisions.insert(key.to_owned(), self.changes);
        self.unwritten = true;
        self.pending = true;
        true
    }

    /// Write the change waiting, once: `None` when none waited, else the
    /// attempt's outcome, logged when it failed. An unreadable file is never
    /// written over.
    pub fn flush(&mut self) -> Option<Result<(), String>> {
        if !std::mem::take(&mut self.pending) {
            return None;
        }
        let result = match (&self.unreadable, &self.path) {
            (Some(error), _) => Err(format!("not written over an unreadable store — {error}")),
            (None, None) => Err("kept in memory only".to_owned()),
            (None, Some(path)) => bubble_assets::save_to(path, &self.file),
        };
        match &result {
            Ok(()) => {
                self.unwritten = false;
                self.write_error = None;
            }
            Err(error) => {
                tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                    event_code = "BUBBLE_ASSETS_NOT_SAVED", error = error.as_str(),
                    action = "keep_settings_in_memory_only",
                    "bubble asset settings could not be saved");
                self.write_error = Some(error.clone());
            }
        }
        Some(result)
    }

    /// Why the file does not hold what memory does; `None` when it does.
    #[must_use]
    pub fn unsaved(&self) -> Option<String> {
        if let Some(error) = &self.unreadable {
            return Some(format!("store unreadable — {error}"));
        }
        self.unwritten.then(|| {
            self.write_error
                .clone()
                .unwrap_or_else(|| "not written yet".to_owned())
        })
    }
}

/// One view's binding to the asset it shows.
#[derive(Debug)]
pub struct AssetBinding {
    store: SharedAssetBubbles,
    asset: BubbleAsset,
    /// The flow pane's candle aggression an asset opens with.
    candle_default: bool,
    /// Every preset name some asset opens on, from the feed config.
    opening_presets: BTreeSet<String>,
    declared_source: AssetSource,
    declared: AssetBubbles,
    /// What this view last filed or adopted for the asset.
    filed: AssetBubbles,
    /// The asset's revision in the store when this view last filed or
    /// adopted.
    seen: u64,
    /// The trader changed a setting since the last filing.
    edited: bool,
    /// Among those changes, the lane's width or window, set on purpose.
    lane_set: bool,
}

impl AssetBinding {
    /// Bind to the asset `symbol` belongs to ([`AppConfig::bubble_asset`]).
    /// The settings to put on screen are its stored ones, else the preset
    /// declared for it, else the presets file's active look, else the code
    /// defaults. An unknown declared name is reported and falls through —
    /// the presets file is user-edited, and a typo must not restyle a market.
    #[must_use]
    pub fn bind(
        store: SharedAssetBubbles,
        config: &AppConfig,
        symbol: &str,
        presets: &BubblePresetFile,
        candle_default: bool,
    ) -> (Self, AssetBubbles) {
        let asset = config.bubble_asset(symbol);
        if let Some(name) = asset
            .preset
            .as_deref()
            .filter(|name| presets.get(name).is_none())
        {
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "FEED_BUBBLE_PRESET_UNKNOWN", symbol, preset = name,
                action = "use_default_look",
                "feed declares a bubble preset that is not in the presets file; ignoring");
        }
        let (declared_source, declared) = declared(&asset, presets, candle_default);
        let (settings, seen) = {
            let shared = store.borrow();
            let settings = shared.get(&asset.key).unwrap_or(&declared).clone();
            (settings, shared.revision(&asset.key))
        };
        let binding = Self {
            store,
            asset,
            candle_default,
            opening_presets: config.opening_bubble_presets(),
            declared_source,
            declared,
            filed: settings.clone(),
            seen,
            edited: false,
            lane_set: false,
        };
        tracing::info!(target: "quantick::app", schema_version = 1_u8,
            event_code = "ASSET_BUBBLES_APPLIED", symbol, asset = binding.key(),
            source = binding.source().as_str(), preset = settings.look.name.as_str(),
            action = "apply_asset_settings", "bubble settings of the asset on screen applied");
        (binding, settings)
    }

    /// The asset's key in the store.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.asset.key
    }

    /// The store this binding files into, which the asset a switch arrives
    /// at binds to as well.
    #[must_use]
    pub fn store(&self) -> &SharedAssetBubbles {
        &self.store
    }

    /// Where the asset's settings come from: its own, once anything of it
    /// is stored.
    #[must_use]
    pub fn source(&self) -> AssetSource {
        if self.store.borrow().get(&self.asset.key).is_some() {
            AssetSource::Stored
        } else {
            self.declared_source
        }
    }

    /// Why the file does not hold the settings on screen; `None` when it
    /// does.
    #[must_use]
    pub fn unsaved(&self) -> Option<String> {
        self.store.borrow().unsaved()
    }

    /// What this view last filed or adopted.
    #[must_use]
    pub fn filed(&self) -> &AssetBubbles {
        &self.filed
    }

    /// A setting changed from `before` to `after`: the panel, a menu, a
    /// control call or a layer switch — never a gesture, which moves the
    /// view and is not noted at all.
    pub fn note_edit(&mut self, before: &HeatmapConfig, after: &HeatmapConfig) {
        self.edited = true;
        let (was, is) = (&before.live_lane, &after.live_lane);
        self.lane_set |= was.window != is.window || was.width_share != is.width_share;
    }

    /// A setting outside the config changed: the look's name, the candles'
    /// opening scale.
    pub fn note_change(&mut self) {
        self.edited = true;
    }

    /// Whether a setting changed since the last filing.
    #[must_use]
    pub fn edited(&self) -> bool {
        self.edited
    }

    /// File `current`, what the view shows, for the asset. The lane's width
    /// and window stay as filed unless they were set on purpose since.
    /// Reports whether the store changed.
    pub fn file(&mut self, mut current: AssetBubbles) -> bool {
        if !std::mem::take(&mut self.lane_set) {
            let filed = &self.filed.look.live_lane;
            current.look.live_lane.window = filed.window;
            current.look.live_lane.width_share = filed.width_share;
        }
        self.edited = false;
        if current == self.filed {
            return false;
        }
        let mut store = self.store.borrow_mut();
        let changed = store.record(&self.asset.key, &current, &self.declared);
        self.seen = store.revision(&self.asset.key);
        drop(store);
        self.filed = current;
        changed
    }

    /// The settings another view filed for this asset since this one last
    /// filed or adopted — or that a reload brought — with whether they move
    /// the lane's width or window. Edits not yet filed here give way to them.
    /// `None` when nothing changed.
    pub fn adoption(&mut self) -> Option<(AssetBubbles, bool)> {
        let store = self.store.borrow();
        let revision = store.revision(&self.asset.key);
        if revision == self.seen {
            return None;
        }
        let settings = store.get(&self.asset.key).unwrap_or(&self.declared).clone();
        drop(store);
        let (was, is) = (&self.filed.look.live_lane, &settings.look.live_lane);
        let lane_moved = was.window != is.window || was.width_share != is.width_share;
        self.seen = revision;
        self.edited = false;
        self.lane_set = false;
        self.filed = settings.clone();
        Some((settings, lane_moved))
    }

    /// Read the declared look again from `presets`, reloaded or saved, so
    /// filing compares against the look the asset opens on now. Returns it
    /// when the asset was on its declared look — nothing of its own stored,
    /// nothing edited — for the view to put on screen: the presets file's
    /// new word reaches an asset nobody tuned, and only that one.
    pub fn refresh_declared(&mut self, presets: &BubblePresetFile) -> Option<AssetBubbles> {
        let untuned = !self.edited && self.store.borrow().get(&self.asset.key).is_none();
        (self.declared_source, self.declared) = declared(&self.asset, presets, self.candle_default);
        untuned.then(|| {
            self.filed = self.declared.clone();
            self.declared.clone()
        })
    }

    /// Why saving the look on screen as `name` is refused: some asset opens
    /// on that preset — the presets file's `active` one or a name the feed
    /// config declares — and overwriting it would restyle every one of them.
    #[must_use]
    pub fn refuses_preset_name(&self, name: &str, presets: &BubblePresetFile) -> Option<String> {
        (name == presets.active || self.opening_presets.contains(name)).then(|| {
            format!(
                "'{name}' is a look other assets open on; save under another name — \
                 {}'s own settings are kept for it already",
                self.asset.key
            )
        })
    }
}

/// The look `asset` opens on, and where it comes from.
fn declared(
    asset: &BubbleAsset,
    presets: &BubblePresetFile,
    candle_default: bool,
) -> (AssetSource, AssetBubbles) {
    let (source, preset) = match asset.preset.as_deref().and_then(|name| presets.get(name)) {
        Some(preset) => (AssetSource::Declared, preset.clone()),
        None => (
            AssetSource::Default,
            presets
                .get(&presets.active)
                .cloned()
                .unwrap_or_else(|| BubblePreset::capture(String::new(), &HeatmapConfig::default())),
        ),
    };
    (source, AssetBubbles::declared(&preset, candle_default))
}

#[cfg(test)]
#[path = "bubble_asset_store_tests.rs"]
mod bubble_asset_store_tests;
