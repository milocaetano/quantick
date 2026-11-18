//! The app's owners answering `quantick_control_handlers`' owner traits: the
//! tab's paper desk, a chart pane's analysis, and the control access's
//! journal.

use quantick_control_handlers::{JournalAccess, analysis::AnalysisPane, trade::TradeDesk};
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_session::paper::{TradeCommand, TradeOutcome};

use crate::control::ControlAccess;
use crate::pane::ChartPane;
use crate::paper_trading::PaperTrading;

// `**self` is the tab's paper (`quantick_session::paper`), never this trait.
impl TradeDesk for PaperTrading {
    fn account(&self) -> &quantick_paper::PaperAccount {
        (**self).account()
    }
    fn account_mut(&mut self) -> &mut quantick_paper::PaperAccount {
        (**self).account_mut()
    }
    fn ruler_ticks(&self) -> u32 {
        (**self).ruler_ticks()
    }
    fn set_ruler_ticks(&mut self, ticks: u32) {
        (**self).set_ruler_ticks(ticks);
    }
    fn execute(&mut self, command: TradeCommand) -> TradeOutcome {
        (**self).execute(command)
    }
    fn mark_price(&self) -> Option<rust_decimal::Decimal> {
        (**self).mark_price()
    }
    fn working_orders(&self) -> &[quantick_sim::Order] {
        (**self).working_orders()
    }
    fn risk_report(&self) -> (quantick_paper::risk_sizing::RiskState, bool) {
        (**self).risk_report()
    }
    fn position_summary(&self) -> Option<quantick_paper::format::PositionSummary> {
        (**self).position_summary()
    }
    fn selected_trade_index(&self) -> Option<usize> {
        (**self).account().selected_trade_index()
    }
    fn ledger_view(&mut self, change: &LedgerViewChange) -> (LedgerView, bool) {
        (**self).account_mut().report_parts().0.ledger_view(change)
    }
}

impl AnalysisPane for ChartPane {
    fn pane_id(&self) -> u64 {
        self.id
    }
    fn indicators(&self) -> &quantick_chart::indicator_views::IndicatorViews {
        &self.indicators
    }
    fn drawings(&self) -> &quantick_drawings::Drawings<quantick_drawings::DrawingTool> {
        &self.drawings
    }
}

impl JournalAccess for ControlAccess {
    fn record_event(
        &mut self,
        event: quantick_control_host::journal::NewEvent,
    ) -> quantick_control::wire::WireU64 {
        self.append_event(event)
    }
    fn now_unix_ms(&self) -> i64 {
        crate::metrics::wall_clock_ms()
    }
}
