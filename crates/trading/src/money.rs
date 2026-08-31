//! What an instrument's price and size are worth in money, and the entry
//! quantity a risk budget buys.
//!
//! This module retires the workspace's oldest declared debt. Every P&L
//! number quantick produces is in *points* — [`signed_points`] and
//! [`ClosedTrade::pnl_points`] both say why: nothing here knew what one
//! point of an instrument was worth. [`InstrumentMoney`] is that missing
//! fact, and once it exists the same arithmetic runs backwards: instead of
//! "this many contracts lost this much", "this much to lose buys this many
//! contracts".
//!
//! # Why it lives beside the order vocabulary
//!
//! What one point of a contract is worth, and the smallest size that can be
//! traded, are facts about *trading*, not about simulation — the same test
//! that put orders and positions in this crate. A real broker adapter needs
//! them more than the paper simulator does: the simulator is the only venue
//! that can get away with not knowing a lot step. Placing them here also
//! keeps sizing in one module with [`signed_points`], the function money
//! multiplies, so the sizer and the P&L readout can never come to disagree
//! about the same stop.
//!
//! # Facts here, policy above
//!
//! [`size_for_risk`] reports what the arithmetic says and stops there. When
//! the smallest tradable size already loses more than the budget it answers
//! [`SizeOutcome::FlooredAboveBudget`] with the real number — it does not
//! decide whether that entry may be placed. That decision is a trader's
//! setting (a lock they can switch off) and belongs to the surface that
//! holds it. Keeping the refusal out of here is what lets a future
//! account-level ceiling reuse this function instead of forking it.
//!
//! # Honesty
//!
//! An [`InstrumentMoney`] is *declared*, never derived. Nothing in this
//! module infers a point value from a price's decimal places, from a price's
//! magnitude, or from a symbol's name. A wrong row on a chart is visible and
//! costs nothing; a wrong point value is a wrong position size and is
//! invisible until it fills.
//!
//! [`ClosedTrade::pnl_points`]: crate::ClosedTrade::pnl_points
//! [`signed_points`]: crate::signed_points

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::order::Bracket;

/// How far the grid landing may be corrected by multiplication.
///
/// A bound, not a preference. The candidate quantity comes out of a 28-digit
/// division, so it can sit at most one step off the answer; anything past
/// that would be a search, and a risk control that searches is one whose
/// cost a reader cannot see.
const GRID_CORRECTION_STEPS: usize = 4;

/// The currency a figure is denominated in.
///
/// Carried with every money amount rather than beside it: an unlabelled
/// currency figure is the data-honesty failure this whole module risks, and
/// two amounts in different currencies must never be added. There is no
/// exchange rate anywhere in this workspace, and this type is what makes
/// that absence enforceable rather than merely intended.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency(String);

impl Currency {
    /// The currency named by `code`, trimmed and upper-cased.
    ///
    /// `None` when the code is blank: an empty currency would let two
    /// unrelated amounts compare equal.
    #[must_use]
    pub fn new(code: &str) -> Option<Self> {
        let code = code.trim();
        if code.is_empty() {
            return None;
        }
        Some(Self(code.to_uppercase()))
    }

    /// The code, as stored.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.0
    }
}

/// An amount of money in a named currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    pub amount: Decimal,
    pub currency: Currency,
}

impl Money {
    /// An amount in `currency`.
    #[must_use]
    pub fn new(amount: Decimal, currency: Currency) -> Self {
        Self { amount, currency }
    }
}

/// Where an [`InstrumentMoney`] came from.
///
/// Shown to the trader, because a number they typed and a number a broker
/// declared are not the same claim, and a typo in the first costs real
/// money on a real platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneySource {
    /// Typed by the trader for this symbol.
    Declared,
    /// Reported by the venue that streams the instrument.
    Venue,
}

