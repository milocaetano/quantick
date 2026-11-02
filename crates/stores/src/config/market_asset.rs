//! The asset a market is keyed by: an explicit declaration, never a guess at a
//! symbol's name.
use super::{AppConfig, FeedConfig, key_matches};

impl FeedConfig {
    fn asset_family(&self, symbol: &str) -> Option<&String> {
        self.asset_families
            .iter()
            .filter(|family| key_matches(family, symbol))
            .max_by_key(|family| family.len())
    }
}

impl AppConfig {
    /// The asset `symbol` belongs to on `venue`, stable across contract rolls: a
    /// real venue answers only for itself, by its declared family, else the key
    /// its bubble settings are shared under, else the symbol alone. A venue with
    /// no feed (a recording) takes the family any feed declares, so WINUSDT on
    /// a crypto venue never joins the mini index on another.
    ///
    /// Modes are left out on purpose: a mode's look key may name its venue, and
    /// the venue is already the other half of a market's key.
    #[must_use]
    pub fn market_asset(&self, venue: &str, symbol: &str) -> String {
        if let Some(feed) = self.feed(venue) {
            return feed
                .asset_family(symbol)
                .cloned()
                .unwrap_or_else(|| feed.plain_asset(symbol).key);
        }
        self.feeds
            .iter()
            .filter_map(|feed| feed.asset_family(symbol))
            .max_by_key(|family| family.len())
            .cloned()
            .unwrap_or_else(|| self.plain_bubble_asset(venue, symbol).key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../app/config/feeds.toml");

    fn shipped() -> AppConfig {
        toml::from_str(SHIPPED).expect("the shipped config parses")
    }

    #[test]
    fn every_contract_of_a_family_is_one_asset_on_a_venue() {
        let config = shipped();
        for symbol in ["WIN$N", "WINV26", "WINZ26", "WINF27"] {
            assert_eq!(config.market_asset("metatrader-b3", symbol), "WIN*");
        }
        for symbol in ["WDO$N", "WDOV26", "WDOZ26"] {
            assert_eq!(config.market_asset("metatrader-b3", symbol), "WDO*");
        }
    }

    #[test]
    fn a_recording_lands_on_the_family_another_feed_declares() {
        let config = shipped();
        assert_eq!(config.market_asset("replay", "WINZ26"), "WIN*");
        assert_eq!(config.market_asset("replay", "WDOV26"), "WDO*");
    }

    #[test]
    fn an_undeclared_symbol_is_its_own_asset_and_a_mode_does_not_rename_it() {
        let config = shipped();
        assert_eq!(config.market_asset("binance", "BTCUSDT"), "BTCUSDT");
        assert_eq!(
            config.market_asset("binance-futures", "BTCUSDT.p"),
            "BTCUSDT.p"
        );
        assert_eq!(config.market_asset("binance", "NOTLISTED"), "NOTLISTED");
    }

    /// A pattern on one feed never captures another venue's symbol.
    #[test]
    fn a_real_venue_answers_only_for_itself() {
        let config = shipped();
        assert_eq!(config.market_asset("binance", "WINUSDT"), "WINUSDT");
        assert_eq!(config.market_asset("binance", "WDOGE"), "WDOGE");
    }

    /// The family is for drawings: WDO's saved bubble look keeps its key.
    #[test]
    fn declaring_a_family_moves_no_saved_look() {
        let config = shipped();
        assert_eq!(config.bubble_asset("metatrader-b3", "WDO$N").key, "WDO$N");
    }
}
