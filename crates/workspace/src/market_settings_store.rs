//! The market settings document at its resolved path, under the commit policy.
use crate::{
    layout_commit::{LayoutCommitPolicy, LayoutSave},
    market_settings::{MARKET_SETTINGS_FILE, MarketSettings},
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