/// What one instrument's price and size are made of, in money.
///
/// Cold path: built when a symbol is configured or connects, never per
/// frame and never per print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstrumentMoney {
    /// Money per one *point* of price — one whole unit of the quoted price —
    /// per one unit of quantity. B3's mini index moves 0.20 BRL per point
    /// per contract; a linear crypto pair quoted in its own quote asset
    /// moves 1.
    pub point_value: Decimal,
    /// The smallest tradable increment of quantity.
    pub size_step: Decimal,
    /// The smallest tradable quantity.
    pub min_size: Decimal,
    /// The largest quantity, where the venue or the trader caps one.
    pub max_size: Option<Decimal>,
    pub currency: Currency,
    pub source: MoneySource,
}

/// How a sized quantity stands to the budget it was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeOutcome {
    /// The size grid divides the budget exactly: risk equals budget.
    Exact,
    /// Rounded down onto the size grid; risk is below budget. The shortfall
    /// is budget deliberately not spent.
    RoundedDown,
    /// The smallest tradable size already loses more than the budget. The
    /// quantity is that minimum and the risk is above budget — reported, not
    /// refused: whether such an entry may be placed is the surface's call.
    FlooredAboveBudget,
    /// The budget bought more than the instrument allows; clamped to
    /// `max_size`, so risk is below budget.
    CappedAtMax,
}

/// Why no quantity could be named at all.
///
/// Every way sizing declines is a named reason carrying a sentence, never a
/// silent zero — the rule [`LadderError`] already follows.
///
/// [`LadderError`]: crate::LadderError
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeRefusal {
    /// The entry carries no protective stop; there is no loss to size to.
    NoStop,
    /// A rung of the exit ladder protects only a target. That share of the
    /// entry has no bounded loss, so the entry as a whole has none.
    PartWithoutStop,
    /// The stop sits at the entry price: nothing to divide by.
    StopAtEntry,
    /// The stop sits on the winning side of the entry — that is a target.
    StopOnWrongSide,
    /// The budget is zero or negative.
    BudgetNotPositive,
    /// The budget and the instrument are denominated differently. There is
    /// no exchange rate here and inventing one would be a guess wearing a
    /// currency sign.
    CurrencyMismatch,
    /// The instrument's own size grid is unusable: a step or minimum that is
    /// not positive, or a maximum below the minimum, can never land a
    /// quantity.
    UnusableSizeGrid,
    /// The instrument's point value is zero or negative, so no stop distance
    /// costs anything. Sizing against it would divide by nothing.
    PointValueNotPositive,
}

impl SizeRefusal {
    /// A sentence for the trader, saying what to do instead.
    #[must_use]
    pub fn advice(self) -> &'static str {
        match self {
            Self::NoStop => {
                "fixed risk needs a stop - roll the wheel over the chart, pick a saved \
                 strategy, or type a stop offset"
            }
            Self::PartWithoutStop => {
                "every rung of the exit ladder needs a stop before its risk can be sized - \
                 give the unprotected rung one"
            }
            Self::StopAtEntry => "the stop sits on the entry price - move it away from the entry",
            Self::StopOnWrongSide => {
                "the stop sits on the winning side of the entry - that is a target, not a stop"
            }
            Self::BudgetNotPositive => "set a risk per trade above zero",
            Self::CurrencyMismatch => {
                "your risk per trade and this instrument are in different currencies, and \
                 nothing here converts between them - set a risk in the instrument's currency"
            }
            Self::UnusableSizeGrid => {
                "this instrument's size step and minimum size must both be above zero, and its \
                 maximum size must not be below its minimum"
            }
            Self::PointValueNotPositive => {
                "set this instrument's point value above zero - what one point of price is \
                 worth, per unit held"
            }
        }
    }
}

/// The quantity a budget buys at this entry's protection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SizedEntry {
    pub quantity: Decimal,
    /// Computed from the quantity actually returned, never echoed back from
    /// the budget that was asked for.
    pub risk: Money,
    pub outcome: SizeOutcome,
}

