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
//!
//! "Save changes for this asset" is the asset's own switch, stored with its
//! settings ([`AssetBubblesStore::saves_changes`]). Off, a change stays on
//! the screen it was made on for the session: it is not filed, so it is not
//! written and no other tab wears it — another tab on the asset shows what
//! is stored, and so does this one once it binds again or the app restarts.
//! Switched on again, the screen of the view that switched it is filed and
//! every other view on the asset wears that ([`AssetBinding::set_save_changes`])
//! — unless the store moved since that view last looked, and then the store's
//! word wins there too. The switch reports which happened ([`SaveSwitch`]),
//! and a view whose held changes give way says so ([`Adoption::dropped`]).
//!
//! A launch hook is not a trader's edit: what one asks for — the tape's
//! window, the aggression bubbles — is held on the view for the run and
//! never filed ([`RunHold`]).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use quantick_orderflow::{HeatmapConfig, LaneWindow};

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

    /// Whether a change to `key`'s settings is filed and written.
    #[must_use]
    pub fn saves_changes(&self, key: &str) -> bool {
        self.file.saves_changes(key)
    }

    /// Switch saving `key`'s changes on or off, written like a change.
    /// Switching it on moves the asset's revision, so every view on it
    /// drops what it held unsaved and wears what is filed next. Reports
    /// whether the switch moved.
    pub fn set_save_changes(&mut self, key: &str, on: bool) -> bool {
        if !self.file.set_save_changes(key, on) {
            return false;
        }
        self.changes += 1;
        if on {
            self.revisions.insert(key.to_owned(), self.changes);
        }
        self.unwritten.insert(key.to_owned());
        self.pending = true;
        tracing::info!(target: "quantick::app", schema_version = 1_u8,
            event_code = "ASSET_BUBBLES_SAVE_SWITCHED", asset = key, on,
            action = if on { "file_the_screen" } else { "keep_changes_on_screen_only" },
            "saving bubble changes for the asset switched");
        true
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
    /// What this view shows that differs from [`Self::filed`] while saving
    /// is off for the asset: held on this screen, never filed.
    held: Option<AssetBubbles>,
    /// Saving was switched on here while the store held something newer than
    /// this view last saw: what it held gives way at the next adoption, and
    /// nothing of it is filed before.
    gives_way: bool,
}

/// What a view wears when another view filed for its asset, or a reload
/// brought the asset's settings ([`AssetBinding::adoption`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Adoption {
    pub settings: AssetBubbles,
    /// They put the lane's width or window elsewhere.
    pub lane_moved: bool,
    /// Changes this view held with saving off are gone — logged as
    /// `ASSET_BUBBLES_HELD_DROPPED`, for the view to tell the trader.
    pub dropped: bool,
}

impl Adoption {
    /// What the view tells the trader when changes it held gave way.
    #[must_use]
    pub fn notice(&self) -> Option<String> {
        self.dropped
            .then(|| "changes held on this tab were dropped for the saved ones".to_owned())
    }
}

/// What switching "Save changes for this asset" did to the switching view's
/// screen ([`AssetBinding::set_save_changes`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveSwitch {
    /// The switch already stood there: nothing moved.
    Unmoved,
    /// Switched off: the screen stays, and later changes are held on it.
    Off,
    /// Switched on: what the view shows is filed for the asset at its next
    /// filing, and every other view on the asset wears it.
    ScreenStored,
    /// Switched on while the store held something newer than this view last
    /// saw — another tab's filing, an import: the view's screen gives way to
    /// the stored settings, and nothing of it is filed.
    StoredTaken,
}

impl SaveSwitch {
    /// Whether the switch moved.
    #[must_use]
    pub fn moved(self) -> bool {
        self != Self::Unmoved
    }

    /// What the view tells the trader; `None` when nothing moved.
    #[must_use]
    pub fn status(self) -> Option<&'static str> {
        match self {
            Self::Unmoved => None,
            Self::Off => Some("changes for this asset are kept for this session only"),
            Self::ScreenStored => Some("changes for this asset are saved again"),
            Self::StoredTaken => Some(
                "changes for this asset are saved again · this tab took the saved settings, \
                 changed elsewhere meanwhile",
            ),
        }
    }
}

