//! The loop: one recorded session, print by print, through the real engine.
//!
//! Nothing here is a backtest-flavoured copy of anything. The builder is the
//! chart's builder, the host is the chart's host, the simulator is the one
//! that fills paper trades on screen. What this module adds is the ordering —
//! and the ordering is the whole correctness argument, so it is spelled out
//! in [`run_session`] rather than left to be reconstructed from the code.
//!
//! `quantick_engine::golden::replay` is deliberately **not** used: it returns
//! bars detached from the prints that made them, and the simulator must see
//! every print, in order, interleaved with the bars.

use std::collections::BTreeMap;
use std::path::PathBuf;

use quantick_engine::Side;
use quantick_indicators::{IndicatorHost, InstanceId};
use quantick_replay::Session;
use quantick_sim::{ClosedTrade, PerformanceReport, RejectReason, SimEvent, Simulator};
use rust_decimal::Decimal;

use crate::bars::BarSpec;
use crate::strategy::{Account, BarView, Signals, Strategy};

/// Everything the tape refused, counted rather than swallowed.
///
/// A run that placed a thousand orders and had nine hundred rejected is not a
/// run with a hundred trades; it is a broken rule. Silence about that would
/// be the most expensive kind of dishonesty in this crate, so the counts
/// travel with the report and are printed beside it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Anomalies {
    /// Commands the simulator refused, by reason code.
    pub rejected: BTreeMap<&'static str, u64>,
    /// Protective prices dropped at fill time — the tape had already run
    /// past the level between the command and the fill — by reason code.
    pub brackets_dropped: BTreeMap<&'static str, u64>,
}

impl Anomalies {
    /// Total refusals.
    #[must_use]
    pub fn rejections(&self) -> u64 {
        self.rejected.values().sum()
    }

    /// Total dropped protective prices.
    #[must_use]
    pub fn dropped_brackets(&self) -> u64 {
        self.brackets_dropped.values().sum()
    }

    /// True when the run produced no refusal of any kind.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.rejected.is_empty() && self.brackets_dropped.is_empty()
    }

    /// Fold another run's counts into this one.
    pub fn absorb(&mut self, other: &Self) {
        for (code, count) in &other.rejected {
            *self.rejected.entry(code).or_default() += count;
        }
        for (code, count) in &other.brackets_dropped {
            *self.brackets_dropped.entry(code).or_default() += count;
        }
    }

    fn observe(&mut self, event: &SimEvent) {
        match event {
            SimEvent::Rejected(reason) => {
                *self.rejected.entry(reject_code(reason)).or_default() += 1;
            }
            SimEvent::BracketDropped { reason } => {
                *self
                    .brackets_dropped
                    .entry(reject_code(reason))
                    .or_default() += 1;
            }
            _ => {}
        }
    }
}

/// A stable snake_case token per refusal, for the report and the JSON trail.
///
/// `RejectReason` renders itself as a sentence for a human in the chart's
/// order ticket; a log wants a token that survives a reworded message.
#[must_use]
pub fn reject_code(reason: &RejectReason) -> &'static str {
    match reason {
        RejectReason::NoMarketPrice => "no_market_price",
        RejectReason::QuantityNotPositive => "quantity_not_positive",
        RejectReason::PriceNotPositive => "price_not_positive",
        RejectReason::LimitOnWrongSide(_) => "limit_on_wrong_side",
        RejectReason::StopOnWrongSide(_) => "stop_on_wrong_side",
        RejectReason::StopLossOnWrongSide(_) => "stop_loss_on_wrong_side",
        RejectReason::TakeProfitOnWrongSide(_) => "take_profit_on_wrong_side",
        RejectReason::NoPosition => "no_position",
        RejectReason::UnknownOrder(_) => "unknown_order",
    }
}

