//! The [`Trade`] input type and aggressor [`Side`].

use rust_decimal::Decimal;

/// The aggressor (taker) side of a trade: the side that removed liquidity.
/// # Venue mappings
/// Binance `is_buyer_maker` `true` is [`Side::Sell`], `false` is [`Side::Buy`] ([`Side::from_buyer_is_maker`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// A buy-initiated (taker-buy) trade — the aggressor lifted the offer.
    Buy,
    /// A sell-initiated (taker-sell) trade — the aggressor hit the bid.
    Sell,
}

impl Side {
    /// Map Binance's `is_buyer_maker` flag to the aggressor side; see [type docs](Side#venue-mappings).
    #[must_use]
    pub fn from_buyer_is_maker(is_buyer_maker: bool) -> Self {
        if is_buyer_maker {
            Side::Sell
        } else {
            Side::Buy
        }
    }

    /// The lower-case token used in fixture files: `"buy"` or `"sell"`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }
}

/// A single executed trade: the engine's sole input, the smallest execution unit a source
/// publishes. [`agg_id`](Trade::agg_id) is monotonic per source stream, venue-assigned or synthetic.
/// Prices and quantities are [`Decimal`] (exact); timestamps are exchange epoch ms, never a clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    /// Source trade id, monotonic within one symbol/session stream.
    pub agg_id: u64,
    /// Trade time, in milliseconds since the Unix epoch.
    pub timestamp_ms: i64,
    /// Execution price.
    pub price: Decimal,
    /// Executed quantity, in base-asset units.
    pub quantity: Decimal,
    /// Aggressor side (see [`Side`]).
    pub side: Side,
}

impl Trade {
    /// Notional value of the trade: `price * quantity`.
    /// Saturates at [`Decimal::MAX`] instead of panicking: feed input is untrusted and must never
    /// panic a builder, backtest or bot.
    #[must_use]
    pub fn notional(&self) -> Decimal {
        self.price.saturating_mul(self.quantity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;

    #[test]
    fn buyer_maker_maps_to_sell_aggressor() {
        assert_eq!(Side::from_buyer_is_maker(true), Side::Sell);
        assert_eq!(Side::from_buyer_is_maker(false), Side::Buy);
    }

    #[test]
    fn side_tokens_round_trip_naming() {
        assert_eq!(Side::Buy.as_str(), "buy");
        assert_eq!(Side::Sell.as_str(), "sell");
    }

    #[test]
    fn notional_is_exact() {
        let t = Trade {
            agg_id: 1,
            timestamp_ms: 1_700_000_000_000,
            price: Decimal::from_str("36000.10").unwrap(),
            quantity: Decimal::from_str("0.005").unwrap(),
            side: Side::Buy,
        };
        // 36000.10 * 0.005 = 180.00050 exactly (no float drift).
        assert_eq!(t.notional(), Decimal::from_str("180.00050").unwrap());
    }

    #[test]
    fn notional_saturates_instead_of_panicking_on_overflow() {
        // A print near Decimal::MAX must not panic a dollar-bar builder: the product saturates.
        let t = Trade {
            agg_id: 1,
            timestamp_ms: 0,
            price: Decimal::MAX,
            quantity: Decimal::from(2),
            side: Side::Buy,
        };
        assert_eq!(t.notional(), Decimal::MAX);
    }
}
