//! The window's adapter onto the tab-scoped and attention ports of
//! `quantick_control_handlers`: each act is the one the window's own button
//! makes, on the window as the gateway holds it.

use quantick_control_handlers::{
    TabDirectory,
    trade::{TradeAccess, TradeDesk, TradePort},
    history::HistoryPort,
    journal_sessions::JournalSessionsPort,
    notify::AttentionPort,
    pnl_display::PnlDisplayPort,
    recovery::{FeedRecoveryPort, RecoveredFeed},
};
use quantick_control_schema::notify::AgentPopup;
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_session::{HistoryAct, HistoryOutcome, HistoryReply};

use crate::app::control_host::ControlPort;
use crate::app::paper_wiring::PaperSettingsChange;
use crate::control::ControlAccess;
use crate::paper_trading::PaperTrading;
use quantick_control_handlers::feed_source::{FeedSelection, FeedSourcePort};

impl FeedSourcePort for dyn ControlPort {
    fn feed_choices(&self) -> Vec<quantick_control_schema::feed_source::FeedSourceChoice> {
        quantick_control_schema::feed_source::choices(self.tab_reads().config())
    }
    fn feed_selection(&self, index: usize) -> Option<FeedSelection> {
        let tab = self.tab_reads().tab_at(index)?;
        Some(FeedSelection {
            feed_id: tab.active.0.clone(),
            symbol: tab.active.1.clone(),
            replay: tab.replay.is_some(),
        })
    }
    fn select_feed(&mut self, index: usize, feed_id: &str, symbol: &str) -> Option<FeedSelection> {
        let (tab, config) = self.tabs_mut().tab_with_config(index)?;
        (tab.feed_id, tab.symbol) = (feed_id.to_owned(), symbol.to_owned());
        tab.maybe_switch_feed(config);
        self.feed_selection(index)
    }
}

impl TabDirectory for dyn ControlPort {
    fn active_tab_index(&self) -> usize {
        self.tab_reads().active_tab_index()
    }
    fn tab_position(&self, tab_id: u64) -> Option<usize> {
        self.tab_reads().tabs().position(tab_id)
    }
    fn tab_id_at(&self, index: usize) -> u64 {
        self.tab_reads().tabs().id_at(index)
    }
}

impl FeedRecoveryPort for dyn ControlPort {
    fn recover_feed(&mut self, index: usize, keep_timeline: bool) -> Option<RecoveredFeed> {
        let (tab, config) = self.tabs_mut().tab_with_config(index)?;
        let respawned = if keep_timeline {
            tab.reconnect_feed(config)
        } else {
            tab.reload_feed(config)
        };
        Some(RecoveredFeed {
            symbol: tab.symbol.clone(),
            respawned,
        })
    }
}

impl HistoryPort for dyn ControlPort {
    fn history_paging(&self, index: usize) -> Option<bool> {
        let reads = self.tab_reads();
        let tab = reads.tab_at(index)?;
        Some(tab.capabilities(reads.config()).history_paging)
    }
    /// The toolbar's own press and cancel (Escape's too), so the two
    /// surfaces cannot judge one act two ways.
    fn dispatch_history(&mut self, index: usize, act: HistoryAct) -> Option<HistoryReply> {
        let (tab, outcome) = match act {
            HistoryAct::Press(reach) => {
                let (tab, press) = self.tabs_mut().press_history(index, reach)?;
                (tab, HistoryOutcome::Pressed(press))
            }
            HistoryAct::Cancel => {
                let tab = self.tabs_mut().tab_at_mut(index)?;
                let cancelled = tab.cancel_history();
                (tab, HistoryOutcome::Cancelled(cancelled))
            }
        };
        Some((outcome, tab.session.snapshot()))
    }
}

impl JournalSessionsPort for dyn ControlPort {
    fn journal_scope(&mut self) -> JournalScope {
        self.paper_settings().journal_scope()
    }
    fn journal_timezone(&mut self) -> crate::timezone::TzOffset {
        self.chrome_reads().timezone()
    }
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError> {
        self.paper_settings().apply_journal_edit(edit)
    }
    fn ledger_view(&mut self, change: &LedgerViewChange) -> Option<(LedgerView, bool)> {
        let paper = self.tabs_mut().active_paper_mut()?;
        Some(paper.account_mut().report_parts().0.ledger_view(change))
    }
}

impl PnlDisplayPort for dyn ControlPort {
    fn active_account(&mut self) -> Option<&mut quantick_paper::PaperAccount> {
        Some(self.tabs_mut().active_paper_mut()?.account_mut())
    }
    fn persist_pnl_display(&mut self) {
        self.paper_settings()
            .persist(crate::app::paper_wiring::PaperSettingsChange::RiskSettings);
    }
}

impl AttentionPort for dyn ControlPort {
    fn show_popup(&mut self, popup: AgentPopup) {
        self.alerts().show_popup(popup);
    }
    fn show_toast(&mut self, message: String) {
        self.alerts().show_toast(message);
    }
    fn sound_alert(&mut self) -> Option<String> {
        self.alerts().sound_alert()
    }
}

impl TradeDesk for PaperTrading {
    fn account(&self) -> &quantick_paper::PaperAccount {
        PaperTrading::account(self)
    }
    fn account_mut(&mut self) -> &mut quantick_paper::PaperAccount {
        PaperTrading::account_mut(self)
    }
    fn ruler_ticks(&self) -> u32 {
        PaperTrading::ruler_ticks(self)
    }
    fn set_ruler_ticks(&mut self, ticks: u32) {
        PaperTrading::set_ruler_ticks(self, ticks);
    }
    fn clear_ruler(&mut self) {
        PaperTrading::clear_ruler(self);
    }
    fn mark_price(&self) -> Option<rust_decimal::Decimal> {
        PaperTrading::mark_price(self)
    }
    fn working_orders(&self) -> &[quantick_sim::Order] {
        PaperTrading::working_orders(self)
    }
    fn armed_bracket(
        &self,
        side: quantick_engine::Side,
        reference: rust_decimal::Decimal,
        quantity: rust_decimal::Decimal,
    ) -> quantick_sim::Bracket {
        PaperTrading::armed_bracket(self, side, reference, quantity)
    }
}

impl TradePort for dyn ControlPort {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        self.tab_reads()
            .active_paper()
            .map(|paper| paper as &dyn TradeDesk)
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        self.tabs_mut()
            .active_paper_mut()
            .map(|paper| paper as &mut dyn TradeDesk)
    }
    fn persist_order_strategies(&mut self) {
        self.paper_settings()
            .persist(PaperSettingsChange::OrderStrategies);
    }
    fn persist_risk_settings(&mut self) {
        self.paper_settings()
            .persist(PaperSettingsChange::RiskSettings);
    }
}

impl TradeAccess for ControlAccess {
    fn record_event(&mut self, event: quantick_control_host::journal::NewEvent) {
        self.append_event(event);
    }
}
