//! Reference implementations of the strategy port.
//!
//! These exist so the harness has something real to run and so the port is
//! proven by more than its own definition. They are **not** the operational
//! rules: WP-03 brings those, over this same port, and nothing here should be
//! read as a claim about edge.
//!
//! Both of them answer one question per bar — *long, short, or flat?* — and
//! hand the answer to [`follow`], which is where the order decision actually
//! happens. That split is the point of the second implementation: swapping
//! the signal source (native EMAs for a compiled script) changes nothing
//! about how orders are placed.

use quantick_engine::Side;
use quantick_indicators::{
    Indicator, PlotId, SourceId,
    native::{Cvd, Ema},
};
use quantick_pine::{CompiledScript, PineError, ScriptIndicator};
use quantick_sim::{Bracket, Command};
use rust_decimal::Decimal;

use crate::strategy::{Account, BarView, Strategy};

/// Protective distances in points, applied to each entry.
///
/// `None` on a side means no protection there — the simulator never invents
/// a level, and neither does this. A rule with neither exits only when the
/// signal flips, which is a complete rule, just a patient one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Protection {
    /// Distance below (long) or above (short) the entry mark.
    pub stop_points: Option<Decimal>,
    /// Distance above (long) or below (short) the entry mark.
    pub target_points: Option<Decimal>,
}

impl Protection {
    /// Absolute prices for an entry on `side` taken around `mark`.
    ///
    /// Returns an empty bracket when there is no mark to measure from: a
    /// level computed from a price that does not exist would be a fiction,
    /// and the simulator would reject it anyway.
    #[must_use]
    pub fn bracket(self, side: Side, mark: Option<Decimal>) -> Bracket {
        let Some(mark) = mark else {
            return Bracket::none();
        };
        let (stop, target) = match side {
            Side::Buy => (
                self.stop_points.map(|d| mark - d),
                self.target_points.map(|d| mark + d),
            ),
            Side::Sell => (
                self.stop_points.map(|d| mark + d),
                self.target_points.map(|d| mark - d),
            ),
        };
        Bracket::whole(
            stop.filter(|price| *price > Decimal::ZERO),
            target.filter(|price| *price > Decimal::ZERO),
        )
    }
}

/// Turn "I want to be long / short / flat" into simulator commands.
///
/// This is the Rust half the dialect refuses to give a script: `strategy.*`
/// is rejected by the compiler with a stable error code, so a `.pine` file
/// can shape a signal and nothing else. Reversals close first and enter
/// after — two commands, applied in order on the next print — so the exit is
/// recorded as what it was rather than folded into a netting entry.
#[must_use]
pub fn follow(
    target: Option<Side>,
    quantity: Decimal,
    protection: Protection,
    account: &Account<'_>,
) -> Vec<Command> {
    let open = account.position.map(|position| position.side);
    match (open, target) {
        (None, None) => Vec::new(),
        (Some(held), Some(wanted)) if held == wanted => Vec::new(),
        (Some(_), None) => vec![Command::ClosePosition],
        (None, Some(side)) => vec![Command::PlaceMarket {
            side,
            quantity,
            bracket: protection.bracket(side, account.mark_price),
        }],
        (Some(_), Some(side)) => vec![
            Command::ClosePosition,
            Command::PlaceMarket {
                side,
                quantity,
                bracket: protection.bracket(side, account.mark_price),
            },
        ],
    }
}

/// The plot every single-series indicator writes its value to.
const VALUE: PlotId = PlotId::new(0);

/// A fast/slow EMA cross, optionally confirmed by the sign of cumulative
/// delta.
///
/// The signal is the one `crates/indicators/tests/bot_readiness.rs` proved a
/// headless consumer can read; this turns it into orders. Demonstration, not
/// doctrine.
pub struct EmaCross {
    name: String,
    fast: usize,
    slow: usize,
    /// Require CVD to agree with the direction before entering.
    confirm_with_flow: bool,
    quantity: Decimal,
    protection: Protection,
}

