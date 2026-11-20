//! One ordinary live print, as the window's owners carry it.
//!
//! [`LiveTradePlan`](quantick_chart_interaction::live_trade_plan::LiveTradePlan)
//! fixes the stage order;
//! [`TabMarket::execute_live_trade`](crate::market::TabMarket::execute_live_trade)
//! walks it. The window owns the effects that are not the market's — the
//! paper account, the panes, the alert cache, the strategy sweep's view of
//! the panes — and performs them through [`LiveTradeOwners`].

use quantick_engine::{Bar, Trade};
use quantick_strategy::sound::Cue;
use rust_decimal::Decimal;

use crate::strategies::{LiveClose, TabStrategies};

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

    /// Record the bar the print closed at `slot`, with its footprint POC.
    pub fn close(&mut self, bar: &Bar, poc: Option<Decimal>, slot: usize) {
        self.closes.push(LiveClose {
            bar: bar.clone(),
            poc,
            slot,
        });
    }
}

/// The effects one print asks of the window, one method per effect. Panes
/// are addressed by position: the flow pane first, then the context stack.
pub trait LiveTradeOwners {
    /// The drawing key the tab's strategies ride.
    type Drawing: Copy + PartialEq;
    /// The simulator fills against the print.
    fn paper_trade(&mut self, trade: &Trade);
    /// How many panes fold the print.
    fn pane_count(&self) -> usize;
    /// Fold the print into pane `pane`'s series, reporting what it closed.
    fn fold_pane(&mut self, pane: usize, trade: &Trade) -> PaneFold;
    /// Show the alerts pane `pane` after the print: once per `close`, then
    /// once with `None` for the print itself.
    fn observe_alert(&mut self, pane: usize, trade: &Trade, close: Option<&LiveClose>);
    /// Run `strategies` against the print, alarms judged at `now_ms` (0
    /// without one); answers with the alarm sounds raised, oldest first.
    fn evaluate_strategies(
        &mut self,
        strategies: &mut TabStrategies<Self::Drawing>,
        now_ms: u64,
    ) -> Vec<Cue>;
}
