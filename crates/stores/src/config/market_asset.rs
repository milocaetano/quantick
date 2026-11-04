//! The asset a market is keyed by: an explicit declaration, never a guess at a
//! symbol's name.
use super::{AppConfig, FeedConfig, ProviderKind, key_matches};
use crate::symbols_file::AddedSymbols;
use quantick_workspace::market_list::{CatalogMarket, Venue, VenueKind};

impl FeedConfig {
    fn asset_family(&self, symbol: &str) -> Option<&String> {
        self.asset_families
            .iter()
            .filter(|family| key_matches(family, symbol))
            .max_by_key(|family| family.len())
    }

    /// Every alias a key matching `symbol` declares, in key order.
    #[must_use]
    pub fn asset_aliases_of(&self, symbol: &str) -> Vec<String> {
        self.asset_aliases
            .iter()
            .filter(|(key, _)| key_matches(key, symbol))
            .flat_map(|(_, aliases)| aliases.iter().cloned())
            .collect()
    }

    pub(super) fn validate_asset_aliases(&self) -> Result<(), String> {
        for (key, aliases) in &self.asset_aliases {
            if !self.symbols.iter().any(|offered| key_matches(key, offered)) {
                return Err(format!(
                    "feed '{}' names aliases for '{key}', which matches none of the symbols it \
                     offers",
                    self.id
                ));
            }
            if aliases.iter().any(|alias| alias.trim().is_empty()) {
                return Err(format!(
                    "feed '{}' names an empty alias for '{key}'",
                    self.id
                ));
            }
        }
        Ok(())
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

    /// The venues and markets "Open a market" lists, in config order.
    #[must_use]
    pub fn market_catalog(&self, added: &AddedSymbols) -> (Vec<Venue>, Vec<CatalogMarket>) {
        let venues = self
            .feeds
            .iter()
            .map(|feed| Venue {
                feed_id: feed.id.clone(),
                name: feed.name.clone(),
                chip: match feed.provider {
                    ProviderKind::Binance | ProviderKind::BinanceFutures => 'B',
                    ProviderKind::Hyperliquid => 'H',
                    ProviderKind::MetaTrader => '5',
                },
                kind: if feed.provider.is_implemented() {
                    VenueKind::Live
                } else {
                    VenueKind::ComingSoon
                },
                // Only what the feed declares: a market with no bar of its own
                // takes the active tab's, as `adopt_tab` decides.
                bar: (self.startup_spec_for(&feed.id)).map(|spec| spec.to_config_string()),
                bridged: feed.provider == ProviderKind::MetaTrader,
            })
            .collect();
        let markets = self
            .feeds
            .iter()
            .flat_map(|feed| feed.symbols.iter().map(move |symbol| (feed, symbol)))
            .map(|(feed, symbol)| CatalogMarket {
                feed_id: feed.id.clone(),
                symbol: symbol.clone(),
                asset: self.market_asset(&feed.id, symbol),
                aliases: feed.asset_aliases_of(symbol),
                user_added: added.contains(&feed.id, symbol),
            })
            .collect();
        (venues, markets)
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

    #[test]
    fn the_catalog_lists_every_venue_and_market_with_its_explicit_asset() {
        let config = shipped();
        let mut added = AddedSymbols::default();
        added.add("binance", "ETHUSDT");
        let (venues, markets) = config.market_catalog(&added);
        assert_eq!(venues.len(), config.feeds.len());
        let binance = &venues[0];
        assert_eq!(
            (binance.chip, binance.bar.as_deref()),
            ('B', Some("tick:50")),
            "BTCUSDT reads tick 50 from Binance's default_bars"
        );
        let b3 = venues
            .iter()
            .find(|venue| venue.feed_id == "metatrader-b3")
            .unwrap();
        assert_eq!((b3.chip, b3.bridged, b3.bar.as_deref()), ('5', true, None));
        let win = markets
            .iter()
            .find(|market| market.symbol == "WINZ26")
            .unwrap();
        assert_eq!(win.asset, "WIN*");
        assert!(win.aliases.contains(&"ibovespa".to_owned()));
        let eth = markets
            .iter()
            .find(|market| market.symbol == "ETHUSDT")
            .unwrap();
        assert!(eth.user_added);
    }

    #[test]
    fn aliases_search_every_contract_of_their_key() {
        let config = shipped();
        let b3 = config.feed("metatrader-b3").expect("the B3 feed");
        for symbol in ["WIN$N", "WINZ26"] {
            assert!(b3.asset_aliases_of(symbol).contains(&"ibovespa".to_owned()));
        }
        assert!(
            !b3.asset_aliases_of("WDO$N")
                .contains(&"ibovespa".to_owned())
        );
    }

    #[test]
    fn an_alias_key_that_matches_nothing_is_refused() {
        let mut config = shipped();
        let feed = config
            .feeds
            .iter_mut()
            .find(|feed| feed.id == "binance")
            .expect("binance");
        feed.asset_aliases
            .insert("GHOST".to_owned(), vec!["ghost".to_owned()]);
        let error = config.validate().expect_err("an unmatched key is refused");
        assert!(error.contains("GHOST"), "{error}");
    }

    /// The family is for drawings: WDO's saved bubble look keeps its key.
    #[test]
    fn declaring_a_family_moves_no_saved_look() {
        let config = shipped();
        assert_eq!(config.bubble_asset("metatrader-b3", "WDO$N").key, "WDO$N");
    }
}
