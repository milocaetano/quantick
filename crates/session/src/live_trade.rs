//! One ordinary live print, carried through a tab's owners in plan order.
//!
//! [`LiveTradePlan`](quantick_chart_interaction::live_trade_plan::LiveTradePlan)
//! fixes the stage order; [`execute_live_trade`] walks it and makes the calls
//! that are not the window's to make: every pane folds the print, each bar a
//! pane closes is queued for the strategies armed on *that* pane's id and
//! shown to the alerts, and the clock is read only when a strategy carries an
//! alarm. The window owns the effects — the paper account, the panes, the
//! alert cache, the strategy sweep — and performs them through
//! [`LiveTradeOwners`]. Told the time by the clock it is handed, never
//! reading one itself, and only for a print an alarm is judged on.

use quantick_chart_interaction::live_trade_plan::LiveTradeStage;
use quantick_engine::{Bar, Trade};

use crate::strategies::TabStrategies;

/// A bar one print closed on one pane, at the composed slot it closed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveClose {
    pub bar: Bar,
    pub slot: usize,
}

/// What folding one print into one pane reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneFold {
    /// The pane the strategies are keyed by.
    pub pane_id: u64,
    /// The bars the print closed there, oldest first.
    pub closes: Vec<LiveClose>,
}

impl PaneFold {
    /// Pane `pane_id`'s fold, before it closed anything.
    #[must_use]
    pub fn new(pane_id: u64) -> Self {
        Self {
            pane_id,
            closes: Vec::new(),
        }
    }

    /// Record the bar the print closed at `slot`.
    pub fn close(&mut self, bar: &Bar, slot: usize) {
        self.closes.push(LiveClose {
            bar: bar.clone(),
            slot,
        });
    }
}

/// The effects one print asks of the window, one method per effect. Panes
/// are addressed by position: the flow pane first, then the context stack.
pub trait LiveTradeOwners {
    /// The drawing key the tab's strategies ride.
    type Drawing: Copy + PartialEq;
    /// The tab's armed strategies, where closes are queued and the alarm
    /// gate is read.
    fn strategies(&mut self) -> &mut TabStrategies<Self::Drawing>;
    /// The simulator fills against the print.
    fn paper_trade(&mut self, trade: &Trade);
    /// How many panes fold the print.
    fn pane_count(&self) -> usize;
    /// Fold the print into pane `pane`'s series, reporting what it closed.
    fn fold_pane(&mut self, pane: usize, trade: &Trade) -> PaneFold;
    /// Show the alerts pane `pane` after the print: once per `close`, then
    /// once with `None` for the print itself.
    fn observe_alert(&mut self, pane: usize, trade: &Trade, close: Option<&LiveClose>);
    /// Run the strategy sweep, alarms judged at `now_ms` (0 without one).
    fn evaluate_strategies(&mut self, now_ms: u64);
}

/// Carry `trade` through `owners` in `stages` order. Production passes the
/// validated plan; a test may pass another order to expose what it breaks.
pub fn execute_live_trade(
    trade: &Trade,
    stages: impl IntoIterator<Item = LiveTradeStage>,
    owners: &mut impl LiveTradeOwners,
    mut clock: impl FnMut() -> i64,
) {
    for stage in stages {
        match stage {
            LiveTradeStage::PaperTrade => owners.paper_trade(trade),
            LiveTradeStage::PaneTrades => {
                for pane in 0..owners.pane_count() {
                    let fold = owners.fold_pane(pane, trade);
                    for close in &fold.closes {
                        (owners.strategies()).queue_closed(fold.pane_id, &close.bar, close.slot);
                        owners.observe_alert(pane, trade, Some(close));
                    }
                    owners.observe_alert(pane, trade, None);
                }
            }
            LiveTradeStage::StrategyEvaluation => {
                // The per-print alarm gate: an idle chart never reads a clock.
                let now_ms = if owners.strategies().any_alarm() {
                    u64::try_from(clock()).unwrap_or(0)
                } else {
                    0
                };
                owners.evaluate_strategies(now_ms);
            }
        }
    }
}
