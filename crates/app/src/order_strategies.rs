//! Named exit strategies: the ladder a trader keeps, not the one they retype.
//!
//! A strategy is a short list of rows — a share of the quantity, a gain in
//! ticks, a loss in ticks — that resolves into a [`Bracket`] once an entry
//! names its side, price and size. Traders coming from other platforms know
//! this shape: "half off at eighty ticks with forty of risk, the rest at
//! twenty with twenty-five".
//!
//! The resolution lives here and nowhere else. The chart projects the ladder
//! before the click and the ticket places it after, and both call this one
//! function — a projection that disagreed with the order it promised would
//! be the worst kind of bug this surface can have.
//!
//! Ticks only, deliberately. A currency or percentage row would need a tick
//! value the workspace does not have, and a number the app cannot compute
//! honestly is a number it does not show.

use quantick_engine::Side;
use quantick_sim::{Bracket, ExitPart, LadderError, MAX_EXIT_PARTS};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// The most rows one strategy may carry — the simulator's own ladder bound,
/// restated here so the editor can refuse a sixth row while it is being
/// typed rather than at placement time.
pub(crate) const MAX_ROWS: usize = MAX_EXIT_PARTS;

/// One rung of a named strategy, in the units its editor shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StrategyRow {
    /// Share of the entry's quantity this rung closes, in percent.
    pub share_percent: Decimal,
    /// Distance to the target, in ticks; `None` leaves the rung with no
    /// target, which is how a runner is expressed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gain_ticks: Option<u32>,
    /// Distance to the stop, in ticks; `None` leaves the rung unstopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss_ticks: Option<u32>,
}

impl StrategyRow {
    /// True when the rung protects nothing at all.
    fn is_bare(&self) -> bool {
        self.gain_ticks.is_none() && self.loss_ticks.is_none()
    }
}

/// A named exit ladder the trader keeps between sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OrderStrategy {
    pub name: String,
    pub rows: Vec<StrategyRow>,
}

/// Why a strategy cannot be used as written.
///
/// Each one is a sentence the editor shows beside the offending field, so
/// the trader fixes it there rather than discovering it when an order is
/// refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StrategyError {
    /// No rows at all.
    Empty,
    /// More rows than [`MAX_ROWS`].
    TooManyRows,
    /// The shares do not add up to a whole position.
    SharesDoNotSumToWhole,
    /// A row has neither a gain nor a loss.
    RowProtectsNothing,
    /// A share is zero or negative.
    ShareNotPositive,
    /// The resolved ladder was refused by the simulator.
    Ladder(LadderError),
}

impl StrategyError {
    /// A sentence for the trader, saying what to do instead.
    pub(crate) fn advice(self) -> &'static str {
        match self {
            Self::Empty => "a strategy needs at least one row",
            Self::TooManyRows => "a strategy takes at most four rows - merge two of them",
            Self::SharesDoNotSumToWhole => "the shares must add up to 100%",
            Self::RowProtectsNothing => "every row needs a gain or a loss - give this one either",
            Self::ShareNotPositive => "every row must close a positive share",
            Self::Ladder(error) => error.advice(),
        }
    }
}

impl OrderStrategy {
    /// Check the strategy as written, without needing an order.
    ///
    /// # Errors
    ///
    /// The first [`StrategyError`] the rows commit, in reading order.
    pub(crate) fn validate(&self) -> Result<(), StrategyError> {
        if self.rows.is_empty() {
            return Err(StrategyError::Empty);
        }
        if self.rows.len() > MAX_ROWS {
            return Err(StrategyError::TooManyRows);
        }
        let mut total = Decimal::ZERO;
        for row in &self.rows {
            if row.share_percent <= Decimal::ZERO {
                return Err(StrategyError::ShareNotPositive);
            }
            if row.is_bare() {
                return Err(StrategyError::RowProtectsNothing);
            }
            total = total.saturating_add(row.share_percent);
        }
        if total != Decimal::ONE_HUNDRED {
            return Err(StrategyError::SharesDoNotSumToWhole);
        }
        Ok(())
    }

