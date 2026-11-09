//! The market settings document at its resolved path, under the commit policy.
use crate::{
    layout_commit::{LayoutCommitPolicy, LayoutSave},
    market_settings::{
        DrawingKey, MARKET_SETTINGS_FILE, MarketKey, MarketSettings, PaneRole,
        pane_marks::shown_sets,
    },
};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

/// Answers which asset a venue's symbol belongs to. Config, handed in.
pub type AssetResolver = Rc<dyn Fn(&str, &str) -> String>;

/// The per-market document, and the one rule that decides when it reaches disk.
///
/// With autosave off the store is *held*: drawings and anchored studies change
/// in memory and wait for Save ([`Self::commit_document`]); `saved` is what the
/// file holds, so a held change can be counted and dropped. `saved` moves only
/// once a write has reached the disk ([`Self::note_saved`]): a failed write
/// leaves every change counted and owed.
pub struct MarketSettingsStore {
    settings: MarketSettings,
    saved: MarketSettings,
    held: bool,
    /// Moves whenever `settings`, `saved` or the hold may have: a reader that
    /// counts held changes recounts only when it moved.
    revision: u64,
    /// How many times [`Self::held_changes`] compared drawings, for a test
    /// that proves an idle frame compares none.
    counted: Cell<u64>,
    path: PathBuf,
    policy: LayoutCommitPolicy,
    asset: AssetResolver,
}

impl MarketSettingsStore {
    /// A store over a document already loaded, at a path already resolved.
    /// `blocked`: see [`LayoutCommitPolicy::new`].
    #[must_use]
    pub fn new(settings: MarketSettings, path: PathBuf, blocked: bool) -> Self {
        Self {
            saved: settings.clone(),
            settings,
            held: false,
            revision: 0,
            counted: Cell::new(0),
            path,
            policy: LayoutCommitPolicy::new(blocked),
            asset: Rc::new(|_, symbol| symbol.to_owned()),
        }
    }

    /// An empty store beside a layouts file, each symbol its own asset.
    #[must_use]
    pub fn beside(layouts: &Path) -> Self {
        Self::new(
            MarketSettings::default(),
            layouts.with_file_name(MARKET_SETTINGS_FILE),
            false,
        )
    }

    /// Name assets as `asset` does, not by the bare symbol.
    #[must_use]
    pub fn with_assets(mut self, asset: AssetResolver) -> Self {
        self.asset = asset;
        self
    }

    /// The resolver itself, for a caller that also edits the document.
    #[must_use]
    pub fn assets(&self) -> AssetResolver {
        Rc::clone(&self.asset)
    }

    /// A contract roll on `venue` from one symbol to another: their assets'
    /// markets, and how many drawings [`MarketSettings::carry_market`] copied
    /// onto the second.
    pub fn carry_roll(&mut self, venue: &str, from: &str, to: &str) -> (MarketKey, usize) {
        let of = |asset: String| MarketKey {
            venue: venue.to_owned(),
            asset,
        };
        let (from, to) = (of(self.asset(venue, from)), of(self.asset(venue, to)));
        let carried = self.settings.carry_market(&from, &to);
        (to, carried)
    }

    /// The asset `symbol` belongs to on `venue`.
    #[must_use]
    pub fn asset(&self, venue: &str, symbol: &str) -> String {
        (self.asset)(venue, symbol)
    }

    /// The market `symbol` on `venue` is, as this document keys it.
    #[must_use]
    pub fn market_key(&self, venue: &str, symbol: &str) -> MarketKey {
        MarketKey {
            venue: venue.to_owned(),
            asset: self.asset(venue, symbol),
        }
    }

    /// Whether that market's last-price tag carries the forming bar's progress.
    #[must_use]
    pub fn bar_progress_on(&self, venue: &str, symbol: &str) -> bool {
        let market = self.market_key(venue, symbol);
        self.settings.bar_progress_on(&market)
    }

    /// Switch it, and owe the file a save when that changed anything; `true`
    /// when it did, so a repeated call is no change and no save.
    pub fn set_bar_progress(&mut self, venue: &str, symbol: &str, on: bool, now: Instant) -> bool {
        let market = self.market_key(venue, symbol);
        let changed = self.settings.set_bar_progress(&market, on);
        if changed {
            self.policy.mark_changed(now);
        }
        changed
    }

    #[must_use]
    pub fn settings(&self) -> &MarketSettings {
        &self.settings
    }
    pub fn settings_mut(&mut self) -> &mut MarketSettings {
        self.revision += 1;
        &mut self.settings
    }