impl EmaCross {
    /// Slot order: fast EMA, slow EMA, CVD.
    const FAST: usize = 0;
    const SLOW: usize = 1;
    const FLOW: usize = 2;

    /// A cross of `fast` over `slow`, trading `quantity` per signal.
    ///
    /// # Panics
    ///
    /// Panics if either length is zero or `fast >= slow` — a "cross" of a
    /// series with itself has no direction, and the misconfiguration is
    /// worth failing loudly at construction rather than producing a run of
    /// silent non-signals.
    #[must_use]
    pub fn new(
        fast: usize,
        slow: usize,
        quantity: Decimal,
        protection: Protection,
        confirm_with_flow: bool,
    ) -> Self {
        assert!(fast > 0 && slow > 0, "EMA lengths must be positive");
        assert!(
            fast < slow,
            "the fast EMA must be shorter than the slow one"
        );
        Self {
            name: format!("ema-cross({fast}/{slow})"),
            fast,
            slow,
            confirm_with_flow,
            quantity,
            protection,
        }
    }
}

impl Strategy for EmaCross {
    fn name(&self) -> &str {
        &self.name
    }

    fn indicators(&mut self) -> Vec<Box<dyn Indicator>> {
        vec![
            Box::new(Ema::new(self.fast, SourceId::Close)),
            Box::new(Ema::new(self.slow, SourceId::Close)),
            Box::new(Cvd::new()),
        ]
    }

    fn on_bar(&mut self, view: &BarView<'_>) -> Vec<Command> {
        let fast_now = view.signals.latest(Self::FAST, VALUE);
        let slow_now = view.signals.latest(Self::SLOW, VALUE);
        let fast_before = view.signals.back(Self::FAST, VALUE, 1);
        let slow_before = view.signals.back(Self::SLOW, VALUE, 1);
        // `na` during warmup. A comparison against NaN is false either way,
        // but reading it explicitly says the absence was considered.
        if fast_now.is_nan() || slow_now.is_nan() || fast_before.is_nan() || slow_before.is_nan() {
            return Vec::new();
        }

        let crossed_up = fast_now > slow_now && fast_before <= slow_before;
        let crossed_down = fast_now < slow_now && fast_before >= slow_before;
        let side = if crossed_up {
            Side::Buy
        } else if crossed_down {
            Side::Sell
        } else {
            return Vec::new();
        };

        if self.confirm_with_flow {
            let flow = view.signals.latest(Self::FLOW, VALUE);
            let agrees = match side {
                Side::Buy => flow > 0.0,
                Side::Sell => flow < 0.0,
            };
            // NaN flow fails both comparisons, so an unknown reading is
            // treated as "not confirmed" rather than as agreement.
            if !agrees {
                return Vec::new();
            }
        }
        follow(Some(side), self.quantity, self.protection, &view.account)
    }
}

/// A strategy whose signal comes from a compiled Quantick Pine script.
///
/// The script plots a number; this reads column `plot` of it and treats
/// positive as long, negative as short, exactly zero as flat, and `na` as
/// "no opinion — hold whatever is open". Position sizing, protection and the
/// reversal rule stay in Rust, because that is where the dialect insists they
/// belong.
pub struct PlotSignal {
    name: String,
    compiled: CompiledScript,
    source: String,
    plot: PlotId,
    quantity: Decimal,
    protection: Protection,
}

impl PlotSignal {
    /// Compile `source` and take its signal from plot index `plot`.
    ///
    /// # Errors
    ///
    /// The compiler's own errors, which render with file, line and a stable
    /// code through [`PineError::render`].
    pub fn compile(
        name: &str,
        source: &str,
        plot: usize,
        quantity: Decimal,
        protection: Protection,
    ) -> Result<Self, Vec<PineError>> {
        let compiled = quantick_pine::compile(source, name)?;
        Ok(Self {
            name: format!("script({name})"),
            compiled,
            source: source.to_owned(),
            plot: PlotId::new(plot),
            quantity,
            protection,
        })
    }
}

