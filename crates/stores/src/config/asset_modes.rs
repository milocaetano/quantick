//! Per-asset modes the config switches on (`symbol_asset_modes`), beside
//! the bubble presets: no symbol, feed id or provider in code decides them.

use serde::Deserialize;

use super::{AppConfig, BubbleAsset, FeedConfig, declared};

/// What an asset gets only because the config switches it on: tick-chart
/// spheres sized in the asset's own quantity unit, under a saved-look key of
/// its feed's own. The shipped file switches it on for Binance's BTC spot
/// and perpetual; any other asset opts in the same way.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetMode {
    /// What a quantity on this asset counts (`"BTC"`): the sphere caption
    /// and the settings say it.
    pub quantity_unit: String,
    /// The key the asset's saved look lives under. Absent, `<feed id>:<symbol>`,
    /// so one venue's market never shares a look with another's.
    #[serde(default)]
    pub look_key: Option<String>,
}

impl FeedConfig {
    /// `symbol`'s asset when this feed switches a mode on for it: the mode's
    /// key and this feed's declared look, never another feed's.
    pub(super) fn mode_asset(&self, symbol: &str) -> Option<BubbleAsset> {
        let (_, mode) = declared(&self.symbol_asset_modes, symbol)?;
        Some(BubbleAsset {
            key: mode
                .look_key
                .clone()
                .unwrap_or_else(|| format!("{}:{symbol}", self.id)),
            preset: self
                .declared_asset(symbol)
                .and_then(|asset| asset.preset)
                .or_else(|| self.bubble_preset.clone()),
            mode: Some(mode.clone()),
        })
    }

    pub(super) fn validate_asset_modes(&self) -> Result<(), String> {
        for (symbol, mode) in &self.symbol_asset_modes {
            if !self
                .symbols
                .iter()
                .any(|offered| super::key_matches(symbol, offered))
            {
                return Err(format!(
                    "feed '{}' switches an asset mode on for symbol '{symbol}', which matches \
                     none of the symbols it offers",
                    self.id
                ));
            }
            if mode.quantity_unit.trim().is_empty() {
                return Err(format!(
                    "feed '{}' names an empty quantity_unit for symbol '{symbol}'",
                    self.id
                ));
            }
            if mode
                .look_key
                .as_ref()
                .is_some_and(|key| key.trim().is_empty())
            {
                return Err(format!(
                    "feed '{}' names an empty look_key for symbol '{symbol}'",
                    self.id
                ));
            }
        }
        Ok(())
    }
}

impl AppConfig {
    /// `symbol`'s asset in a tab on `feed_id` when a mode is switched on for
    /// it: decided by the feed that offers the symbol (the tab's when it
    /// does, else the first that does), so a recording keeps its market.
    pub(super) fn mode_asset(&self, feed_id: &str, symbol: &str) -> Option<BubbleAsset> {
        let tab_feed = self.feed(feed_id);
        let offers = |feed: &&FeedConfig| feed.symbols.iter().any(|offered| offered == symbol);
        tab_feed
            .filter(offers)
            .or_else(|| self.feeds.iter().find(offers))
            .or(tab_feed)?
            .mode_asset(symbol)
    }

    /// Every saved-look key a mode moved: `(the mode's key, the key the asset
    /// had without it)` for each offered symbol whose two differ. What a
    /// store migrates, once per move, so a look saved before the mode keeps
    /// showing.
    #[must_use]
    pub fn moved_asset_keys(&self) -> Vec<(String, String)> {
        let mut moved = Vec::new();
        for feed in &self.feeds {
            for symbol in &feed.symbols {
                let Some(asset) = self.mode_asset(&feed.id, symbol) else {
                    continue;
                };
                let plain = self.plain_bubble_asset(&feed.id, symbol).key;
                if plain != asset.key {
                    moved.push((asset.key, plain));
                }
            }
        }
        moved
    }
}