    /// The ladder this strategy makes of one entry.
    ///
    /// Shares become quantities against `quantity`, and the last row takes
    /// whatever rounding left over — a ladder that protected 1.99 of two
    /// contracts would leave a sliver of the position naked, which is the
    /// one outcome a protective ladder may never produce.
    ///
    /// # Errors
    ///
    /// [`StrategyError`] when the strategy is invalid, or when the ladder it
    /// resolves to is one the simulator refuses.
    pub(crate) fn resolve(
        &self,
        side: Side,
        entry: Decimal,
        quantity: Decimal,
        tick: Decimal,
    ) -> Result<Bracket, StrategyError> {
        self.validate()?;
        let mut parts: Vec<ExitPart> = Vec::with_capacity(self.rows.len());
        let mut assigned = Decimal::ZERO;
        for (index, row) in self.rows.iter().enumerate() {
            let last = index + 1 == self.rows.len();
            let share = if last {
                quantity.saturating_sub(assigned)
            } else {
                let raw = quantity.saturating_mul(row.share_percent) / Decimal::ONE_HUNDRED;
                // Round to the quantity's own precision so a third of three
                // contracts is one contract, not 0.999999.
                raw.round_dp(quantity.scale())
            };
            assigned = assigned.saturating_add(share);
            if share <= Decimal::ZERO {
                // The instrument is too small to split this finely. Skipping
                // the rung is honest: the remaining rows still cover the
                // whole quantity, because the last one takes the remainder.
                continue;
            }
            parts.push(ExitPart {
                quantity: Some(share),
                stop_loss: row
                    .loss_ticks
                    .map(|ticks| offset(side, entry, tick, ticks, true)),
                take_profit: row
                    .gain_ticks
                    .map(|ticks| offset(side, entry, tick, ticks, false)),
            });
        }
        Bracket::ladder(&parts).map_err(StrategyError::Ladder)
    }
}