impl Strategy for PlotSignal {
    fn name(&self) -> &str {
        &self.name
    }

    fn indicators(&mut self) -> Vec<Box<dyn Indicator>> {
        // Cloned rather than re-parsed: the compile is kept precisely so a
        // second session runs the script that was validated at startup, not
        // whatever the file says now.
        vec![Box::new(ScriptIndicator::new(
            self.compiled.clone(),
            self.source.clone(),
        ))]
    }

    fn on_bar(&mut self, view: &BarView<'_>) -> Vec<Command> {
        let signal = view.signals.latest(0, self.plot);
        if signal.is_nan() {
            return Vec::new();
        }
        let target = if signal > 0.0 {
            Some(Side::Buy)
        } else if signal < 0.0 {
            Some(Side::Sell)
        } else {
            None
        };
        follow(target, self.quantity, self.protection, &view.account)
    }
}

/// The armed-rectangle strategy, headless: the same `quantick-strategy`
/// kernel the chart arms on a drawing, driven here by a fixed price region
/// from configuration instead of a hand-drawn rectangle.
///
/// One brain, two consumers — the chart's paper trading and this harness
/// run identical trigger, gates and projections, which is what makes a
/// backtest of the mechanical half of the hybrid setup mean something. The
/// human half (where the region sits) is exactly the part the harness
/// cannot supply; a fixed region approximates it and the report must be
/// read with that difference in mind.
pub struct ForceRegion {
    name: String,
    region: quantick_strategy::Region,
    /// Kept to rebuild a fresh instance per session (`end_of_session`):
    /// bodies and a pending operation from one recorded day must not judge
    /// the next, the same rule the per-session simulator and indicator
    /// host already keep.
    params: quantick_strategy::StrategyParams,
    force: quantick_strategy::ForceParams,
    instance: quantick_strategy::ArmedStrategy,
}

impl ForceRegion {
    /// Arm the kernel on `region`, hunting `params.side` force bars.
    #[must_use]
    pub fn new(
        region: quantick_strategy::Region,
        params: quantick_strategy::StrategyParams,
        force: quantick_strategy::ForceParams,
    ) -> Self {
        let instance = quantick_strategy::ArmedStrategy::new(
            params.clone(),
            Box::new(quantick_strategy::ForceTrigger::new(force.clone())),
        );
        Self {
            name: format!("force-region({}..{})", region.low(), region.high()),
            region,
            params,
            force,
            instance,
        }
    }
}

impl Strategy for ForceRegion {
    fn name(&self) -> &str {
        &self.name
    }

    fn on_bar(&mut self, view: &BarView<'_>) -> Vec<Command> {
        // The recorded session is the region's whole life, so the time
        // window is always active. The flat gate mirrors the chart's:
        // no position and no resting order (queued market actions are
        // impossible here — the print that runs this bar's close drained
        // the queue before the bar could close).
        let flat = view.account.position.is_none() && view.account.orders.is_empty();
        self.instance
            .on_closed_bar(view.bar, &self.region, true, flat)
    }

    fn on_events(&mut self, events: &[quantick_sim::VenueEvent]) -> Vec<Command> {
        self.instance.on_sim_events(events)
    }

