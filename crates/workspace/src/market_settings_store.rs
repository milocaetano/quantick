//! The market settings document at its resolved path, under the commit policy.
use crate::{
    layout_commit::{LayoutCommitPolicy, LayoutSave},
    market_settings::{DrawingKey, MARKET_SETTINGS_FILE, MarketKey, MarketSettings},
};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

/// Answers which asset a venue's symbol belongs to. Config, handed in.
pub type AssetResolver = Rc<dyn Fn(&str, &str) -> String>;

/// The per-market document, and the one rule that decides when it reaches disk.
///
/// With autosave off the store is *held*: drawings and anchored studies change
/// in memory and wait for Save ([`Self::take_commit`]); `saved` is what the
/// file holds, so a held change can be counted and dropped.
pub struct MarketSettingsStore {
    settings: MarketSettings,
    saved: MarketSettings,
    held: bool,
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

    #[must_use]
    pub fn settings(&self) -> &MarketSettings {
        &self.settings
    }
    pub fn settings_mut(&mut self) -> &mut MarketSettings {
        &mut self.settings
    }

    /// Replace the document wholesale — an imported bundle landing.
    pub fn set_settings(&mut self, settings: MarketSettings) {
        self.saved = settings.clone();
        self.settings = settings;
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
    /// The debounced write; nothing while held.
    pub fn take_save(&mut self, now: Instant) -> LayoutSave {
        if self.held {
            return LayoutSave::Wait;
        }
        let save = self.policy.take_save(now);
        self.note_written(save)
    }
    /// The write on exit or before an export; nothing while held.
    pub fn take_flush(&mut self) -> LayoutSave {
        if self.held {
            return LayoutSave::Wait;
        }
        self.take_commit()
    }
    /// Save: the write now, held or not.
    pub fn take_commit(&mut self) -> LayoutSave {
        let save = self.policy.take_flush();
        self.note_written(save)
    }
    fn note_written(&mut self, save: LayoutSave) -> LayoutSave {
        if save == LayoutSave::Write {
            self.saved = self.settings.clone();
        }
        save
    }

    /// Hold writes until Save (autosave off), or let them through.
    pub fn set_held(&mut self, held: bool) {
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
        keys.into_iter()
            .map(|key| self.settings.drawing_changes(&self.saved, key))
            .sum()
    }

    /// Close without saving: put every key in `closed` that no pane in `open`
    /// still shows back to the file's version. Once nothing differs, nothing is
    /// owed. Answers whether anything was dropped.
    pub fn discard(&mut self, closed: &[DrawingKey], open: &[DrawingKey]) -> bool {
        let mut dropped = false;
        for key in closed.iter().filter(|key| !open.contains(key)) {
            dropped |= self.settings.drawing_changes(&self.saved, key) > 0;
            self.settings.restore(&self.saved, key);
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
