//! The trigger port: what kind of closed bar pulls the trigger.
//!
//! One shipped implementation — the force bar — and a port so the next
//! ruler (the operational document's BEI, an imbalance-efficiency bar) can
//! dock without the state machine changing shape. The port is deliberately
//! bar-shaped: triggers judge **closed bars only**, because a signal that
//! repaints mid-bar is a signal the trader cannot audit afterwards.

use quantick_engine::{Bar, Side};
use rust_decimal::Decimal;

use crate::force::{BarVerdict, ForceParams, ForceWindow};

/// A trigger's verdict on one closed bar: fire in `side`'s direction,
/// projecting the bracket off `projection` around `reference`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signal {
    /// Direction the bar pushed.
    pub side: Side,
    /// The entry reference price, and **only** the price the bracket
    /// projects from: a market order meets the tape at the next print,
    /// while the projection anchors on the fact the trigger measured.
    ///
    /// The region test does not read this — it reads the bar's own open
    /// and close ([`Region::body_cut`]), because "the force bar cut the
    /// region" is a statement about the bar, not about a ruler's anchor.
    /// An implementation whose reference is *not* the judged bar's close
    /// therefore brackets around a price the entry will not fill near, and
    /// owes its consumers a word about that in its own documentation.
    ///
    /// [`Region::body_cut`]: crate::Region::body_cut
    pub reference: Decimal,
    /// The projection ruler: for the force bar, its full range.
    pub projection: Decimal,
}

/// The port. Feed **every** closed bar of the series in order, whether or
/// not the instance is armed — rulers keep running averages that must stay
/// warm across disarmed stretches.
pub trait Trigger {
    /// Judge one closed bar. `Some` means "this bar fires".
    fn on_closed_bar(&mut self, bar: &Bar) -> Option<Signal>;

    /// Judge a bar the series has **not** closed — the one still forming —
    /// without advancing any running state.
    ///
    /// This is the [signal alarm](crate::SignalAlarm)'s path, and only it.
    /// The verdict is provisional by construction: the bar keeps moving,
    /// and one that reads as force at 70% of its measure may not at its
    /// close. So it never reaches the state machine and never places an
    /// order — the trigger's own contract, that a signal the trader can
    /// audit afterwards comes from a closed bar, is untouched. What the
    /// preview buys is the seconds a trader needs to act on another
    /// platform, at the honest price of a reading labelled provisional.
    ///
    /// `&self` is the guarantee: a ruler cannot be corrupted by a bar that
    /// has not finished. A ruler with no honest provisional reading keeps
    /// the default `None`, and its consumer falls back to closed bars.
    fn preview(&self, bar: &Bar) -> Option<Signal> {
        let _ = bar;
        None
    }

    /// One human-readable line for badges and tooltips: why the trigger is
    /// or is not firing ("warmup 7/20", "quiet 0.8×", "force 1.9×").
    ///
    /// **It may only change when the ruler is advanced or reset** — that
    /// is, inside [`Self::on_closed_bar`] or [`Self::reset`], never on a
    /// clock and never per print. Consumers cache it: `ArmedStrategy` holds
    /// the text so the chart badge, which paints every frame, does not
    /// build a `String` sixty times a second. A ruler whose line moved
    /// between those calls would be quoted stale, which is the exact defect
    /// the badge was repaired to end. A ruler with a genuinely live reading
    /// belongs in [`Self::preview`], which is `&self` and uncached.
    fn status(&self) -> String;

    /// Why this ruler declined the last bar it judged, as a **stable name**
    /// rather than a sentence — or `None` when the bar fired.
    ///
    /// The twin of [`Self::status`], and the reason it exists separately:
    /// `status` is prose with numbers in it, written to be read by a
    /// person in a chart corner. An operator that is not looking at the
    /// screen — a script, a test, the assistant `CLAUDE.md` plans for —
    /// needs to know *that the ruler refused this bar* without parsing
    /// English out of a badge. Without this, "why did nothing happen here?"
    /// is answerable only by a human reading pixels, which is precisely
    /// what "operable without a hand" forbids.
    ///
    /// The names are stable ids, not display text: a surface may phrase
    /// them however it likes, but two builds must agree on the string.
    /// Same timing contract as [`Self::status`].
    fn refusal(&self) -> Option<&'static str> {
        None
    }

    /// Forget every bar seen: the series the ruler was measuring no longer
    /// exists (a rebuilt timeline, another bar spec, another market). A
    /// stateless trigger may keep the default no-op.
    fn reset(&mut self) {}

    /// How many recent closed bars re-warm this ruler after a [`reset`]
    /// (or a fresh arm) — the consumer replays that many from its series,
    /// gates shut, so "armed" means armed *now* instead of after another
    /// unexplained warmup. A stateless trigger needs none.
    ///
    /// [`reset`]: Self::reset
    fn warmup_bars(&self) -> usize {
        0
    }
}

/// The force-bar trigger: fires on a bar the [`ForceWindow`] rules force.
#[derive(Debug, Clone)]
pub struct ForceTrigger {
    window: ForceWindow,
    last: Option<BarVerdict>,
}

impl ForceTrigger {
    #[must_use]
    pub fn new(params: ForceParams) -> Self {
        Self {
            window: ForceWindow::new(params),
            last: None,
        }
    }

    /// The full verdict on the last bar, for callers that want more than
    /// the fired/not-fired answer (the chart's tooltip does).
    #[must_use]
    pub fn last_verdict(&self) -> Option<&BarVerdict> {
        self.last.as_ref()
    }
}

impl Trigger for ForceTrigger {
    fn on_closed_bar(&mut self, bar: &Bar) -> Option<Signal> {
        let verdict = self.window.classify(bar);
        let signal = signal_from(&verdict, bar);
        self.last = Some(verdict);
        signal
    }