    fn end_of_session(&mut self) {
        self.instance = quantick_strategy::ArmedStrategy::new(
            self.params.clone(),
            Box::new(quantick_strategy::ForceTrigger::new(self.force.clone())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_sim::{Order, Position};

    fn account<'a>(position: Option<&'a Position>, orders: &'a [Order]) -> Account<'a> {
        Account {
            position,
            orders,
            mark_price: Some(Decimal::from(100)),
            mark_timestamp_ms: Some(0),
            closed_trades: 0,
            realized_points: Decimal::ZERO,
        }
    }

    fn long(quantity: u64) -> Position {
        Position {
            side: Side::Buy,
            quantity: Decimal::from(quantity),
            avg_price: Decimal::from(100),
            opened_ms: 0,
            opened_agg_id: 1,
            low_price: Decimal::from(100),
            high_price: Decimal::from(100),
            stop_loss: None,
            take_profit: None,
        }
    }

    #[test]
    fn holding_the_wanted_side_issues_nothing() {
        let held = long(1);
        let commands = follow(
            Some(Side::Buy),
            Decimal::ONE,
            Protection::default(),
            &account(Some(&held), &[]),
        );
        assert!(commands.is_empty(), "already long: {commands:?}");
    }

    #[test]
    fn a_flip_closes_before_it_enters() {
        let held = long(1);
        let commands = follow(
            Some(Side::Sell),
            Decimal::ONE,
            Protection::default(),
            &account(Some(&held), &[]),
        );
        assert_eq!(commands.len(), 2, "{commands:?}");
        assert_eq!(commands[0], Command::ClosePosition);
        assert!(matches!(
            commands[1],
            Command::PlaceMarket {
                side: Side::Sell,
                ..
            }
        ));
    }

    #[test]
    fn going_flat_closes_and_a_flat_account_wanting_nothing_stays_quiet() {
        let held = long(1);
        assert_eq!(
            follow(
                None,
                Decimal::ONE,
                Protection::default(),
                &account(Some(&held), &[])
            ),
            vec![Command::ClosePosition]
        );
        assert!(
            follow(
                None,
                Decimal::ONE,
                Protection::default(),
                &account(None, &[])
            )
            .is_empty()
        );
    }

    #[test]
    fn protection_is_measured_from_the_mark_and_needs_one() {
        let protection = Protection {
            stop_points: Some(Decimal::from(10)),
            target_points: Some(Decimal::from(20)),
        };
        let long_bracket = protection.bracket(Side::Buy, Some(Decimal::from(100)));
        assert_eq!(long_bracket.stop_loss(), Some(Decimal::from(90)));
        assert_eq!(long_bracket.take_profit(), Some(Decimal::from(120)));

        let short_bracket = protection.bracket(Side::Sell, Some(Decimal::from(100)));
        assert_eq!(short_bracket.stop_loss(), Some(Decimal::from(110)));
        assert_eq!(short_bracket.take_profit(), Some(Decimal::from(80)));

        assert!(
            protection.bracket(Side::Buy, None).is_empty(),
            "no mark, no invented levels"
        );
        // A target below zero is not a price; it is dropped rather than sent.
        let absurd = Protection {
            stop_points: None,
            target_points: Some(Decimal::from(500)),
        };
        assert!(
            absurd
                .bracket(Side::Sell, Some(Decimal::from(100)))
                .is_empty()
        );
    }

    #[test]
    #[should_panic(expected = "fast EMA must be shorter")]
    fn an_ema_cross_of_equal_lengths_is_refused_at_construction() {
        let _ = EmaCross::new(9, 9, Decimal::ONE, Protection::default(), false);
    }

    #[test]
    fn a_script_that_does_not_compile_reports_the_compiler_error() {
        let outcome = PlotSignal::compile(
            "broken",
            "indicator(\"x\")\nplot(",
            0,
            Decimal::ONE,
            Protection::default(),
        );
        // `PlotSignal` holds a compiled script and is deliberately not
        // `Debug`, so the error side is matched rather than unwrapped.
        let Err(errors) = outcome else {
            panic!("an unclosed call is not valid Pine");
        };
        assert!(!errors.is_empty());
        let rendered = errors[0].render("broken.pine", "indicator(\"x\")\nplot(");
        assert!(rendered.contains("broken.pine:2"), "{rendered}");
    }
}
