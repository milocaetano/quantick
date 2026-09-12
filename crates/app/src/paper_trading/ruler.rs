//! The ruler: the notches a drag rolls through to size a bracket.
//!
//! One step is a whole number of ticks, and the notch count is what the
//! ticket and the account both read - so rolling the ruler and typing the
//! offset reach the same price. The step itself is derived by the account;
//! what is here is the winding.

use std::collections::BTreeMap;

use quantick_engine::Side;
use rust_decimal::Decimal;

use super::{PaperTrading, RULER_MAX_NOTCHES, RULER_MIN_NOTCH_PX};
use crate::paper_chrome::fmt_decimal;

impl PaperTrading {
    /// Spend this frame's wheel travel on the ruler, one tick per notch.
    ///
    /// Rolling up widens the bracket, rolling down narrows it; at zero the
    /// ruler is off and the order rests as bare as it always did.
    pub(super) fn step_ruler(&mut self, scroll_y: f32) {
        // Learn this device's notch from the smallest roll it has produced,
        // so the very first roll moves a tick instead of vanishing into an
        // assumption about how a wheel is built.
        let travel = scroll_y.abs();
        if travel >= RULER_MIN_NOTCH_PX && travel < self.ruler_notch_px {
            self.ruler_notch_px = travel;
        }
        let notch = if self.ruler_notch_px.is_finite() {
            self.ruler_notch_px
        } else {
            return;
        };
        self.ruler_travel_px += scroll_y;
        let notches = (self.ruler_travel_px / notch).trunc();
        if notches == 0.0 {
            return;
        }
        self.ruler_travel_px -= notches * notch;
        let stepped = i64::from(self.ruler_notches) + notches as i64;
        self.ruler_notches = stepped.clamp(0, i64::from(RULER_MAX_NOTCHES)) as u32;
    }

    /// Put the ruler at `ticks`, clamped to what the wheel itself can reach,
    /// and answer with where it landed.
    ///
    /// The named form of rolling the wheel: same field, same bound, so a
    /// hand and a second operator can never leave the ruler somewhere the
    /// other cannot.
    pub(crate) fn set_ruler_ticks(&mut self, notches: u32) -> u32 {
        self.ruler_notches = notches.min(RULER_MAX_NOTCHES);
        self.ruler_travel_px = 0.0;
        self.ruler_notches
    }

    /// How far one notch walks this instrument's ruler, in points.
    ///
    /// What the trader typed for this symbol, else the derived default. A
    /// typed value is never silently corrected: it is theirs, and the field
    /// beside it is where a bad one is refused.
    #[must_use]
    pub(crate) fn ruler_step(&self) -> Decimal {
        if let Some(step) = self.ruler_steps.get(&self.account.symbol)
            && *step > Decimal::ZERO
        {
            return *step;
        }
        self.account.derived_ruler_step()
    }

    /// Name this instrument's step, in points. A value that is not positive
    /// clears it, which puts the instrument back on the derived default.
    pub(crate) fn set_ruler_step(&mut self, step: Option<Decimal>) {
        match step.filter(|value| *value > Decimal::ZERO) {
            Some(value) => {
                self.ruler_steps.insert(self.account.symbol.clone(), value);
            }
            None => {
                self.ruler_steps.remove(&self.account.symbol);
            }
        }
    }

    /// Every step the trader has named, by symbol, for the sidecar.
    pub(crate) fn ruler_steps(&self) -> &BTreeMap<String, Decimal> {
        &self.ruler_steps
    }

    /// Replace the remembered steps wholesale, from the sidecar.
    pub(crate) fn set_ruler_steps(&mut self, steps: BTreeMap<String, Decimal>) {
        self.ruler_steps = steps;
        self.ruler_step_text = self
            .ruler_steps
            .get(&self.account.symbol)
            .map(|step| fmt_decimal(*step))
            .unwrap_or_default();
    }

    /// Clear the ruler, so the next aim starts from the entry again.
    ///
    /// A distance chosen for one setup silently arming the next order was
    /// the review's own finding; `Esc` is where a trader already asks for
    /// "never mind".
    pub(crate) fn clear_ruler(&mut self) -> bool {
        let stood = self.ruler_notches > 0;
        self.ruler_notches = 0;
        self.ruler_travel_px = 0.0;
        stood
    }

    /// How far the ruler stands from the aim, in ticks; zero when it is off.
    #[must_use]
    pub(crate) fn ruler_ticks(&self) -> u32 {
        self.ruler_notches
    }

    /// The ruler's projected protection around an aimed entry.
    ///
    /// The same distance either side, always: what the trader reads is the
    /// trade at 1:1, and the question it answers is "is that distance worth
    /// it?" - asked before the order exists rather than after.
    ///
    /// It answers whatever the ticket is armed with, a strategy included:
    /// the ruler is a *compass*, and a trader deciding whether a setup is
    /// worth taking needs it most when they already have a ladder in mind.
    /// Standing it down under a strategy was a rule this module invented
    /// and the trader never asked for. `None` only while the ruler is at
    /// zero, and for a stop that would fall through zero on a cheap
    /// instrument.
    pub(super) fn ruler_levels(
        &self,
        side: Side,
        price: Decimal,
    ) -> (Option<Decimal>, Option<Decimal>) {
        if self.ruler_notches == 0 {
            return (None, None);
        }
        let distance = self
            .ruler_step()
            .saturating_mul(Decimal::from(self.ruler_notches));
        let (stop, target) = match side {
            Side::Buy => (
                price.saturating_sub(distance),
                price.saturating_add(distance),
            ),
            Side::Sell => (
                price.saturating_add(distance),
                price.saturating_sub(distance),
            ),
        };
        if stop <= Decimal::ZERO || target <= Decimal::ZERO {
            return (None, None);
        }
        (Some(stop), Some(target))
    }
}
