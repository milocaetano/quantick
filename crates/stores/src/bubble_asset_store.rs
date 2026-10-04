//! Every asset's bubble settings while the app runs, and each view's binding
//! to the asset it shows.
//!
//! One [`AssetBubblesStore`] per app, shared by every view: an edit filed by
//! one tab reaches every other tab on the same asset ([`AssetBinding::adoption`])
//! instead of being overwritten by their stale copy. The store is read once
//! and written once per change ([`AssetBubblesStore::flush`]), never per
//! frame — a failed write is retried after a growing number of frames — and
//! it says, per asset, when the file does not hold what is on screen
//! ([`AssetBubblesStore::unsaved`]). A presets file one view reloads reaches
//! every view ([`AssetBinding::publish_presets`]). The document and its I/O
//! are [`crate::bubble_assets`].
//!
//! A setting is something the trader set: the panel, a menu, a control call,
//! a layer switch. Wheeling or dragging the tape's window or width moves the
//! view, not the asset: the lane's width and window are filed only when the
//! caller says they were set on purpose ([`AssetBinding::note_lane_set`]).

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

/// The most flush opportunities — frames — a failing disk waits between two
/// write attempts. The wait doubles from one up to this.
pub const MAX_WRITE_BACKOFF: u32 = 512;

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
    /// The assets changed since the file last held every one.
    unwritten: BTreeSet<String>,
    /// A change waiting for [`Self::flush`].
    pending: bool,
    /// Flushes to let pass before retrying a failed write, and the wait the
    /// next failure doubles, so a failing disk is not tried every frame.
    retry_in: u32,
    backoff: u32,
    /// Why the last write failed.
    write_error: Option<String>,
    /// Counts changes; an asset's revision is the count at its last one.
    changes: u64,
    /// The count at the last reload, which changed every asset at once.
    reloaded_at: u64,
    revisions: BTreeMap<String, u64>,
    /// The presets file a view last reloaded, and how many reloads there
    /// were: every other view takes it at its next sync.
    presets: Option<BubblePresetFile>,
    presets_revision: u64,
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
        self.unwritten.clear();
        self.pending = false;
        (self.retry_in, self.backoff) = (0, 0);
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
        self.unwritten.insert(key.to_owned());
        self.pending = true;
        true
    }

    /// Write the change waiting: `None` when none waited or a failed write
    /// is still backing off, else the attempt's outcome, logged when it
    /// failed. A failed write is retried after 1, 2, 4 … up to
    /// [`MAX_WRITE_BACKOFF`] flushes; an unreadable file is never written
    /// over, and neither it nor a memory-only store is retried.
    pub fn flush(&mut self) -> Option<Result<(), String>> {
        if !self.pending {
            return None;
        }
        if self.retry_in > 0 {
            self.retry_in -= 1;
            return None;
        }
        Some(self.write())
    }

    /// Write what the file does not hold now, backoff or not — a workspace
    /// export captures the file next. Returns why the file still does not
    /// hold every asset's settings; `None` when it does.
    pub fn write_now(&mut self) -> Option<String> {
        if self.pending {
            let _ = self.write();
        }
        self.unsaved_any()
    }

    fn write(&mut self) -> Result<(), String> {
        let result = match (&self.unreadable, &self.path) {
            (Some(error), _) => Err(format!("not written over an unreadable store — {error}")),
            (None, None) => Err("kept in memory only".to_owned()),
            (None, Some(path)) => bubble_assets::save_to(path, &self.file),
        };
        match &result {
            Ok(()) => {
                self.unwritten.clear();
                self.pending = false;
                self.backoff = 0;
                self.write_error = None;
            }
            Err(error) => {
                tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                    event_code = "BUBBLE_ASSETS_NOT_SAVED", error = error.as_str(),
                    action = "keep_settings_in_memory_only",
                    "bubble asset settings could not be saved");
                self.write_error = Some(error.clone());
                if self.unreadable.is_some() || self.path.is_none() {
                    self.pending = false;
                } else {
                    self.backoff = self.backoff.saturating_mul(2).clamp(1, MAX_WRITE_BACKOFF);
                    self.retry_in = self.backoff;
                }
            }
        }
        result
    }

    /// Why the file does not hold `key`'s settings as memory has them;
    /// `None` when it does. Another asset's failed write is not this one's.
    #[must_use]
    pub fn unsaved(&self, key: &str) -> Option<String> {
        if let Some(error) = &self.unreadable {
            return Some(format!("store unreadable — {error}"));
        }
        self.unwritten.contains(key).then(|| {
            self.write_error
                .clone()
                .unwrap_or_else(|| "not written yet".to_owned())
        })
    }

    /// Why the file does not hold every asset's settings; `None` when it
    /// does.
    #[must_use]
    pub fn unsaved_any(&self) -> Option<String> {
        if let Some(error) = &self.unreadable {
            return Some(format!("store unreadable — {error}"));
        }
        let keys = self
            .unwritten
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        (!keys.is_empty()).then(|| {
            let why = self.write_error.as_deref().unwrap_or("not written yet");
            format!("{} — {why}", keys.join(", "))
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
    /// The store's presets revision this view's presets reflect: a view
    /// takes every reload at its next sync, so it binds current.
    presets_seen: u64,
    /// The trader changed a setting since the last filing.
    edited: bool,
    /// Among those changes, the lane's width or window, set on purpose.
    lane_set: bool,
}

impl AssetBinding {
    /// Bind to the asset `symbol` belongs to in a tab on `feed_id`
    /// ([`AppConfig::bubble_asset`]). The settings to put on screen are its
    /// stored ones, else the preset declared for it, else the presets file's
    /// active look, else the code defaults. An unknown declared name is
    /// reported and falls through — the presets file is user-edited, and a
    /// typo must not restyle a market.
    #[must_use]
    pub fn bind(
        store: SharedAssetBubbles,
        config: &AppConfig,
        (feed_id, symbol): (&str, &str),
        presets: &BubblePresetFile,
        candle_default: bool,
    ) -> (Self, AssetBubbles) {
        let asset = config.bubble_asset(feed_id, symbol);
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
        let (settings, seen, presets_seen) = {
            let shared = store.borrow();
            let settings = shared.get(&asset.key).unwrap_or(&declared).clone();
            (
                settings,
                shared.revision(&asset.key),
                shared.presets_revision,
            )
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
            presets_seen,
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
    /// does. A change noted but not filed yet is not saved either.
    #[must_use]
    pub fn unsaved(&self) -> Option<String> {
        if self.edited {
            return Some("changed on screen, not filed yet".to_owned());
        }
        self.store.borrow().unsaved(&self.asset.key)
    }

    /// What this view last filed or adopted.
    #[must_use]
    pub fn filed(&self) -> &AssetBubbles {
        &self.filed
    }

    /// A setting changed: the panel, a menu, a control call or a layer
    /// switch — never a gesture, which moves the view and is not noted at
    /// all. The lane's width and window are not filed by this; see
    /// [`Self::note_lane_set`].
    pub fn note_edit(&mut self) {
        self.edited = true;
    }

    /// The lane's width or window was set on purpose — a menu entry, the
    /// panel, a preset — even to the value navigation already shows: the
    /// next filing keeps the lane as it is on screen.
    pub fn note_lane_set(&mut self) {
        self.edited = true;
        self.lane_set = true;
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
        let key = self.asset.key.as_str();
        if store.reloaded_at > self.seen {
            // An import rewrote the store: the file is the newer word, and
            // the adoption that follows puts it on screen.
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "ASSET_BUBBLES_EDIT_SUPERSEDED", asset = key, by = "import",
                action = "adopt_imported_settings",
                "an unfiled bubble edit gives way to the imported settings");
            return false;
        }
        if store.revision(key) != self.seen {
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "ASSET_BUBBLES_EDIT_SUPERSEDED", asset = key, by = "this_view",
                action = "last_writer_wins",
                "another tab's bubble edit on this asset is replaced by this tab's newer one");
        }
        let changed = store.record(key, &current, &self.declared);
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
        let lane_moved = lane_moved(&self.filed, &settings);
        self.seen = revision;
        self.edited = false;
        self.lane_set = false;
        self.filed = settings.clone();
        Some((settings, lane_moved))
    }

    /// Hand `presets`, reloaded by this view, to every other view on the
    /// store; each takes it at its next sync
    /// ([`Self::take_reloaded_presets`]).
    pub fn publish_presets(&mut self, presets: &BubblePresetFile) {
        let mut store = self.store.borrow_mut();
        store.presets = Some(presets.clone());
        store.presets_revision += 1;
        self.presets_seen = store.presets_revision;
    }

    /// The presets file another view reloaded since this one last looked,
    /// for this view to hold and [`Self::refresh_declared`] from.
    pub fn take_reloaded_presets(&mut self) -> Option<BubblePresetFile> {
        let store = self.store.borrow();
        if store.presets_revision == self.presets_seen {
            return None;
        }
        self.presets_seen = store.presets_revision;
        store.presets.clone()
    }

    /// Read the declared look again from `presets`, reloaded here or
    /// elsewhere, so filing compares against the look the asset opens on
    /// now. Returns it, with whether it moves the lane's width or window,
    /// when the asset was on its declared look — nothing of its own stored,
    /// nothing edited — for the view to put on screen: the presets file's
    /// new word reaches an asset nobody tuned, and only that one.
    pub fn refresh_declared(&mut self, presets: &BubblePresetFile) -> Option<(AssetBubbles, bool)> {
        let untuned = !self.edited && self.store.borrow().get(&self.asset.key).is_none();
        (self.declared_source, self.declared) = declared(&self.asset, presets, self.candle_default);
        untuned.then(|| {
            let moved = lane_moved(&self.filed, &self.declared);
            self.filed = self.declared.clone();
            (self.declared.clone(), moved)
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

/// Whether `to` puts the lane's width or window elsewhere than `from`.
fn lane_moved(from: &AssetBubbles, to: &AssetBubbles) -> bool {
    let (was, is) = (&from.look.live_lane, &to.look.live_lane);
    was.window != is.window || was.width_share != is.width_share
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
