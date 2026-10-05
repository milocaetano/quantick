//! The ruler: the notches a wheel rolls through to size a bracket.
//!
//! One notch is a step in points, and the notch count is what the ticket and
//! the account both read - so rolling the ruler and typing the offset reach
//! the same price. The default step is derived by the account; what is here
//! is the winding, and the step a trader named for each instrument.

use std::collections::BTreeMap;

use quantick_engine::Side;
use rust_decimal::Decimal;

use crate::format::fmt_decimal;

/// The smallest wheel travel that can still count as a notch.
///
/// A floor, not the notch itself: how many pixels a mouse reports per notch
/// is the mouse's business, not ours. This build guessed 50 and met a mouse
/// that reports 40 — under which every roll computed zero ticks and the
/// ruler silently refused to move. The notch is *learned* from the smallest
/// travel actually seen ([`Ruler::notch_px`]), and this floor only keeps a
/// trackpad's near-zero jitter from being mistaken for one.
pub const RULER_MIN_NOTCH_PX: f32 = 1.0;

/// The furthest the ruler walks from the aim, counted in *notches*.
///
/// A tick count cannot be the bound once a notch is worth more than a tick:
/// at five points a notch on a one-cent instrument, the second roll would
/// hit a 999-tick ceiling and the ruler would stop dead at ten points —
/// short of every distance it exists to measure. Two hundred rolls is a
/// wrist's worth of wheel in either direction, whatever the step is worth.
pub const RULER_MAX_NOTCHES: u32 = 200;

/// The ruler's whole state: how far it stands, the wheel travel it has not
/// spent yet, and the step each instrument was given.
#[derive(Debug, Clone, PartialEq)]
pub struct Ruler {
    /// How many *notches* the wheel has walked the projected bracket out
    /// from the aim. Sticky across aims within a session: a trader who
    /// decided their distance should not have to re-roll it for the next
    /// setup — but not across instruments, where the step itself changes.
    pub notches: u32,
    /// What the trader typed for this instrument's step, in points. Empty
    /// follows the instrument (see [`crate::PaperAccount::derived_ruler_step`]).
    pub step_text: String,
    /// The step each instrument was last given, in points, by symbol.
    ///
    /// Keyed by the bare symbol rather than by feed and symbol: the step
    /// describes the instrument's price geometry, not who streams it, and a
    /// recorded session must not make a trader relearn their wheel. The
    /// journal is already keyed this way.
    pub steps: BTreeMap<String, Decimal>,
    /// Sub-notch wheel travel not yet worth a tick (a trackpad's scroll
    /// arrives in fractions of a notch).
    pub travel_px: f32,
    /// Whether the wheel has ever been rolled over an aim this session.
    ///
    /// Only the hint under the aim's label reads it: an affordance nobody
    /// can see needs saying once, and saying it forever is clutter a trader
    /// has to look past on every aim they take.
    pub rolled: bool,
    /// How much travel this pointing device reports for one notch, learned
    /// from the smallest roll seen rather than assumed.
    ///
    /// A mouse reports a fixed step per detent — 40 px here, 50 on the
    /// machine this was written on, something else on the next one — and a
    /// trackpad reports a continuous stream. Taking the smallest non-zero
    /// travel as the notch makes "one notch, one tick" true on all of them,
    /// and makes the first roll count instead of being swallowed.
    pub notch_px: f32,
}

impl Default for Ruler {
    fn default() -> Self {
        Self {
            notches: 0,
            step_text: String::new(),
            steps: BTreeMap::new(),
            travel_px: 0.0,
            rolled: false,
            notch_px: f32::INFINITY,
        }
    }
}

