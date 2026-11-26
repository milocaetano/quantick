//! What a simulated fill costs beyond its print: a commission on each side
//! of a round trip, slippage on the fills that take liquidity, and an
//! optional stricter rule for when a resting limit counts as filled.
//!
//! Every amount is in points (price units × quantity), the unit the
//! simulator books everything in; turning points into money stays with the
//! instrument catalog that knows a point's value. [`FillCosts::default`] is
//! free: no commission, no slippage, a touch fills a limit — exactly the
//! fill model that existed before costs did.

use quantick_engine::Side;
use rust_decimal::Decimal;

/// How a commission is charged on each side of a round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Commission {
    /// Nothing is charged.
    #[default]
    Free,
    /// Points per contract, per side.
    PerContract(Decimal),
    /// A fraction of the fill's notional (price × quantity), per side:
    /// `0.0004` is four basis points.
    PerNotional(Decimal),
}

/// Why a [`FillCosts`] cannot be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostsError {
    /// A commission below zero would pay the trader to trade.
    NegativeCommission,
    /// Slippage or conservative limits measure in ticks, and no positive
    /// tick size was given.
    MissingTick,
}

impl std::fmt::Display for CostsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NegativeCommission => "a commission must not be negative",
            Self::MissingTick => "slippage and conservative limit fills need a positive tick size",
        })
    }
}

impl std::error::Error for CostsError {}

/// One instrument's fill costs. Built only through [`FillCosts::new`], so a
/// value in hand is always consistent: slippage and conservative limits
/// always have the tick they are measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FillCosts {
    commission: Commission,
    slippage_ticks: u32,
    tick_size: Option<Decimal>,
    conservative_limits: bool,
}

impl FillCosts {
    /// Fill costs for one instrument.
    ///
    /// `slippage_ticks` moves every market-style fill (market and stop
    /// entries, stop exits, hand closes and flattens) that many ticks
    /// against the trader. `conservative_limits` makes a resting limit (an
    /// entry or a take profit) fill only when a print trades one tick
    /// through its price, instead of on a touch.
    ///
    /// # Errors
    /// [`CostsError::NegativeCommission`] for a commission below zero;
    /// [`CostsError::MissingTick`] when slippage or conservative limits are
    /// asked for without a positive `tick_size`.
    pub fn new(
        commission: Commission,
        slippage_ticks: u32,
        tick_size: Option<Decimal>,
        conservative_limits: bool,
    ) -> Result<Self, CostsError> {
        let rate = match commission {
            Commission::Free => Decimal::ZERO,
            Commission::PerContract(rate) | Commission::PerNotional(rate) => rate,
        };
        if rate < Decimal::ZERO {
            return Err(CostsError::NegativeCommission);
        }
        let tick_size = tick_size.filter(|tick| *tick > Decimal::ZERO);
        if (slippage_ticks > 0 || conservative_limits) && tick_size.is_none() {
            return Err(CostsError::MissingTick);
        }
        Ok(Self {
            commission,
            slippage_ticks,
            tick_size,
            conservative_limits,
        })
    }

    /// The commission one side of a trade pays: `quantity` filled at
    /// `price`.
    #[must_use]
    pub fn side_fee(&self, price: Decimal, quantity: Decimal) -> Decimal {
        match self.commission {
            Commission::Free => Decimal::ZERO,
            Commission::PerContract(rate) => rate.saturating_mul(quantity),
            Commission::PerNotional(rate) => rate.saturating_mul(price.saturating_mul(quantity)),
        }
    }

    /// The commission a round trip of `quantity` pays: the entry side at
    /// `entry`, the exit side at `exit`.
    #[must_use]
    pub fn round_trip_fees(&self, entry: Decimal, exit: Decimal, quantity: Decimal) -> Decimal {
        self.side_fee(entry, quantity)
            .saturating_add(self.side_fee(exit, quantity))
    }