/// A level `ticks` away from `entry`, on the losing side for a stop and the
/// winning side for a target.
fn offset(side: Side, entry: Decimal, tick: Decimal, ticks: u32, losing: bool) -> Decimal {
    let distance = tick.saturating_mul(Decimal::from(ticks));
    let away = match (side, losing) {
        (Side::Buy, true) | (Side::Sell, false) => false,
        (Side::Buy, false) | (Side::Sell, true) => true,
    };
    if away {
        entry.saturating_add(distance)
    } else {
        entry.saturating_sub(distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dec(value: i64) -> Decimal {
        Decimal::from(value)
    }

    /// The strategy from the trader's own editor: half at eighty ticks with
    /// forty of risk, half at twenty with twenty-five.
    fn two_row() -> OrderStrategy {
        OrderStrategy {
            name: "Str ClaudeVEr".to_owned(),
            rows: vec![
                StrategyRow {
                    share_percent: dec(50),
                    gain_ticks: Some(80),
                    loss_ticks: Some(40),
                },
                StrategyRow {
                    share_percent: dec(50),
                    gain_ticks: Some(20),
                    loss_ticks: Some(25),
                },
            ],
        }
    }

    #[test]
    fn a_long_puts_targets_above_and_stops_below() {
        let bracket = two_row()
            .resolve(Side::Buy, dec(1000), dec(2), Decimal::ONE)
            .expect("valid");
        let parts: Vec<_> = bracket.parts().copied().collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].quantity, Some(Decimal::ONE));
        assert_eq!(parts[0].take_profit, Some(dec(1080)), "eighty ticks up");
        assert_eq!(parts[0].stop_loss, Some(dec(960)), "forty ticks down");
        assert_eq!(parts[1].take_profit, Some(dec(1020)));
        assert_eq!(parts[1].stop_loss, Some(dec(975)));
    }

    #[test]
    fn a_short_mirrors_every_level() {
        let bracket = two_row()
            .resolve(Side::Sell, dec(1000), dec(2), Decimal::ONE)
            .expect("valid");
        let parts: Vec<_> = bracket.parts().copied().collect();
        assert_eq!(parts[0].take_profit, Some(dec(920)), "eighty ticks down");
        assert_eq!(parts[0].stop_loss, Some(dec(1040)), "forty ticks up");
        assert_eq!(parts[1].take_profit, Some(dec(980)));
        assert_eq!(parts[1].stop_loss, Some(dec(1025)));
    }

    #[test]
    fn the_tick_is_the_instruments_own_and_not_a_whole_point() {
        // A two-decimal instrument: one tick is 0.01, so forty ticks is 0.40.
        let bracket = two_row()
            .resolve(
                Side::Buy,
                Decimal::new(10_000, 2),
                dec(2),
                Decimal::new(1, 2),
            )
            .expect("valid");
        let parts: Vec<_> = bracket.parts().copied().collect();
        assert_eq!(parts[0].stop_loss, Some(Decimal::new(9_960, 2)));
        assert_eq!(parts[0].take_profit, Some(Decimal::new(10_080, 2)));
    }

    #[test]
    fn the_last_row_takes_the_rounding_so_nothing_is_left_naked() {
        let thirds = OrderStrategy {
            name: "thirds".to_owned(),
            rows: vec![
                StrategyRow {
                    share_percent: Decimal::new(3333, 2),
                    gain_ticks: Some(10),
                    loss_ticks: Some(10),
                },
                StrategyRow {
                    share_percent: Decimal::new(3333, 2),
                    gain_ticks: Some(20),
                    loss_ticks: Some(10),
                },
                StrategyRow {
                    share_percent: Decimal::new(3334, 2),
                    gain_ticks: Some(30),
                    loss_ticks: Some(10),
                },
            ],
        };
        let bracket = thirds
            .resolve(Side::Buy, dec(1000), dec(3), Decimal::ONE)
            .expect("valid");
        let covered: Decimal = bracket
            .parts()
            .filter_map(|part| part.quantity)
            .sum::<Decimal>();
        assert_eq!(covered, dec(3), "every contract is covered: {bracket:?}");
    }

    #[test]
    fn a_runner_row_may_carry_a_stop_and_no_target() {
        let runner = OrderStrategy {
            name: "runner".to_owned(),
            rows: vec![
                StrategyRow {
                    share_percent: dec(50),
                    gain_ticks: Some(20),
                    loss_ticks: Some(20),
                },
                StrategyRow {
                    share_percent: dec(50),
                    gain_ticks: None,
                    loss_ticks: Some(20),
                },
            ],
        };
        let bracket = runner
            .resolve(Side::Buy, dec(1000), dec(2), Decimal::ONE)
            .expect("valid");
        let parts: Vec<_> = bracket.parts().copied().collect();
        assert_eq!(parts[1].take_profit, None, "the runner has no target");
        assert_eq!(parts[1].stop_loss, Some(dec(980)), "but it is protected");
    }

    #[test]
    fn the_shares_must_add_up_and_say_so_when_they_do_not() {
        let mut strategy = two_row();
        strategy.rows[1].share_percent = dec(40);
        assert_eq!(
            strategy.validate(),
            Err(StrategyError::SharesDoNotSumToWhole)
        );
        assert_eq!(
            strategy.resolve(Side::Buy, dec(1000), dec(2), Decimal::ONE),
            Err(StrategyError::SharesDoNotSumToWhole),
            "and resolving refuses for the same reason"
        );
    }

    #[test]
    fn a_row_that_protects_nothing_is_refused() {
        let mut strategy = two_row();
        strategy.rows[1].gain_ticks = None;
        strategy.rows[1].loss_ticks = None;
        assert_eq!(strategy.validate(), Err(StrategyError::RowProtectsNothing));
    }

    #[test]
    fn a_fifth_row_is_refused_before_it_reaches_the_simulator() {
        let row = StrategyRow {
            share_percent: dec(20),
            gain_ticks: Some(10),
            loss_ticks: Some(10),
        };
        let strategy = OrderStrategy {
            name: "five".to_owned(),
            rows: vec![row.clone(), row.clone(), row.clone(), row.clone(), row],
        };
        assert_eq!(strategy.validate(), Err(StrategyError::TooManyRows));
    }

    #[test]
    fn an_empty_strategy_is_refused() {
        let strategy = OrderStrategy {
            name: "nothing".to_owned(),
            rows: Vec::new(),
        };
        assert_eq!(strategy.validate(), Err(StrategyError::Empty));
    }
}
