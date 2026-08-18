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
    /// The entry reference price (the trigger bar's close — a market order
    /// meets the tape at the next print, but the *projection* anchors on
    /// the fact the trigger measured).
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

    /// One human-readable line for badges and tooltips: why the trigger is
    /// or is not firing ("warmup 7/20", "quiet 0.8×", "force 1.9×").
    fn status(&self) -> String;

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
        let signal = match &verdict {
            BarVerdict::Force(force) => Some(Signal {
                side: force.side,
                reference: bar.close,
                projection: force.range,
            }),
            _ => None,
        };
        self.last = Some(verdict);
        signal
    }

    fn reset(&mut self) {
        self.window = ForceWindow::new(self.window.params().clone());
        self.last = None;
    }

    fn warmup_bars(&self) -> usize {
        self.window.params().window
    }

    fn status(&self) -> String {
        match &self.last {
            None => format!("waiting for bars 0/{}", self.window.params().window),
            Some(BarVerdict::Warmup { seen, window }) => format!("warmup {seen}/{window}"),
            Some(BarVerdict::FlatAverage) => "flat average — no ruler".to_owned(),
            Some(BarVerdict::NoSide) => "doji — no side".to_owned(),
            Some(BarVerdict::Quiet { ratio }) => format!("quiet {}×", round_ratio(*ratio)),
            Some(BarVerdict::Force(force)) => format!("force {}×", round_ratio(force.ratio)),
            Some(BarVerdict::UnderFloor { ratio, body }) => {
                // The band said force; the absolute floor said no. Saying
                // "quiet" here would hide the one number the trader needs.
                format!("{}× in band · body {body} under floor", round_ratio(*ratio))
            }
            Some(BarVerdict::Exhaustion { ratio, .. }) => {
                format!("exhaustion {}×", round_ratio(*ratio))
            }
        }
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
            min_body: Decimal::ZERO,
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
            min_body: Decimal::ZERO,
        });
        assert_eq!(trigger.status(), "waiting for bars 0/2");
        trigger.on_closed_bar(&bar("100", "101"));
        assert_eq!(trigger.status(), "warmup 1/2");
        trigger.on_closed_bar(&bar("100", "101"));
        assert_eq!(trigger.status(), "quiet 1×");
    }
}