/// The stop distance one unit of this entry is exposed to, in price units.
///
/// A plain bracket has one stop and one distance. A laddered bracket has
/// several, and the answer is the share-weighted mean — the rungs' own
/// quantities are the weights, so the result is independent of the quantity
/// they were resolved at.
///
/// # Errors
///
/// [`SizeRefusal`] when the bracket carries no stop, when a rung protects
/// only a target, or when a stop sits at or on the winning side of `entry`.
pub fn stop_distance_per_unit(
    side: Side,
    entry: Decimal,
    bracket: &Bracket,
) -> Result<Decimal, SizeRefusal> {
    let mut weighted = Decimal::ZERO;
    let mut total_weight = Decimal::ZERO;
    let mut any_stop = false;
    let mut any_rung_unprotected = false;

    for part in bracket.parts() {
        // A rung with no quantity of its own covers the whole fill, which is
        // one share however large the fill turns out to be.
        let weight = part.quantity.unwrap_or(Decimal::ONE);
        let Some(stop) = part.stop_loss else {
            any_rung_unprotected = true;
            continue;
        };
        let distance = match side {
            Side::Buy => entry.saturating_sub(stop),
            Side::Sell => stop.saturating_sub(entry),
        };
        if distance.is_zero() {
            return Err(SizeRefusal::StopAtEntry);
        }
        if distance < Decimal::ZERO {
            return Err(SizeRefusal::StopOnWrongSide);
        }
        any_stop = true;
        weighted = weighted.saturating_add(weight.saturating_mul(distance));
        total_weight = total_weight.saturating_add(weight);
    }

    // Order matters: an entry with no stop anywhere is missing its stop, not
    // carrying a half-protected ladder.
    if !any_stop {
        return Err(SizeRefusal::NoStop);
    }
    if any_rung_unprotected {
        return Err(SizeRefusal::PartWithoutStop);
    }
    if total_weight <= Decimal::ZERO {
        return Err(SizeRefusal::NoStop);
    }
    weighted
        .checked_div(total_weight)
        .ok_or(SizeRefusal::NoStop)
}

/// What `quantity` of this entry stands to lose at its protection.
///
/// The inverse of [`size_for_risk`] over the same arithmetic, so a readout
/// built on one can never disagree with a size built on the other.
///
/// # Errors
///
/// [`SizeRefusal`] for the same reasons as [`stop_distance_per_unit`].
pub fn risk_at(
    side: Side,
    entry: Decimal,
    bracket: &Bracket,
    quantity: Decimal,
    money: &InstrumentMoney,
) -> Result<Money, SizeRefusal> {
    let per_unit = stop_distance_per_unit(side, entry, bracket)?;
    let amount = per_unit
        .saturating_mul(money.point_value)
        .saturating_mul(quantity);
    Ok(Money::new(amount, money.currency.clone()))
}