/// What launch hooks hold on a view for one run — the tape's window, the
/// aggression bubbles switch: put back on every asset the view wears, so a
/// hook reaches the asset a replay autostart switches to, and never filed.
/// An env var is not a trader's edit. Empty outside a capture run.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RunHold {
    pub window: Option<LaneWindow>,
    pub bubbles: Option<bool>,
}

impl RunHold {
    /// Hold what `hold` adds for the run, and put everything held on
    /// `config`, the view's screen.
    pub fn add(&mut self, hold: impl FnOnce(&mut Self), config: &mut HeatmapConfig) {
        hold(self);
        self.wear(config);
    }

    /// Put what is held on `config`, the view's screen.
    pub fn wear(&self, config: &mut HeatmapConfig) {
        if let Some(window) = self.window {
            config.live_lane.window = window;
            config.live_lane.window.sanitize();
        }
        if let Some(on) = self.bubbles {
            config.show_aggressions = on;
        }
    }

    /// A setting changed on purpose from `before` to `after`: a bubbles
    /// switch the trader set is theirs from now on, no longer held.
    pub fn release(&mut self, before: &HeatmapConfig, after: &HeatmapConfig) {
        if before.show_aggressions != after.show_aggressions {
            self.bubbles = None;
        }
    }

    /// The bubbles switch to file for a screen showing `shown`: while a
    /// hook holds it, what `binding` last filed or held instead.
    #[must_use]
    pub fn filed(&self, shown: bool, binding: Option<&AssetBinding>) -> bool {
        match (self.bubbles, binding) {
            (Some(_), Some(binding)) => binding.on_screen().bubbles,
            _ => shown,
        }
    }
}

