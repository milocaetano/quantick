//! The [`Trade`] input type and aggressor [`Side`].

use rust_decimal::Decimal;

/// The aggressor (taker) side of a trade.
///
/// A trade happens when an incoming *taker* order crosses the spread and matches
/// a resting *maker* order. The aggressor is the taker: the side that removed
/// liquidity and, by convention, "caused" the print. Order-flow bars track how
/// much volume was taker-buy vs taker-sell.
///
/// # Mapping from Binance aggTrades
///
/// Binance reports `m` (`is_buyer_maker`) per aggregate trade. If the buyer is
/// the maker, then the taker — the aggressor — is the seller, and vice versa:
///
/// | `is_buyer_maker` | aggressor      |
/// |------------------|----------------|
/// | `true`           | [`Side::Sell`] |
/// | `false`          | [`Side::Buy`]  |
///
/// Use [`Side::from_buyer_is_maker`] to apply this mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// A buy-initiated (taker-buy) trade — the aggressor lifted the offer.
    Buy,
    /// A sell-initiated (taker-sell) trade — the aggressor hit the bid.
    Sell,
}

impl Side {
    /// Map Binance's `is_buyer_maker` flag to the aggressor side.
    ///
    /// See the [type-level docs](Side#mapping-from-binance-aggtrades) for why
    /// the flag inverts.
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

/// A single executed trade — the engine's sole input.
///
/// One `Trade` corresponds to one Binance *aggregate* trade (`aggTrade`): a run
/// of individual fills at the same price from the same taker order, collapsed
/// into a single print. [`agg_id`](Trade::agg_id) is that aggregate trade's id,
/// monotonic per symbol; the live feed uses it to detect gaps and out-of-order
/// delivery.
///
/// Prices and quantities are [`Decimal`] for exact, deterministic arithmetic: no
/// binary-float rounding error accumulates as bars are built across a session.
/// Timestamps are epoch milliseconds carried from the exchange — the engine
/// never reads a wall clock (see the determinism rule in `CLAUDE.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trade {
    /// Exchange aggregate-trade id, monotonic per symbol.
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
    ///
    /// Exact for every realistic print, because both operands are [`Decimal`].
    /// The multiplication **saturates** at [`Decimal::MAX`] instead of panicking
    /// on the (physically impossible) overflow: prices and quantities arrive
    /// from an untrusted feed (a live exchange socket, or the local MT5 bridge
    /// any process can dial), and a corrupt or adversarial print near
    /// `Decimal::MAX` must never panic a bar builder, backtest or bot. Saturation
    /// keeps the engine deterministic — realistic values are unaffected — while
    /// honouring the "never panic on feed input" rule the feeds already follow.
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
        // A corrupt or adversarial feed print near Decimal::MAX must not panic a
        // dollar-bar builder, backtest or bot: the product saturates.
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