    /// Replace the document wholesale — an imported bundle landing.
    pub fn set_settings(&mut self, settings: MarketSettings) {
        self.revision += 1;
        self.saved = settings.clone();
        self.settings = settings;
    }

    /// Moves whenever what [`Self::held_changes`] answers may have.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// How many sets [`Self::held_changes`] has compared so far.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn held_counts(&self) -> u64 {
        self.counted.get()
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Redirect a fixture without exposing the commit policy.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_path(&mut self, path: PathBuf) {
        self.path = path;
    }

    pub fn set_blocked(&mut self, blocked: bool) {
        self.policy.set_blocked(blocked);
    }
    #[must_use]
    pub fn is_blocked(&self) -> bool {
        self.policy.is_blocked()
    }
    pub fn settle(&mut self, changed: bool, now: Instant) {
        self.policy.settle(changed, now);
    }
    pub fn mark_changed(&mut self, now: Instant) {
        self.policy.mark_changed(now);
    }
    /// The debounced write of the whole document; nothing while held. On
    /// `Write` the caller writes [`Self::settings`] and reports the outcome
    /// with [`Self::note_saved`] or [`Self::note_write_failed`].
    pub fn take_save(&mut self, now: Instant) -> LayoutSave {
        if self.held {
            return LayoutSave::Wait;
        }
        self.policy.take_save(now)
    }
    /// The write on exit or before an export; nothing while held.
    pub fn take_flush(&mut self) -> LayoutSave {
        if self.held {
            return LayoutSave::Wait;
        }
        self.policy.take_flush()
    }

    /// Save for one tab: the file as it stands, with only `keys` taken from
    /// memory, so a set another tab holds back stays held. `None` when those
    /// keys hold nothing the file lacks.
    #[must_use]
    pub fn commit_document(&self, keys: &[DrawingKey]) -> Option<MarketSettings> {
        let mut document = self.saved.clone();
        for key in &shown_sets(keys) {
            document.restore(&self.settings, key);
            if key.role == PaneRole::AllTimeframes {
                document.carry_retirement(&self.settings, &key.market);
            }
        }
        (document != self.saved).then_some(document)
    }

    /// Whether memory holds anything the file lacks.
    #[must_use]
    pub fn owes_write(&self) -> bool {
        self.settings != self.saved
    }

    /// `written` reached the disk: it is what the file holds now. Once memory
    /// holds nothing more, nothing is owed.
    pub fn note_saved(&mut self, written: MarketSettings) {
        self.revision += 1;
        self.saved = written;
        if self.settings == self.saved {
            // The pending change is consumed: the file holds it.
            let _ = self.policy.take_flush();
        }
    }

    /// A write did not reach the disk: the change is owed again, and the
    /// debounce retries it a quiet second after `now`.
    pub fn note_write_failed(&mut self, now: Instant) {
        self.policy.mark_changed(now);
    }

    /// Hold writes until Save (autosave off), or let them through.
    pub fn set_held(&mut self, held: bool) {
        if self.held != held {
            self.revision += 1;
        }
        self.held = held;
    }
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// How many drawings on `keys` wait for Save; none unless held.
    #[must_use]
    pub fn held_changes<'a>(&self, keys: impl IntoIterator<Item = &'a DrawingKey>) -> usize {
        if !self.held {
            return 0;
        }
        shown_sets(keys)
            .iter()
            .inspect(|_| self.counted.set(self.counted.get() + 1))
            .map(|key| self.settings.drawing_changes(&self.saved, key))
            .sum()
    }

    /// Close without saving: put every key in `closed` that no pane in `open`
    /// still shows back to the file's version. Once nothing differs, nothing is
    /// owed. Answers whether anything was dropped.
    pub fn discard(&mut self, closed: &[DrawingKey], open: &[DrawingKey]) -> bool {
        self.revision += 1;
        let mut dropped = false;
        let open = shown_sets(open);
        for key in shown_sets(closed).difference(&open) {
            dropped |= self.settings.drawing_changes(&self.saved, key) > 0;
            self.settings.restore(&self.saved, key);
            if key.role == PaneRole::AllTimeframes {
                self.settings.undo_retirement(&self.saved, &key.market);
            }
        }
        if self.settings == self.saved {
            // The pending change is consumed unwritten: the file already holds this.
            let _ = self.policy.take_flush();
        }
        dropped
    }
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.policy.is_dirty()
    }
}

#[cfg(test)]
#[path = "market_settings_store_tests.rs"]
mod tests;