/// What a workspace export leaves out, as the caveat its report ends on:
/// settings the file does not hold (`unsaved`, from
/// [`AssetBubblesStore::write_now`]) and changes `views` hold with saving
/// off, which are never filed. Empty when the bundle holds everything.
pub fn export_caveat<'a>(
    unsaved: Option<String>,
    views: impl Iterator<Item = &'a AssetBinding>,
) -> String {
    let held = views.filter(|view| view.holds_unsaved());
    let held: BTreeSet<&str> = held.map(AssetBinding::key).collect();
    let mut caveat = unsaved.map_or_else(String::new, |why| {
        format!("; bubble settings per asset as last saved, not as on screen ({why})")
    });
    if !held.is_empty() {
        let held = held.into_iter().collect::<Vec<_>>().join(", ");
        caveat += &format!("; not included: changes held with saving off for {held}");
    }
    caveat
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
            held: None,
            gives_way: false,
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
    /// does. The store's own failure comes first — an unreadable file, a
    /// write refused, the save switch's own included — then a change noted
    /// but not filed yet, or held because saving is off.
    #[must_use]
    pub fn unsaved(&self) -> Option<String> {
        let screen = if self.holds_unsaved() {
            Some(format!(
                "saving is off for {} — the changes on screen last this session only",
                self.asset.key
            ))
        } else {
            (self.edited || self.held.is_some())
                .then(|| "changed on screen, not filed yet".to_owned())
        };
        match (self.store.borrow().unsaved(&self.asset.key), screen) {
            (Some(store), Some(screen)) => Some(format!("{store}; {screen}")),
            (store, screen) => store.or(screen),
        }
    }

    /// Whether this view shows changes saving is off for: held on this
    /// screen for the session, in no file and no export.
    #[must_use]
    pub fn holds_unsaved(&self) -> bool {
        !self.saves_changes() && (self.edited || self.held.is_some())
    }

    /// What this view last filed or adopted.
    #[must_use]
    pub fn filed(&self) -> &AssetBubbles {
        &self.filed
    }

    /// What this view showed at its last filing: what it holds unsaved
    /// while saving is off, else what it filed or adopted.
    #[must_use]
    pub fn on_screen(&self) -> &AssetBubbles {
        self.held.as_ref().unwrap_or(&self.filed)
    }

    /// Whether a change to the asset's settings is filed — the asset's
    /// "Save changes for this asset", the same in every tab on it.
    #[must_use]
    pub fn saves_changes(&self) -> bool {
        self.store.borrow().saves_changes(&self.asset.key)
    }

    /// Switch saving the asset's changes on or off. Switched on, this view's
    /// screen is filed at its next filing — the lane included when it was
    /// set on purpose while saving was off — and every other view on the
    /// asset drops what it held and wears that. When the store moved since
    /// this view last looked — an import, another tab's filing — the store
    /// is the newer word: this view drops what it held and wears it instead
    /// ([`Self::adoption`]). Reports which happened.
    pub fn set_save_changes(&mut self, on: bool) -> SaveSwitch {
        let mut store = self.store.borrow_mut();
        let behind = store.revision(&self.asset.key) != self.seen;
        if !store.set_save_changes(&self.asset.key, on) {
            return SaveSwitch::Unmoved;
        }
        if !on {
            return SaveSwitch::Off;
        }
        if behind {
            self.gives_way = true;
            return SaveSwitch::StoredTaken;
        }
        // This view's screen is the one to file, not one to give way.
        self.seen = store.revision(&self.asset.key);
        self.edited = true;
        SaveSwitch::ScreenStored
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
    /// and window stay as filed — or as held, with saving off — unless they
    /// were set on purpose since: navigation is never filed nor held.
    /// Reports whether the store changed. With saving off nothing is filed:
    /// what differs is held on this screen ([`Self::on_screen`]).
    pub fn file(&mut self, mut current: AssetBubbles) -> bool {
        if !self.lane_set {
            let shown = &self.on_screen().look.live_lane;
            current.look.live_lane.window = shown.window;
            current.look.live_lane.width_share = shown.width_share;
        }
        self.edited = false;
        self.lane_set = false;
        if !self.saves_changes() {
            self.held = (current != self.filed).then_some(current);
            return false;
        }
        let mut store = self.store.borrow_mut();
        let key = self.asset.key.as_str();
        if self.gives_way || store.reloaded_at > self.seen {
            // An import rewrote the store, or saving came on here behind
            // another tab's filing: the store is the newer word, and the
            // adoption that follows puts it on screen.
            let by = if self.gives_way { "store" } else { "import" };
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "ASSET_BUBBLES_EDIT_SUPERSEDED", asset = key, by,
                action = "adopt_stored_settings",
                "an unfiled bubble edit gives way to the asset's stored settings");
            return false;
        }
        self.held = None;
        if current == self.filed {
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
    /// filed or adopted — or that a reload brought. Edits not yet filed
    /// here, and changes held with saving off, give way to them. `None` when
    /// nothing changed.
    pub fn adoption(&mut self) -> Option<Adoption> {
        let store = self.store.borrow();
        let revision = store.revision(&self.asset.key);
        if revision == self.seen {
            return None;
        }
        let settings = store.get(&self.asset.key).unwrap_or(&self.declared).clone();
        drop(store);
        let lane_moved = lane_moved(self.on_screen(), &settings);
        let dropped = self.held.take().is_some_and(|held| held != settings);
        if dropped {
            tracing::warn!(target: "quantick::app", schema_version = 1_u8,
                event_code = "ASSET_BUBBLES_HELD_DROPPED", asset = self.asset.key.as_str(),
                action = "wear_stored_settings",
                "bubble changes a tab held with saving off gave way to the asset's stored settings");
        }
        self.seen = revision;
        self.edited = false;
        self.lane_set = false;
        self.gives_way = false;
        self.filed = settings.clone();
        Some(Adoption {
            settings,
            lane_moved,
            dropped,
        })
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
        let untuned = !self.edited
            && self.held.is_none()
            && self.store.borrow().get(&self.asset.key).is_none();
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
#[path = "bubble_asset_store_save_tests.rs"]
mod bubble_asset_store_save_tests;
#[cfg(test)]
#[path = "bubble_asset_store_tests.rs"]
pub(crate) mod bubble_asset_store_tests;