    /// Where a market-style fill on `side` lands when the print is at
    /// `price`: a buy pays up, a sell gives up, each by the slippage.
    #[must_use]
    pub fn slipped(&self, side: Side, price: Decimal) -> Decimal {
        let slip = self.ticks(self.slippage_ticks);
        match side {
            Side::Buy => price.saturating_add(slip),
            Side::Sell => price.saturating_sub(slip),
        }
    }

    /// Whether a print at `price` fills a resting limit on `side` at
    /// `level`: a touch by default, one tick through with conservative
    /// limits on.
    #[must_use]
    pub fn limit_reached(&self, side: Side, level: Decimal, price: Decimal) -> bool {
        let through = self.ticks(u32::from(self.conservative_limits));
        match side {
            Side::Buy => price <= level.saturating_sub(through),
            Side::Sell => price >= level.saturating_add(through),
        }
    }

    fn ticks(&self, count: u32) -> Decimal {
        self.tick_size.map_or(Decimal::ZERO, |tick| {
            tick.saturating_mul(Decimal::from(count))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(text: &str) -> Decimal {
        text.parse().expect("test decimal")
    }

    #[test]
    fn the_default_is_free_and_fills_on_a_touch() {
        let costs = FillCosts::default();
        assert_eq!(
            costs.round_trip_fees(dec("100"), dec("105"), dec("3")),
            Decimal::ZERO
        );
        assert_eq!(costs.slipped(Side::Buy, dec("100")), dec("100"));
        assert!(costs.limit_reached(Side::Buy, dec("100"), dec("100")));
        assert!(costs.limit_reached(Side::Sell, dec("100"), dec("100")));
    }

    #[test]
    fn a_per_contract_commission_charges_each_side_by_quantity() {
        let costs = FillCosts::new(Commission::PerContract(dec("1.25")), 0, None, false)
            .expect("valid costs");
        assert_eq!(costs.side_fee(dec("120000"), dec("2")), dec("2.5"));
        assert_eq!(
            costs.round_trip_fees(dec("1"), dec("9"), dec("2")),
            dec("5")
        );
    }

    #[test]
    fn a_notional_commission_charges_each_side_on_its_own_price() {
        let costs = FillCosts::new(Commission::PerNotional(dec("0.001")), 0, None, false)
            .expect("valid costs");
        // 0.001 × (100 × 2) + 0.001 × (110 × 2)
        assert_eq!(
            costs.round_trip_fees(dec("100"), dec("110"), dec("2")),
            dec("0.42")
        );
    }

    #[test]
    fn slippage_moves_a_fill_against_its_side() {
        let costs =
            FillCosts::new(Commission::Free, 2, Some(dec("0.5")), false).expect("valid costs");
        assert_eq!(costs.slipped(Side::Buy, dec("100")), dec("101"));
        assert_eq!(costs.slipped(Side::Sell, dec("100")), dec("99"));
    }

    #[test]
    fn a_conservative_limit_needs_a_print_one_tick_through() {
        let costs = FillCosts::new(Commission::Free, 0, Some(dec("5")), true).expect("valid costs");
        assert!(!costs.limit_reached(Side::Buy, dec("100"), dec("100")));
        assert!(!costs.limit_reached(Side::Buy, dec("100"), dec("96")));
        assert!(costs.limit_reached(Side::Buy, dec("100"), dec("95")));
        assert!(!costs.limit_reached(Side::Sell, dec("100"), dec("104")));
        assert!(costs.limit_reached(Side::Sell, dec("100"), dec("105")));
    }

    #[test]
    fn slippage_or_conservative_limits_without_a_tick_are_refused() {
        assert_eq!(
            FillCosts::new(Commission::Free, 1, None, false),
            Err(CostsError::MissingTick)
        );
        assert_eq!(
            FillCosts::new(Commission::Free, 0, Some(Decimal::ZERO), true),
            Err(CostsError::MissingTick)
        );
    }

    #[test]
    fn a_negative_commission_is_refused() {
        assert_eq!(
            FillCosts::new(Commission::PerContract(dec("-1")), 0, None, false),
            Err(CostsError::NegativeCommission)
        );
    }
}
