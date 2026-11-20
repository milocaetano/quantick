//! A tab's market owner: the strategies armed on its panes and the alarm
//! sounds they raised, walked through each live print.
//!
//! The window keeps one [`TabMarket`] per tab and hands it every ordinary
//! live print with the owners only the window has — the paper account, the
//! panes, the alert cache — behind [`LiveTradeOwners`]. The market walks the
//! [`LiveTradePlan`](quantick_chart_interaction::live_trade_plan::LiveTradePlan)
//! stages: each bar a pane closes is queued for the strategies armed on that
//! pane's id, the clock is read only when a strategy carries an alarm, and
//! the sounds the sweep raises are parked here until the window drains them
//! once per frame. Told the time by the clock it is handed, never reading
//! one itself.

use quantick_chart_interaction::live_trade_plan::LiveTradeStage;
use quantick_engine::Trade;
use quantick_strategy::sound::Cue;

use crate::live_trade::LiveTradeOwners;
use crate::strategies::TabStrategies;

/// One tab's armed strategies and the alarm sounds waiting to be played.
pub struct TabMarket<D> {
    /// The strategies armed on the panes' drawings, keyed by pane id.
    pub strategies: TabStrategies<D>,
    /// Alarm sounds the strategies raised, oldest first.
    ///
    /// A queue rather than a direct call to the platform: the judgement is
    /// made in the per-trade sweep, where a tab must stay testable without a
    /// machine making noise and where blocking on an operating-system call
    /// would be on the tape's path.
    alarms: Vec<Cue>,
}

impl<D> Default for TabMarket<D> {
    fn default() -> Self {
        Self {
            strategies: TabStrategies::default(),
            alarms: Vec::new(),
        }
    }
}

impl<D: Copy + PartialEq> TabMarket<D> {
    /// The alarm sounds waiting to be played, oldest first.
    #[must_use]
    pub fn pending_alarms(&self) -> &[Cue] {
        &self.alarms
    }

    /// Take the alarm sounds waiting to be played, oldest first.
    pub fn drain_alarms(&mut self) -> std::vec::Drain<'_, Cue> {
        self.alarms.drain(..)
    }

    /// Carry `trade` through `owners` in `stages` order. Production passes
    /// the validated plan; a test may pass another order to expose what it
    /// breaks.
    pub fn execute_live_trade(
        &mut self,
        trade: &Trade,
        stages: impl IntoIterator<Item = LiveTradeStage>,
        owners: &mut impl LiveTradeOwners<Drawing = D>,
        mut clock: impl FnMut() -> i64,
    ) {
        for stage in stages {
            match stage {
                LiveTradeStage::PaperTrade => owners.paper_trade(trade),
                LiveTradeStage::PaneTrades => {
                    for pane in 0..owners.pane_count() {
                        let fold = owners.fold_pane(pane, trade);
                        for close in &fold.closes {
                            let (bar, poc, slot) = (&close.bar, close.poc, close.slot);
                            (self.strategies).queue_closed(fold.pane_id, bar, poc, slot, true);
                            owners.observe_alert(pane, trade, Some(close));
                        }
                        owners.observe_alert(pane, trade, None);
                    }
                }
                LiveTradeStage::StrategyEvaluation => {
                    // The per-print alarm gate: an idle chart never reads a clock.
                    let now_ms = if self.strategies.any_alarm() {
                        u64::try_from(clock()).unwrap_or(0)
                    } else {
                        0
                    };
                    let alarms = owners.evaluate_strategies(&mut self.strategies, now_ms);
                    self.alarms.extend(alarms);
                }
            }
        }
    }
}