/// What one recorded session produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRun {
    /// `WINQ26 2026-08-12`, as the session library labels it.
    pub label: String,
    /// Instrument the recording declares.
    pub symbol: String,
    /// File the prints came from.
    pub path: PathBuf,
    /// Prints read.
    pub prints: usize,
    /// Bars the rule closed over them.
    pub bars: usize,
    /// Metrics over the round trips that completed.
    pub report: PerformanceReport,
    /// The round trips themselves, in the order they closed.
    pub trades: Vec<ClosedTrade>,
    /// What the tape refused.
    pub anomalies: Anomalies,
    /// The position still open when the recording ended — its side and
    /// quantity — or `None` if the session ended flat.
    ///
    /// It is **not** counted in `report`: no print proves an exit, and
    /// flattening at the last mark would invent a fill. The report names it
    /// instead of quietly rounding the day off, and names which way it was
    /// facing, because "a trade was left open" and "a *short* was left open
    /// into a rally" are not the same warning.
    pub open_at_end: Option<(Side, Decimal)>,
}

/// Run one strategy over one recorded session.
///
/// The order inside the loop is the contract:
///
/// 1. `sim.on_trade(print)` — resting orders, brackets and queued market
///    actions meet the print **before** anything looks at it. An order placed
///    on the previous bar fills here, which is why a decision can never act
///    on the print that triggered it.
/// 2. `builder.push(print)` — at most one bar closes per print.
/// 3. On a close: the host evaluates indicators, then the strategy is asked
///    for commands, then `sim.apply` queues them for the *next* print.
///
/// Indicator and simulator state are created here and dropped with the
/// session: nothing carries over into the next recorded day.
pub fn run_session(session: &Session, spec: BarSpec, strategy: &mut dyn Strategy) -> SessionRun {
    let mut builder = spec.build();
    let mut host = IndicatorHost::new();
    let slots: Vec<InstanceId> = strategy
        .indicators()
        .into_iter()
        .map(|indicator| host.add(indicator))
        .collect();
    let mut sim = Simulator::new();
    let mut anomalies = Anomalies::default();
    let mut bars = 0usize;

    for trade in &session.trades {
        let events = sim.on_trade(trade);
        for event in &events {
            anomalies.observe(event);
        }
        // Self-protection commands (a dropped bracket's close) apply now;
        // applying one emits nothing, so the echo terminates.
        for command in strategy.on_events(&events) {
            let events = sim.apply(command);
            for event in &events {
                anomalies.observe(event);
            }
            let _ = strategy.on_events(&events);
        }
        let Some(bar) = builder.push(trade) else {
            continue;
        };
        host.push_closed_bar(&bar);
        let index = bars;
        bars += 1;

        // The view borrows the host and the simulator, so it must be gone
        // before `apply` can mutate the simulator. The block is the seam.
        let commands = {
            let view = BarView {
                bar: &bar,
                index,
                signals: Signals::new(&host, &slots),
                account: Account {
                    position: sim.position(),
                    orders: sim.orders(),
                    mark_price: sim.mark_price(),
                    mark_timestamp_ms: sim.mark_timestamp_ms(),
                    closed_trades: sim.closed_trades().len(),
                    realized_points: sim.realized_points(),
                },
            };
            strategy.on_bar(&view)
        };
        for command in commands {
            let events = sim.apply(command);
            for event in &events {
                anomalies.observe(event);
            }
            let _ = strategy.on_events(&events);
        }
    }
    strategy.end_of_session();

    let trades = sim.closed_trades().to_vec();
    SessionRun {
        label: session.label(),
        symbol: session.symbol.clone(),
        path: session.path.clone(),
        prints: session.trades.len(),
        bars,
        report: PerformanceReport::from_trades(&trades),
        trades,
        anomalies,
        open_at_end: sim
            .position()
            .map(|position| (position.side, position.quantity)),
    }
}

/// How the per-session results are spread — the part an average hides.
///
/// A strategy with a good aggregate and one session carrying it is not the
/// same instrument as one that is flat every day, and the aggregate report
/// alone cannot tell them apart.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Distribution {
    /// Sessions run.
    pub sessions: usize,
    /// Sessions that ended net positive.
    pub up: usize,
    /// Sessions that ended net negative.
    pub down: usize,
    /// Sessions that traded and ended exactly flat.
    pub flat: usize,
    /// Sessions that produced no closed trade at all.
    pub without_trades: usize,
    /// Sessions still holding a position when the recording ended.
    pub open_at_end: usize,
    /// Best session by net points, and its label.
    pub best: Option<(String, Decimal)>,
    /// Worst session by net points, and its label.
    pub worst: Option<(String, Decimal)>,
    /// Median net points across the sessions that traded. `None` when none
    /// did — a median of nothing is not zero.
    pub median_net: Option<Decimal>,
}

