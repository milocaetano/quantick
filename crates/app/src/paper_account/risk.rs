//! Sizing the entry and pricing the protection around it.
//!
//! The arithmetic half of the account: how many contracts the capital and
//! the risk-per-trade setting allow, what the stop and target are worth in
//! money, where the bracket's two prices land, and the rounding rules the
//! answers are snapped to. Nothing here reaches a venue and nothing here
//! draws; an auditor asking "was the stop placed at the right price?" reads
//! this file for the price and [`super::orders`] for the placement.

use quantick_engine::Side;
use quantick_sim::{Bracket, OcoId, OrderIntent, OrderRole, Position};
use rust_decimal::Decimal;

use super::{AccountEnv, PaperAccount};
use crate::paper_chrome::fmt_decimal;

impl PaperAccount {
    // ------------------------------------------------------------------
    // Risk, sizing and the bracket an entry carries
    //
    // The seam's reason for existing. Every one of these used to reach into
    // the ticket for the ruler's notch count and the three typed boxes; each
    // now takes an [`AccountEnv`] instead, and none of them can see a pixel.
    // ------------------------------------------------------------------

    /// The bracket an entry would carry at this size: the ruler first, then
    /// the armed strategy, then the ticket's own offsets.
    ///
    /// The ruler leads because rolling it is the most recent thing the trader
    /// did and it is the answer they are looking at. Rolling back to zero puts
    /// the armed ladder in front again, so neither gesture costs the other.
    pub(crate) fn aim_bracket(
        &self,
        side: Side,
        price: Decimal,
        quantity: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) -> Bracket {
        if let Some((stop, target)) = env.ruler_levels {
            return Bracket::whole(Some(stop), Some(target));
        }
        if let Some(strategy) = self.selected_order_strategy() {
            // A strategy edited into an invalid state falls through to the
            // plainer sources rather than blocking the trade; the ticket
            // names the reason beside the selector.
            if let Ok(bracket) = strategy.resolve(side, price, quantity, self.tick()) {
                return bracket;
            }
        }
        ticket
    }

    /// What the risk per trade makes of an entry, and the bracket it would
    /// rest with.
    pub(crate) fn risk_sized(
        &self,
        side: Side,
        reference: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) -> (crate::risk_sizing::RiskState, Bracket) {
        crate::risk_sizing::sized_for_aim(
            &crate::risk_sizing::RiskContext {
                settings: &self.risk,
                capital: &self.capital,
                book: &self.instrument_money,
                symbol: &self.symbol,
            },
            side,
            reference,
            &|quantity| self.aim_bracket(side, reference, quantity, ticket, env),
        )
    }

    /// What the risk per trade says about the entry the aim is holding.
    pub(crate) fn risk_state(
        &self,
        side: Side,
        reference: Decimal,
        env: &AccountEnv,
    ) -> crate::risk_sizing::RiskState {
        let ticket = env.form.bracket(side, reference);
        self.risk_sized(side, reference, ticket, env).0
    }

    /// The risk read, and whether it blocks an entry.
    pub(crate) fn risk_report(&self, env: &AccountEnv) -> (crate::risk_sizing::RiskState, bool) {
        let reference = self.mark_price().unwrap_or_default();
        let state = self.risk_state(Side::Buy, reference, env);
        let blocks = state.blocks_entry(self.risk.lock);
        (state, blocks)
    }

    /// The bracket an armed entry would carry, at this size.
    pub(crate) fn armed_bracket(
        &self,
        side: Side,
        reference: Decimal,
        quantity: Decimal,
        env: &AccountEnv,
    ) -> Bracket {
        let ticket = env.form.bracket(side, reference);
        self.aim_bracket(side, reference, quantity, ticket, env)
    }

    /// The size and bracket an entry takes, or nothing with the reason
    /// posted to the outbox.
    pub(crate) fn entry_size(
        &mut self,
        side: Side,
        reference: Decimal,
        ticket: Bracket,
        env: &AccountEnv,
    ) -> Option<(Decimal, Bracket)> {
        let (state, resting) = self.risk_sized(side, reference, ticket, env);
        if state.blocks_entry(self.risk.lock) {
            self.set_toast(format!("SIM: {}", state.sentence()));
            return None;
        }
        if let Some(quantity) = state.derived_quantity() {
            return Some((quantity, resting));
        }
        // Off, or nothing to size against: the typed quantity still rules,
        // and a quantity that does not parse complains as it always did -
        // in the ticket's own words, carried here by the form.
        match env.form.quantity.clone() {
            Ok(quantity) => Some((
                quantity,
                self.aim_bracket(side, reference, quantity, ticket, env),
            )),
            Err(complaint) => {
                self.set_toast(complaint);
                None
            }
        }
    }

