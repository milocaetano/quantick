//! Binance market identity and public endpoint routing. The display suffix
//! identifies a perpetual; it never reaches Binance's wire symbol.

/// One independently sequenced Binance market.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum BinanceMarket {
    /// The existing cash-market source.
    #[default]
    Spot,
    /// USDT-margined perpetual futures.
    UsdM,
}

impl BinanceMarket {
    /// Public REST origin for this market.
    #[must_use]
    pub const fn rest_base(self) -> &'static str {
        match self {
            Self::Spot => "https://api.binance.com",
            Self::UsdM => "https://fapi.binance.com",
        }
    }

    /// REST path prefix shared by trades, depth and candles.
    #[must_use]
    pub const fn rest_prefix(self) -> &'static str {
        match self {
            Self::Spot => "/api/v3",
            Self::UsdM => "/fapi/v1",
        }
    }

    /// Raw aggregate-trade socket origin/path.
    #[must_use]
    pub const fn trade_ws_base(self) -> &'static str {
        match self {
            Self::Spot => "wss://stream.binance.com:9443/ws",
            Self::UsdM => "wss://fstream.binance.com/market/ws",
        }
    }

    /// Depth and aggregate trades use different Futures socket services.
    #[must_use]
    pub const fn depth_ws_base(self) -> &'static str {
        match self {
            Self::Spot => "wss://stream.binance.com:9443/ws",
            Self::UsdM => "wss://fstream.binance.com/public/ws",
        }
    }

    /// Exchange symbol; `.p` is an application display identity only.
    #[must_use]
    pub fn wire_symbol(self, symbol: &str) -> String {
        match self {
            Self::Spot => symbol.to_ascii_uppercase(),
            Self::UsdM => quantick_sources::perpetual::wire_symbol(symbol),
        }
    }

    /// Canonical identity on the application's independent market channels.
    #[must_use]
    pub fn display_symbol(self, symbol: &str) -> String {
        match self {
            Self::Spot => self.wire_symbol(symbol),
            Self::UsdM => quantick_sources::perpetual::display_symbol(symbol),
        }
    }

    /// The error code this market answers an `aggTrades` page with once the
    /// IDs asked for are older than its retention: history ends there,
    /// nothing failed. `None` where the market keeps its whole history.
    #[must_use]
    pub const fn history_expired_code(self) -> Option<i64> {
        match self {
            Self::Spot => None,
            Self::UsdM => Some(-4166),
        }
    }

    /// Maximum depth snapshot supported by this market.
    #[must_use]
    pub const fn max_depth(self) -> u16 {
        match self {
            Self::Spot => 5_000,
            Self::UsdM => 1_000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn futures_identity_never_leaks_the_display_suffix_to_binance() {
        assert_eq!(BinanceMarket::UsdM.wire_symbol("btcusdt.p"), "BTCUSDT");
        assert_eq!(BinanceMarket::UsdM.display_symbol("BTCUSDT.P"), "BTCUSDT.p");
        assert_eq!(BinanceMarket::Spot.display_symbol("BTCUSDT"), "BTCUSDT");
        assert_eq!(BinanceMarket::UsdM.rest_prefix(), "/fapi/v1");
        assert_ne!(
            BinanceMarket::UsdM.trade_ws_base(),
            BinanceMarket::UsdM.depth_ws_base()
        );
        assert_eq!(BinanceMarket::UsdM.max_depth(), 1_000);
        assert_eq!(BinanceMarket::UsdM.history_expired_code(), Some(-4166));
        assert_eq!(BinanceMarket::Spot.history_expired_code(), None);
    }
}
