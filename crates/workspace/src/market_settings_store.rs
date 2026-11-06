//! The market settings document at its resolved path, under the commit policy.
use crate::{
    layout_commit::{LayoutCommitPolicy, LayoutSave},
    market_settings::{MARKET_SETTINGS_FILE, MarketKey, MarketSettings},
};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

/// Answers which asset a venue's symbol belongs to. Config, handed in.
pub type AssetResolver = Rc<dyn Fn(&str, &str) -> String>;

/// The per-market document, and the one rule that decides when it reaches disk.
pub struct MarketSettingsStore {
    settings: MarketSettings,
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
            settings,
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
        &mut self.settings
    }

    /// Replace the document wholesale — an imported bundle landing.
    pub fn set_settings(&mut self, settings: MarketSettings) {
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
    pub fn take_save(&mut self, now: Instant) -> LayoutSave {
        self.policy.take_save(now)
    }
    pub fn take_flush(&mut self) -> LayoutSave {
        self.policy.take_flush()
    }
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.policy.is_dirty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The switch is kept per asset: two symbols of one asset share it, the
    /// same asset on another venue does not, and only a change owes a save.
    #[test]
    fn bar_progress_is_kept_per_market_and_only_a_change_is_saved() {
        let assets: AssetResolver = Rc::new(|_, symbol: &str| symbol[..3].to_owned());
        let mut store =
            MarketSettingsStore::new(MarketSettings::default(), PathBuf::from("m.toml"), false)
                .with_assets(assets);
        let now = Instant::now();
        assert!(store.bar_progress_on("mt5", "WINZ26"), "on by default");
        assert!(store.set_bar_progress("mt5", "WINZ26", false, now));
        assert!(store.is_dirty(), "a change owes the file a save");
        assert!(!store.bar_progress_on("mt5", "WING27"), "the same asset");
        assert!(store.bar_progress_on("replay", "WINZ26"), "another venue");
        assert!(store.bar_progress_on("mt5", "WDOZ26"), "another asset");

        let _ = store.take_flush();
        assert!(!store.set_bar_progress("mt5", "WING27", false, now));
        assert!(!store.is_dirty(), "a repeat is no change and no save");
    }
}