    /// Why the lock refuses this intent, when it does.
    ///
    /// Reads the intent's *own* protection and quantity rather than the
    /// ticket's: a named call states what it wants, and the ceiling has to
    /// be measured against what was actually asked for.
    pub(crate) fn risk_refusal_for(&self, intent: &OrderIntent) -> Option<String> {
        if !self.risk.lock || self.risk.basis == crate::risk_sizing::RiskBasis::Off {
            return None;
        }
        let reference = intent
            .price
            .or_else(|| self.venue.mark_price())
            .unwrap_or_default();
        let risk = crate::risk_sizing::risk_of(
            &self.instrument_money,
            &self.symbol,
            intent.side,
            reference,
            &intent.bracket,
            intent.quantity,
        )?;
        let budget = crate::risk_sizing::budget_for(
            &self.risk,
            &self.capital,
            &self.instrument_money.get(&self.symbol)?.currency,
        )
        .ok()?;
        (risk.amount > budget.amount).then(|| {
            format!(
                "this order risks {} {} - over your {} {} risk per trade. Raise the risk, or                  turn the lock off.",
                risk.amount.normalize(),
                risk.currency.code(),
                budget.amount.normalize(),
                budget.currency.code(),
            )
        })
    }

    /// Whether a launch hook owns the risk per trade for this run, in which
    /// case the stored settings must not be fanned back over it.
    #[must_use]
    pub(crate) fn risk_from_hook(&self) -> bool {
        self.risk_from_hook
    }

    /// What one trade may lose, and whether the lock stands.
    pub(crate) fn risk_settings(&self) -> &crate::risk_sizing::RiskSettings {
        &self.risk
    }

    /// Replace the risk per trade. App-wide, like the ticket's other
    /// settings: a ceiling a trader sets in one tab is one they mean in all.
    pub(crate) fn set_risk_settings(&mut self, risk: crate::risk_sizing::RiskSettings) {
        self.risk = risk;
    }

    /// The declared practice capital, one amount per currency.
    pub(crate) fn capital(&self) -> &crate::risk_sizing::Capital {
        &self.capital
    }

    /// Replace the declared capital.
    pub(crate) fn set_capital(&mut self, capital: crate::risk_sizing::Capital) {
        self.capital = capital;
    }

    /// What one point of each instrument is worth, by bare symbol.
    pub(crate) fn instrument_money(&self) -> &crate::risk_sizing::InstrumentBook {
        &self.instrument_money
    }

    /// Replace the declared instrument money.
    pub(crate) fn set_instrument_money(&mut self, book: crate::risk_sizing::InstrumentBook) {
        self.instrument_money = book;
    }

    /// What actually guards the open position, whatever shape it is in.
    ///
    /// The position's own `stop_loss`/`take_profit` answer only the plain
    /// pair; under a ladder they are `None` and the working legs carry the
    /// truth. Reading the pair alone left a laddered position drawn as if it
    /// had no protection at all *and* offering the create-handles of an
    /// unprotected one - and one drag on those replaces the whole ladder
    /// with a single level. Folding the legs back into a bracket here means
    /// every surface downstream sees the same shape for a position that it
    /// already sees for an order.
    ///
    /// Grouped by OCO id, which is what a rung *is*, and walked in placement
    /// order so the rungs read in the order the trader wrote them.
    pub(crate) fn position_bracket(&self, position: &Position) -> Bracket {
        let mut parts: Vec<(OcoId, quantick_sim::ExitPart)> = Vec::new();
        for leg in self
            .venue
            .working_orders()
            .iter()
            .filter(|order| order.is_protective())
        {
            let (Some(level), Some(oco)) = (leg.price, leg.oco) else {
                continue;
            };
            let part = match parts.iter_mut().find(|(id, _)| *id == oco) {
                Some((_, part)) => part,
                None => {
                    parts.push((
                        oco,
                        quantick_sim::ExitPart {
                            quantity: Some(leg.quantity),
                            stop_loss: None,
                            take_profit: None,
                        },
                    ));
                    &mut parts.last_mut().expect("just pushed").1
                }
            };
            match leg.role {
                OrderRole::StopLoss => part.stop_loss = Some(level),
                OrderRole::TakeProfit => part.take_profit = Some(level),
                OrderRole::Entry => {}
            }
        }
        if parts.is_empty() {
            return Bracket::whole(position.stop_loss, position.take_profit);
        }
        let rungs: Vec<quantick_sim::ExitPart> = parts.into_iter().map(|(_, part)| part).collect();
        Bracket::ladder(&rungs)
            .unwrap_or_else(|_| Bracket::whole(position.stop_loss, position.take_profit))
    }