/// The quantity `budget` buys at this entry's protection.
///
/// Rounds **down** onto the instrument's size grid, always. The budget is a
/// ceiling the trader set, not a target to reach, and rounding up would
/// spend money they did not authorise — the one direction a risk control
/// must be incapable of.
///
/// # Errors
///
/// [`SizeRefusal`] when the entry cannot be sized at all. Note that "the
/// minimum size loses more than the budget" is *not* an error: it is
/// [`SizeOutcome::FlooredAboveBudget`], reported with the real risk.
pub fn size_for_risk(
    side: Side,
    entry: Decimal,
    bracket: &Bracket,
    budget: &Money,
    money: &InstrumentMoney,
) -> Result<SizedEntry, SizeRefusal> {
    if money.size_step <= Decimal::ZERO || money.min_size <= Decimal::ZERO {
        return Err(SizeRefusal::UnusableSizeGrid);
    }
    if money.max_size.is_some_and(|max| max < money.min_size) {
        return Err(SizeRefusal::UnusableSizeGrid);
    }
    if money.point_value <= Decimal::ZERO {
        return Err(SizeRefusal::PointValueNotPositive);
    }
    if budget.amount <= Decimal::ZERO {
        return Err(SizeRefusal::BudgetNotPositive);
    }
    if budget.currency != money.currency {
        return Err(SizeRefusal::CurrencyMismatch);
    }

    let per_unit = stop_distance_per_unit(side, entry, bracket)?;
    let risk_per_unit = per_unit.saturating_mul(money.point_value);
    if risk_per_unit <= Decimal::ZERO {
        return Err(SizeRefusal::PointValueNotPositive);
    }
    let raw = budget
        .amount
        .checked_div(risk_per_unit)
        .ok_or(SizeRefusal::PointValueNotPositive)?;

    // Land on the instrument's own grid, downward. The minimum is the
    // origin: a venue's tradable sizes are `min + k * step`, not every
    // multiple of the step.
    let mut quantity = if raw <= money.min_size {
        money.min_size
    } else {
        let steps = raw
            .saturating_sub(money.min_size)
            .checked_div(money.size_step)
            .ok_or(SizeRefusal::UnusableSizeGrid)?
            .floor();
        money
            .min_size
            .saturating_add(steps.saturating_mul(money.size_step))
    };

    // The cap lands on the grid like every other exit from this function.
    // A maximum that is not itself `min + k * step` - nothing forces one to
    // be - would otherwise return a quantity the venue cannot trade.
    let capped = match money.max_size {
        Some(max) if quantity > max => {
            let steps = max
                .saturating_sub(money.min_size)
                .checked_div(money.size_step)
                .ok_or(SizeRefusal::UnusableSizeGrid)?
                .floor();
            quantity = money
                .min_size
                .saturating_add(steps.saturating_mul(money.size_step));
            true
        }
        _ => false,
    };

    // `Decimal` division carries 28 digits, which is not exact arithmetic,
    // so the quotient above is a candidate and not yet an answer. Prove the
    // landing by multiplication instead: step down while the risk exceeds
    // the budget, then back up while a further step still fits inside it.
    // Both directions are bounded - they exist to correct a division
    // artefact of at most one step, never to search - and the step up is
    // gated on the multiplication, so it can never carry the risk over.
    for _ in 0..GRID_CORRECTION_STEPS {
        let risk = risk_per_unit.saturating_mul(quantity);
        let below = quantity.saturating_sub(money.size_step);
        if risk > budget.amount && below >= money.min_size {
            quantity = below;
            continue;
        }
        let above = quantity.saturating_add(money.size_step);
        let inside_ceiling = match money.max_size {
            Some(max) => above <= max,
            None => true,
        };
        if inside_ceiling && risk_per_unit.saturating_mul(above) <= budget.amount {
            quantity = above;
            continue;
        }
        break;
    }

    let risk = risk_per_unit.saturating_mul(quantity);
    let outcome = if risk > budget.amount {
        SizeOutcome::FlooredAboveBudget
    } else if capped {
        SizeOutcome::CappedAtMax
    } else if risk == budget.amount {
        SizeOutcome::Exact
    } else {
        SizeOutcome::RoundedDown
    };

    Ok(SizedEntry {
        quantity,
        risk: Money::new(risk, money.currency.clone()),
        outcome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::ExitPart;

    fn d(text: &str) -> Decimal {
        Decimal::from_str_exact(text).expect("test decimal literal")
    }

    fn brl() -> Currency {
        Currency::new("BRL").expect("BRL")
    }

    /// B3's mini index, at the point value the trader reported. A fixture,
    /// not a claim about B3: the app never compiles an instrument's money in.
    fn win() -> InstrumentMoney {
        InstrumentMoney {
            point_value: d("0.20"),
            size_step: Decimal::ONE,
            min_size: Decimal::ONE,
            max_size: None,
            currency: brl(),
            source: MoneySource::Declared,
        }
    }

    /// A linear crypto pair: quantity in the base asset, one point of price
    /// worth one unit of the quote asset per unit held.
    fn btc() -> InstrumentMoney {
        InstrumentMoney {
            point_value: Decimal::ONE,
            size_step: d("0.00001"),
            min_size: d("0.00001"),
            max_size: None,
            currency: Currency::new("USDT").expect("USDT"),
            source: MoneySource::Declared,
        }
    }

    fn budget(amount: &str) -> Money {
        Money::new(d(amount), brl())
    }

    fn long_stop(stop: &str) -> Bracket {
        Bracket::whole(Some(d(stop)), None)
    }

    #[test]
    fn an_exact_budget_buys_a_whole_number_of_contracts() {
        let sized = size_for_risk(
            Side::Buy,
            d("140000"),
            &long_stop("139500"),
            &budget("100"),
            &win(),
        )
        .expect("sizes");
        assert_eq!(sized.quantity, Decimal::ONE);
        assert_eq!(sized.risk.amount, d("100"));
        assert_eq!(sized.risk.currency, brl());
        assert_eq!(sized.outcome, SizeOutcome::Exact);
    }

    #[test]
    fn a_budget_that_does_not_divide_rounds_the_quantity_down() {
        // 200 points x 0.20 = 40.00 per contract; 100 / 40 = 2.5.
        let sized = size_for_risk(
            Side::Buy,
            d("140000"),
            &long_stop("139800"),
            &budget("100"),
            &win(),
        )
        .expect("sizes");
        assert_eq!(sized.quantity, d("2"), "2.5 contracts must never become 3");
        assert_eq!(sized.risk.amount, d("80"));
        assert_eq!(sized.outcome, SizeOutcome::RoundedDown);
    }

    /// The trader's own scenario: a stop so wide that one contract - the
    /// smallest position that exists - already loses more than the budget.
    /// The kernel reports it; whether such an entry may be placed is the
    /// surface's lock to decide.
    #[test]
    fn the_traders_clamp_a_wide_stop_floors_at_one_contract_over_budget() {
        // 4000 points x 0.20 = 800.00 for a single contract.
        let sized = size_for_risk(
            Side::Buy,
            d("140000"),
            &long_stop("136000"),
            &budget("100"),
            &win(),
        )
        .expect("reports rather than refuses");
        assert_eq!(sized.quantity, Decimal::ONE);
        assert_eq!(sized.risk.amount, d("800"));
        assert_eq!(sized.outcome, SizeOutcome::FlooredAboveBudget);
    }

    #[test]
    fn a_tight_stop_is_capped_by_the_instrument_not_by_the_budget() {
        let mut win = win();
        win.max_size = Some(d("50"));
        // 5 points x 0.20 = 1.00 per contract; the budget would buy 100.
        let sized = size_for_risk(
            Side::Buy,
            d("140000"),
            &long_stop("139995"),
            &budget("100"),
            &win,
        )
        .expect("sizes");
        assert_eq!(sized.quantity, d("50"));
        assert_eq!(sized.risk.amount, d("50"));
        assert_eq!(sized.outcome, SizeOutcome::CappedAtMax);
    }

    #[test]
    fn a_fractional_size_step_lands_on_the_grid_and_under_the_budget() {
        let money = btc();
        let budget = Money::new(d("100"), money.currency.clone());
        let sized = size_for_risk(
            Side::Buy,
            d("68000"),
            &Bracket::whole(Some(d("67150")), None),
            &budget,
            &money,
        )
        .expect("sizes");
        assert_eq!(sized.quantity, d("0.11764"));
        assert!(
            sized.risk.amount <= d("100"),
            "risk {} must not exceed the budget",
            sized.risk.amount
        );
        assert_eq!(sized.outcome, SizeOutcome::RoundedDown);
    }

    #[test]
    fn a_short_entry_is_sized_off_a_stop_above_it() {
        let sized = size_for_risk(
            Side::Sell,
            d("140000"),
            &Bracket::whole(Some(d("140500")), None),
            &budget("100"),
            &win(),
        )
        .expect("sizes");
        assert_eq!(sized.quantity, Decimal::ONE);
        assert_eq!(sized.risk.amount, d("100"));
    }

    #[test]
    fn a_stop_at_the_entry_is_refused_by_name() {
        let refusal = size_for_risk(
            Side::Buy,
            d("140000"),
            &long_stop("140000"),
            &budget("100"),
            &win(),
        )
        .expect_err("refuses");
        assert_eq!(refusal, SizeRefusal::StopAtEntry);
    }

    #[test]
    fn a_stop_on_the_winning_side_is_refused_by_name() {
        let refusal = size_for_risk(
            Side::Buy,
            d("140000"),
            &long_stop("140500"),
            &budget("100"),
            &win(),
        )
        .expect_err("refuses");
        assert_eq!(refusal, SizeRefusal::StopOnWrongSide);
    }

    #[test]
    fn an_entry_with_no_stop_cannot_be_sized() {
        let refusal = size_for_risk(
            Side::Buy,
            d("140000"),
            &Bracket::none(),
            &budget("100"),
            &win(),
        )
        .expect_err("refuses");
        assert_eq!(refusal, SizeRefusal::NoStop);
    }

    #[test]
    fn an_entry_protected_only_by_a_target_cannot_be_sized() {
        let refusal = size_for_risk(
            Side::Buy,
            d("140000"),
            &Bracket::whole(None, Some(d("141000"))),
            &budget("100"),
            &win(),
        )
        .expect_err("refuses");
        assert_eq!(refusal, SizeRefusal::NoStop);
    }

    #[test]
    fn a_budget_of_zero_or_less_is_refused() {
        for amount in ["0", "-5"] {
            let refusal = size_for_risk(
                Side::Buy,
                d("140000"),
                &long_stop("139500"),
                &Money::new(d(amount), brl()),
                &win(),
            )
            .expect_err("refuses");
            assert_eq!(refusal, SizeRefusal::BudgetNotPositive, "budget {amount}");
        }
    }

    #[test]
    fn a_budget_in_another_currency_is_refused_rather_than_converted() {
        let refusal = size_for_risk(
            Side::Buy,
            d("140000"),
            &long_stop("139500"),
            &Money::new(d("100"), Currency::new("USD").expect("USD")),
            &win(),
        )
        .expect_err("refuses");
        assert_eq!(refusal, SizeRefusal::CurrencyMismatch);
    }

    #[test]
    fn an_unusable_size_grid_is_refused_by_name() {
        for (step, min) in [("0", "1"), ("1", "0"), ("-1", "1")] {
            let mut money = win();
            money.size_step = d(step);
            money.min_size = d(min);
            let refusal = size_for_risk(
                Side::Buy,
                d("140000"),
                &long_stop("139500"),
                &budget("100"),
                &money,
            )
            .expect_err("refuses");
            assert_eq!(
                refusal,
                SizeRefusal::UnusableSizeGrid,
                "step {step} min {min}"
            );
        }
    }

    /// A saved strategy's ladder: the entry is protected in rungs, each with
    /// its own stop, so the risk per unit is the share-weighted distance.
    #[test]
    fn a_ladder_is_sized_on_its_share_weighted_stop_distance() {
        let bracket = Bracket::ladder(&[
            ExitPart {
                quantity: Some(Decimal::ONE),
                stop_loss: Some(d("139800")),
                take_profit: Some(d("140400")),
            },
            ExitPart {
                quantity: Some(Decimal::ONE),
                stop_loss: Some(d("139600")),
                take_profit: Some(d("140800")),
            },
        ])
        .expect("ladder");
        // Weighted distance (200 + 400) / 2 = 300 points; 300 x 0.20 = 60.00.
        let per_unit = stop_distance_per_unit(Side::Buy, d("140000"), &bracket).expect("distance");
        assert_eq!(per_unit, d("300"));

        let sized =
            size_for_risk(Side::Buy, d("140000"), &bracket, &budget("100"), &win()).expect("sizes");
        assert_eq!(sized.quantity, Decimal::ONE);
        assert_eq!(sized.risk.amount, d("60"));
        assert_eq!(sized.outcome, SizeOutcome::RoundedDown);
    }

    /// The weights are shares, so the same ladder resolved at a different
    /// quantity must give the same per-unit distance. Without this, sizing a
    /// ladder would depend on the quantity it was resolved at - which is the
    /// quantity sizing is trying to compute.
    #[test]
    fn a_ladders_per_unit_distance_does_not_depend_on_the_quantity_it_was_resolved_at() {
        let at_two = Bracket::ladder(&[
            ExitPart {
                quantity: Some(Decimal::ONE),
                stop_loss: Some(d("139800")),
                take_profit: None,
            },
            ExitPart {
                quantity: Some(Decimal::ONE),
                stop_loss: Some(d("139600")),
                take_profit: None,
            },
        ])
        .expect("ladder");
        let at_six = Bracket::ladder(&[
            ExitPart {
                quantity: Some(d("3")),
                stop_loss: Some(d("139800")),
                take_profit: None,
            },
            ExitPart {
                quantity: Some(d("3")),
                stop_loss: Some(d("139600")),
                take_profit: None,
            },
        ])
        .expect("ladder");
        assert_eq!(
            stop_distance_per_unit(Side::Buy, d("140000"), &at_two),
            stop_distance_per_unit(Side::Buy, d("140000"), &at_six),
        );
    }

    #[test]
    fn a_ladder_rung_without_a_stop_is_refused_by_name() {
        let bracket = Bracket::ladder(&[
            ExitPart {
                quantity: Some(Decimal::ONE),
                stop_loss: Some(d("139800")),
                take_profit: None,
            },
            ExitPart {
                quantity: Some(Decimal::ONE),
                stop_loss: None,
                take_profit: Some(d("140800")),
            },
        ])
        .expect("ladder");
        let refusal = size_for_risk(Side::Buy, d("140000"), &bracket, &budget("100"), &win())
            .expect_err("refuses");
        assert_eq!(refusal, SizeRefusal::PartWithoutStop);
    }

    #[test]
    fn the_readout_and_the_sizer_agree_on_the_same_stop() {
        let bracket = long_stop("139800");
        let sized =
            size_for_risk(Side::Buy, d("140000"), &bracket, &budget("100"), &win()).expect("sizes");
        let readout =
            risk_at(Side::Buy, d("140000"), &bracket, sized.quantity, &win()).expect("reads");
        assert_eq!(readout, sized.risk);
    }

    /// The postcondition the whole feature rests on, swept rather than
    /// spot-checked: the risk returned never exceeds the budget unless the
    /// minimum tradable size made that impossible. This is what proves the
    /// rounding is a floor and that `Decimal`'s 28-digit division never
    /// nudged a quantity one step up.
    #[test]
    fn the_returned_risk_never_exceeds_the_budget_except_at_the_floor() {
        let money = win();
        for stop_points in 1..400_i64 {
            for budget_amount in [7_i64, 40, 100, 333, 1000] {
                let entry = d("140000");
                let bracket = Bracket::whole(Some(entry - Decimal::from(stop_points)), None);
                let budget = Money::new(Decimal::from(budget_amount), brl());
                let sized =
                    size_for_risk(Side::Buy, entry, &bracket, &budget, &money).expect("sizes");
                assert!(
                    sized.quantity >= money.min_size,
                    "quantity {} below the minimum",
                    sized.quantity
                );
                if sized.outcome == SizeOutcome::FlooredAboveBudget {
                    assert_eq!(sized.quantity, money.min_size);
                    assert!(sized.risk.amount > budget.amount);
                } else {
                    assert!(
                        sized.risk.amount <= budget.amount,
                        "risk {} over budget {} at {stop_points} points",
                        sized.risk.amount,
                        budget.amount
                    );
                }
            }
        }
    }
}