/// A whole run: every session, the aggregate, and the spread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    /// The strategy that produced it.
    pub strategy: String,
    /// The bar rule it ran on.
    pub spec: BarSpec,
    /// Per-session results, in the order the sessions were run.
    pub sessions: Vec<SessionRun>,
    /// Metrics over every round trip of every session, concatenated in
    /// session order — so the aggregate drawdown is the equity curve of the
    /// whole period, not the worst single day.
    pub aggregate: PerformanceReport,
    /// How the sessions are spread.
    pub distribution: Distribution,
    /// Every session's refusals, summed.
    pub anomalies: Anomalies,
}

impl RunOutcome {
    /// Fold finished sessions into a run. Sessions keep the order they were
    /// run in, which the library scan already fixed as (symbol, date, path).
    #[must_use]
    pub fn from_sessions(strategy: &str, spec: BarSpec, sessions: Vec<SessionRun>) -> Self {
        let mut all_trades = Vec::new();
        let mut anomalies = Anomalies::default();
        let mut distribution = Distribution {
            sessions: sessions.len(),
            ..Distribution::default()
        };
        let mut nets: Vec<Decimal> = Vec::new();

        for session in &sessions {
            all_trades.extend(session.trades.iter().cloned());
            anomalies.absorb(&session.anomalies);
            if session.open_at_end.is_some() {
                distribution.open_at_end += 1;
            }
            if session.trades.is_empty() {
                distribution.without_trades += 1;
                continue;
            }
            let net = session.report.net_points;
            nets.push(net);
            match net.cmp(&Decimal::ZERO) {
                std::cmp::Ordering::Greater => distribution.up += 1,
                std::cmp::Ordering::Less => distribution.down += 1,
                std::cmp::Ordering::Equal => distribution.flat += 1,
            }
            let better = distribution
                .best
                .as_ref()
                .is_none_or(|(_, best)| net > *best);
            if better {
                distribution.best = Some((session.label.clone(), net));
            }
            let worse = distribution
                .worst
                .as_ref()
                .is_none_or(|(_, worst)| net < *worst);
            if worse {
                distribution.worst = Some((session.label.clone(), net));
            }
        }
        distribution.median_net = median(&mut nets);

        Self {
            strategy: strategy.to_owned(),
            spec,
            aggregate: PerformanceReport::from_trades(&all_trades),
            sessions,
            distribution,
            anomalies,
        }
    }
}

/// The median of the sessions that traded; the mean of the middle two when
/// the count is even (halving a decimal is exact, so this stays reproducible).
fn median(values: &mut [Decimal]) -> Option<Decimal> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[middle])
    } else {
        values[middle - 1]
            .checked_add(values[middle])
            .and_then(|sum| sum.checked_div(Decimal::TWO))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_nothing_is_not_zero() {
        assert_eq!(median(&mut []), None);
        assert_eq!(median(&mut [Decimal::from(3)]), Some(Decimal::from(3)));
        assert_eq!(
            median(&mut [Decimal::from(1), Decimal::from(4)]),
            Some(Decimal::from_str_exact("2.5").expect("a decimal"))
        );
        assert_eq!(
            median(&mut [Decimal::from(9), Decimal::from(1), Decimal::from(4)]),
            Some(Decimal::from(4)),
            "the input is sorted before the middle is taken"
        );
    }

    #[test]
    fn anomaly_counts_add_up_across_sessions() {
        let mut a = Anomalies::default();
        a.observe(&SimEvent::Rejected(RejectReason::NoMarketPrice));
        a.observe(&SimEvent::Rejected(RejectReason::NoMarketPrice));
        a.observe(&SimEvent::BracketDropped {
            reason: RejectReason::StopLossOnWrongSide(quantick_engine::Side::Buy),
        });
        assert_eq!(a.rejections(), 2);
        assert_eq!(a.dropped_brackets(), 1);
        assert!(!a.is_clean());

        let mut total = Anomalies::default();
        total.absorb(&a);
        total.absorb(&a);
        assert_eq!(total.rejections(), 4);
        assert_eq!(total.rejected.get("no_market_price"), Some(&4));
        assert_eq!(total.dropped_brackets(), 2);
        assert!(Anomalies::default().is_clean());
    }
}