    /// The step an instrument gets before anyone names one.
    ///
    /// Half a basis point of the mark, rounded *up* the 1-2-5 ladder to a
    /// whole number of ticks and never below one. Volatility scales with
    /// price, so the same fraction gives a wheel that feels the same on an
    /// instrument quoted at 78,000 and one quoted at 5.
    #[must_use]
    pub(crate) fn derived_ruler_step(&self) -> Decimal {
        let tick = self.tick();
        let Some(mark) = self.venue.mark_price() else {
            return tick;
        };
        let wanted = mark.saturating_mul(RULER_DEFAULT_STEP_FRACTION);
        if wanted <= tick {
            return tick;
        }
        // Climb the 1-2-5 ladder in units of a tick until it covers `wanted`,
        // so the step is always a whole number of ticks a price can land on.
        let mut multiple = Decimal::ONE;
        loop {
            for rung in [Decimal::ONE, Decimal::TWO, Decimal::from(5)] {
                let step = tick.saturating_mul(multiple).saturating_mul(rung);
                if step >= wanted {
                    return step;
                }
            }
            multiple = multiple.saturating_mul(Decimal::TEN);
            if multiple > Decimal::from(1_000_000) {
                return wanted.round_dp(tick.scale());
            }
        }
    }

    /// Round a pointer price to the precision the tape itself uses (the
    /// mark's decimal places), so a dragged line lands on a price the
    /// instrument can actually print.
    pub(crate) fn snap(&self, price: f64) -> Decimal {
        // The instrument's own precision, learned from the tape - not the
        // raw scale of the last print. A venue that quotes
        // `79172.37000000` has a raw scale of eight, and snapping to it put
        // eight decimals on every level this layer draws: `SL 79026.38465256`
        // where the instrument trades in cents. The tick is the same one the
        // ruler and the ladders step in, so every number on this surface
        // rounds the same way.
        let places = if self.venue.mark_price().is_some() {
            self.tick_scale
        } else {
            SNAP_FALLBACK_DECIMALS
        };
        Decimal::from_f64_retain(price)
            .unwrap_or_default()
            .round_dp(places)
            .normalize()
    }

    /// The preview the pointer and the held key describe this frame;
    /// `None` hides the overlay. Both keys down at once is ambiguous and
    /// shows nothing — so a shared binding degrades to "off", never to a
    /// wrong side.
    ///
    /// **The aim is the last claimant on the canvas.** Its target is the
    /// whole plot, so anything already holding the pixel outranks it: an
    /// annotation or the canvas chrome (the pane says so), an armed
    /// placement the trader is in the middle of, an overlay ✕ or bracket
    /// handle, and this module's own draggable lines. Standing the aim
    /// *down* rather than merely refusing its press is what keeps the
    /// promise: no preview means nothing paints, no hand cursor, no place
    /// — the label can never advertise an order the press will not make.
    /// One tick of the instrument: the smallest price move its own prints
    /// can express. Read from the mark the same way [`Self::snap`] reads it,
    /// so the ruler steps in exactly the units a drag would land on.
    #[must_use]
    pub(crate) fn tick_size(&self) -> Decimal {
        self.tick()
    }

    pub(crate) fn tick(&self) -> Decimal {
        let places = if self.venue.mark_price().is_some() {
            self.tick_scale
        } else {
            SNAP_FALLBACK_DECIMALS
        };
        Decimal::new(1, places)
    }

    /// The size one stepper press moves, for the hover that promises it.
    pub(crate) fn quantity_step_hint(&self, notches: Decimal) -> String {
        let unit = self
            .instrument_money
            .get(&self.symbol)
            .map_or(Decimal::ONE, |money| money.size_step);
        fmt_decimal(notches.saturating_mul(unit))
    }
}
/// The step a notch walks when the trader has named none, as a fraction of
/// the mark.
///
/// Half a basis point: volatility scales with price, so one fraction serves
/// an instrument quoted at 78,000 and one quoted at 5. It is rounded up the
/// 1-2-5 ladder to a whole number of ticks, which lands on 5 points for
/// BTCUSDT near 78,000 and 10 for the mini index near 138,000 — in both
/// cases a twenty-to-forty point read is four to eight rolls away.
const RULER_DEFAULT_STEP_FRACTION: Decimal = Decimal::from_parts(5, 0, 0, false, 5);

/// Price precision for snapped drags before any print reveals the
/// instrument's own (two decimals, the crypto-major default).
const SNAP_FALLBACK_DECIMALS: u32 = 2;