impl Ruler {
    /// Spend this frame's wheel travel on the ruler, one notch per notch.
    ///
    /// Rolling up widens the bracket, rolling down narrows it; at zero the
    /// ruler is off and the order rests as bare as it always did.
    pub fn step(&mut self, scroll_y: f32) {
        // Learn this device's notch from the smallest roll it has produced,
        // so the very first roll moves a tick instead of vanishing into an
        // assumption about how a wheel is built.
        let travel = scroll_y.abs();
        if travel >= RULER_MIN_NOTCH_PX && travel < self.notch_px {
            self.notch_px = travel;
        }
        let notch = if self.notch_px.is_finite() {
            self.notch_px
        } else {
            return;
        };
        self.travel_px += scroll_y;
        let notches = (self.travel_px / notch).trunc();
        if notches == 0.0 {
            return;
        }
        self.travel_px -= notches * notch;
        let stepped = i64::from(self.notches) + notches as i64;
        self.notches = stepped.clamp(0, i64::from(RULER_MAX_NOTCHES)) as u32;
    }

    /// Put the ruler at `notches`, clamped to what the wheel itself can
    /// reach, and answer with where it landed.
    ///
    /// The named form of rolling the wheel: same field, same bound, so a
    /// hand and a second operator can never leave the ruler somewhere the
    /// other cannot.
    pub fn set_ticks(&mut self, notches: u32) -> u32 {
        self.notches = notches.min(RULER_MAX_NOTCHES);
        self.travel_px = 0.0;
        self.notches
    }

    /// Clear the ruler, so the next aim starts from the entry again; answers
    /// whether it was standing.
    ///
    /// A distance chosen for one setup silently arming the next order was
    /// the review's own finding; `Esc` is where a trader already asks for
    /// "never mind".
    pub fn clear(&mut self) -> bool {
        let stood = self.notches > 0;
        self.notches = 0;
        self.travel_px = 0.0;
        stood
    }

    /// How far one notch walks `symbol`'s ruler, in points: what the trader
    /// typed for it, else `derived`.
    ///
    /// A typed value is never silently corrected: it is theirs, and the
    /// field beside it is where a bad one is refused.
    #[must_use]
    pub fn step_for(&self, symbol: &str, derived: Decimal) -> Decimal {
        if let Some(step) = self.steps.get(symbol)
            && *step > Decimal::ZERO
        {
            return *step;
        }
        derived
    }

    /// Name `symbol`'s step, in points. A value that is not positive clears
    /// it, which puts the instrument back on the derived default.
    pub fn set_step(&mut self, symbol: &str, step: Option<Decimal>) {
        match step.filter(|value| *value > Decimal::ZERO) {
            Some(value) => {
                self.steps.insert(symbol.to_owned(), value);
            }
            None => {
                self.steps.remove(symbol);
            }
        }
    }

    /// Replace the remembered steps wholesale, from the sidecar, and show
    /// `symbol`'s in the step field.
    pub fn set_steps(&mut self, steps: BTreeMap<String, Decimal>, symbol: &str) {
        self.steps = steps;
        self.show_step_of(symbol);
    }

    /// The account moved to `symbol`. The ruler goes with the instrument for
    /// the reason the tick does - see `PaperAccount::set_symbol` - unless
    /// this is the first symbol of the session (`arriving`), which is not a
    /// switch.
    pub fn follow_symbol(&mut self, symbol: &str, arriving: bool) {
        if !arriving {
            self.notches = 0;
        }
        self.travel_px = 0.0;
        self.show_step_of(symbol);
    }

    /// Put `symbol`'s remembered step in the field, or empty it.
    fn show_step_of(&mut self, symbol: &str) {
        self.step_text = self
            .steps
            .get(symbol)
            .map(|step| fmt_decimal(*step))
            .unwrap_or_default();
    }

    /// The ruler's projected protection around an aimed entry, `step`
    /// points a notch.
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
    #[must_use]
    pub fn levels(
        &self,
        side: Side,
        price: Decimal,
        step: Decimal,
    ) -> (Option<Decimal>, Option<Decimal>) {
        if self.notches == 0 {
            return (None, None);
        }
        let distance = step.saturating_mul(Decimal::from(self.notches));
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
