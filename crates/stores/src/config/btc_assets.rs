//! Narrow BTC market identities; existing other-asset and family keys remain.

use super::{BubbleAsset, FeedConfig, ProviderKind};

impl FeedConfig {
    /// The pre-scoping key whose saved look an existing BTC market owned.
    /// USD-M is a new market and must never inherit Spot's former settings.
    pub(crate) fn legacy_btc_key(&self, symbol: &str) -> Option<String> {
        if self.provider == ProviderKind::BinanceFutures || scoped_asset(self, symbol).is_none() {
            return None;
        }
        Some(
            self.declared_asset(symbol)
                .map_or_else(|| symbol.to_owned(), |asset| asset.key),
        )
    }
}

pub(super) fn scoped_asset(feed: &FeedConfig, symbol: &str) -> Option<BubbleAsset> {
    let symbol = symbol.to_ascii_uppercase();
    if symbol != "BTCUSDT" && symbol != "BTCUSDT.P" {
        return None;
    }
    let canonical = if feed.provider == ProviderKind::BinanceFutures {
        "BTCUSDT.p"
    } else {
        symbol.as_str()
    };
    let declared = feed
        .declared_asset(canonical)
        .or_else(|| feed.declared_asset(&symbol));
    let key = match feed.provider {
        // Preserve the original Spot key and saved trader customizations.
        ProviderKind::Binance if canonical == "BTCUSDT" => "BTCUSDT".to_owned(),
        ProviderKind::BinanceFutures => "binance-futures:BTCUSDT.p".to_owned(),
        // Length delimiting makes arbitrary feed IDs unambiguous. The provider
        // is included so changing a feed's backend cannot reuse another venue.
        provider => format!("{provider:?}:{}:{}:{canonical}", feed.id.len(), feed.id),
    };
    Some(BubbleAsset {
        key,
        preset: declared
            .and_then(|asset| asset.preset)
            .or_else(|| feed.bubble_preset.clone()),
    })
}