    fn preview(&self, bar: &Bar) -> Option<Signal> {
        // `weigh` is `classify` without the fold, and `signal_from` is the
        // same mapping the closed-bar path uses. The provisional answer is
        // therefore the answer this bar would get if it closed right now —
        // which is exactly the claim the alarm makes on the trader's
        // behalf. The `last` verdict is deliberately not touched: the badge
        // narrates closed bars, and a forming bar overwriting it would make
        // the ruler's own status flicker with the tape.
        signal_from(&self.window.weigh(bar), bar)
    }

    fn reset(&mut self) {
        self.window = ForceWindow::new(self.window.params().clone());
        self.last = None;
    }

    fn warmup_bars(&self) -> usize {
        self.window.params().window
    }

    fn refusal(&self) -> Option<&'static str> {
        // One arm per verdict, and no catch-all: a sixth `BarVerdict` must
        // decide what it is called here rather than inheriting a
        // neighbour's name by falling through.
        match &self.last {
            None => Some("no bars judged yet"),
            Some(BarVerdict::Warmup { .. }) => Some("ruler still warming up"),
            Some(BarVerdict::FlatAverage) => Some("flat average — no ruler"),
            Some(BarVerdict::NoSide) => Some("doji — no side"),
            Some(BarVerdict::Quiet { .. }) => Some("body quiet against the average"),
            Some(BarVerdict::UnderFloor { .. }) => Some("candle under the floor"),
            Some(BarVerdict::Exhaustion { .. }) => Some("body above the band"),
            Some(BarVerdict::Force(_)) => None,
        }
    }

    fn status(&self) -> String {
        match &self.last {
            None => format!("waiting for bars 0/{}", self.window.params().window),
            Some(BarVerdict::Warmup { seen, window }) => format!("warmup {seen}/{window}"),
            Some(BarVerdict::FlatAverage) => "flat average — no ruler".to_owned(),
            Some(BarVerdict::NoSide) => "doji — no side".to_owned(),
            Some(BarVerdict::Quiet { ratio }) => format!("quiet {}×", round_ratio(*ratio)),
            Some(BarVerdict::Force(force)) => format!("force {}×", round_ratio(force.ratio)),
            Some(BarVerdict::UnderFloor { ratio, range }) => {
                // The band said force; the absolute floor said no. Saying
                // "quiet" here would hide the one number the trader needs —
                // and that number is the candle's size, because size is what
                // the floor actually measured. Printing the body here would
                // send the trader to change an input this gate never read.
                // `normalize` and not `round_dp`: this is a price span and
                // instruments differ by orders of magnitude. Rounding to two
                // places would print `candle 0.00` on a market whose whole
                // range is thousandths — worse than the trailing zeros it
                // fixes. Subtraction in `rust_decimal` keeps the operands'
                // scale, so an untouched span reaches the badge as
                // `0.10000000`.
                format!(
                    "{}× in band · candle {} under floor",
                    round_ratio(*ratio),
                    range.normalize()
                )
            }
            Some(BarVerdict::Exhaustion { ratio, .. }) => {
                format!("exhaustion {}×", round_ratio(*ratio))
            }
        }
    }
}

/// The one place a [`BarVerdict`] becomes a [`Signal`], shared by the
/// closed-bar path and the forming bar's preview. Two copies of this match
/// would let the alarm and the order disagree about the same bar — the
/// divergence a trader can only discover at the worst possible moment.
fn signal_from(verdict: &BarVerdict, bar: &Bar) -> Option<Signal> {
    match verdict {
        BarVerdict::Force(force) => Some(Signal {
            side: force.side,
            reference: bar.close,
            projection: force.range,
        }),
        _ => None,
    }
}

/// Two decimals is plenty for a badge; full precision stays in the verdict.
fn round_ratio(ratio: Decimal) -> Decimal {
    ratio.round_dp(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn bar(open: &str, close: &str) -> Bar {
        let open = dec(open);
        let close = dec(close);
        Bar {
            open_time: 0,
            close_time: 0,
            open,
            high: open.max(close) + Decimal::ONE,
            low: open.min(close) - Decimal::ONE,
            close,
            buy_volume: Decimal::ONE,
            sell_volume: Decimal::ONE,
            trade_count: 2,
        }
    }

    #[test]
    fn fires_only_on_force_and_reports_the_bars_facts() {
        let mut trigger = ForceTrigger::new(ForceParams {
            window: 3,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: Decimal::ZERO,
        });
        assert_eq!(trigger.on_closed_bar(&bar("100", "101")), None);
        assert_eq!(trigger.on_closed_bar(&bar("101", "102")), None);
        let signal = trigger
            .on_closed_bar(&bar("102", "106"))
            .expect("body 4 over average 2 is force");
        assert_eq!(signal.side, Side::Buy);
        assert_eq!(signal.reference, dec("106"));
        // Range: high 107, low 101.
        assert_eq!(signal.projection, dec("6"));
        assert_eq!(trigger.status(), "force 2×");
    }

    #[test]
    fn status_narrates_the_non_firing_states() {
        let mut trigger = ForceTrigger::new(ForceParams {
            window: 2,
            min_factor: dec("1.5"),
            max_factor: dec("2.5"),
            min_range: Decimal::ZERO,
        });
        assert_eq!(trigger.status(), "waiting for bars 0/2");
        trigger.on_closed_bar(&bar("100", "101"));
        assert_eq!(trigger.status(), "warmup 1/2");
        trigger.on_closed_bar(&bar("100", "101"));
        assert_eq!(trigger.status(), "quiet 1×");
    }
}
