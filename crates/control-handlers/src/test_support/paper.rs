#![cfg(test)]

use super::FakeWindow;
use crate::trade::{PaperHost, PaperOwner, TradeDesk};
use quantick_civil::TzOffset;
use quantick_paper::PaperAccount;
use quantick_paper::format::PositionSummary;
use quantick_paper::report::ledger_state::{LedgerView, LedgerViewChange};
use quantick_paper::report::session_edit::{
    JournalEdit, JournalEditOutcome, JournalScope, SessionEditError,
};
use quantick_paper::risk_sizing::RiskState;
use quantick_session::paper::{SharedChange, TabPaper, TradeCommand, TradeOutcome};
use quantick_sim::Order;
use rust_decimal::Decimal;

/// The fake window keeps no paper desk: every trade action finds no
/// chart open.
impl PaperHost for FakeWindow {
    fn paper(&mut self) -> Box<dyn PaperOwner + '_> {
        Box::new(NoDesk)
    }
}

/// A paper owner with no chart open and an empty journal it never writes.
pub(crate) struct NoDesk;

impl PaperOwner for NoDesk {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        None
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        None
    }
    fn persist(&mut self, _change: SharedChange) {}
    fn journal_scope(&self) -> JournalScope {
        JournalScope {
            dir: std::path::PathBuf::from("no-such-trades-folder"),
            live: Vec::new(),
        }
    }
    fn journal_timezone(&self) -> TzOffset {
        TzOffset::new(0)
    }
    fn edit_journal(
        &mut self,
        _edit: &JournalEdit,
    ) -> Result<JournalEditOutcome, SessionEditError> {
        Err(SessionEditError::Io("the fake keeps no journal".to_owned()))
    }
}

/// A test's own paper owner is its own door, lent for the one call.
impl<T: PaperOwner> PaperHost for T {
    fn paper(&mut self) -> Box<dyn PaperOwner + '_> {
        Box::new(self)
    }
}

impl<T: PaperOwner + ?Sized> PaperOwner for &mut T {
    fn active_desk(&self) -> Option<&dyn TradeDesk> {
        (**self).active_desk()
    }
    fn active_desk_mut(&mut self) -> Option<&mut dyn TradeDesk> {
        (**self).active_desk_mut()
    }
    fn persist(&mut self, change: SharedChange) {
        (**self).persist(change);
    }
    fn journal_scope(&self) -> JournalScope {
        (**self).journal_scope()
    }
    fn journal_timezone(&self) -> TzOffset {
        (**self).journal_timezone()
    }
    fn edit_journal(&mut self, edit: &JournalEdit) -> Result<JournalEditOutcome, SessionEditError> {
        (**self).edit_journal(edit)
    }
    fn route_call(&mut self) -> Option<quantick_paper::route::RouteCall<'_>> {
        (**self).route_call()
    }
    fn account_route(
        &mut self,
    ) -> Option<quantick_control_schema::trade_account::AccountRouteResult> {
        (**self).account_route()
    }
    fn request_route(
        &mut self,
        request: quantick_paper::route::RouteRequest,
    ) -> Result<(), quantick_paper::route::Refusal> {
        (**self).request_route(request)
    }
    fn broker_write_block(
        &mut self,
        actor: quantick_paper::route::Actor,
    ) -> Option<quantick_paper::route::Refusal> {
        (**self).broker_write_block(actor)
    }
}

/// One paper desk, as a window hands the active tab's over: the tab's own
/// paper (an account and the desk's ruler and ticket around it), the Trades
/// ledger's view, and every outcome a control call ran through it, for a
/// parity test to hold against the tab's own surfaces.
pub(crate) struct PaperDesk {
    pub reports: Vec<quantick_paper::report::scope::ReportReceipt>,
    pub paper: TabPaper,
    pub view: LedgerView,
    pub outcomes: Vec<TradeOutcome>,
}

impl PaperDesk {
    pub fn new(account: PaperAccount) -> Self {
        Self {
            reports: Vec::new(),
            paper: TabPaper::new(account),
            view: LedgerView::default(),
            outcomes: Vec::new(),
        }
    }
}

impl std::ops::Deref for PaperDesk {
    type Target = TabPaper;
    fn deref(&self) -> &TabPaper {
        &self.paper
    }
}

impl std::ops::DerefMut for PaperDesk {
    fn deref_mut(&mut self) -> &mut TabPaper {
        &mut self.paper
    }
}

impl TradeDesk for PaperDesk {
    fn report_sources(
        &mut self,
        sources: Option<quantick_paper::report::SourceSelection>,
        _: TzOffset,
    ) -> quantick_paper::report::snapshot::ReportReadout {
        quantick_paper::report::snapshot::ReportReadout {
            open: sources.is_some(),
            loaded: false,
            source: sources.unwrap_or_default(),
            symbol: None,
            window: "everything saved".to_owned(),
            scope: None,
            trade_count: 0,
            unreadable_files: 0,
            skipped_rows: 0,
            net_points: Decimal::ZERO,
        }
    }
    fn reset_report(&mut self) {
        self.reports.clear();
    }
    fn show_report(&mut self, report: quantick_paper::report::scope::PreparedReport) {
        self.reports.push(report.receipt);
    }
    fn account(&self) -> &PaperAccount {
        &self.account
    }
    fn account_mut(&mut self) -> &mut PaperAccount {
        &mut self.paper.account
    }
    fn ruler_ticks(&self) -> u32 {
        self.desk.ruler.notches
    }
    fn set_ruler_ticks(&mut self, ticks: u32) {
        self.paper.desk.ruler.set_ticks(ticks);
    }
    fn execute(&mut self, command: TradeCommand) -> TradeOutcome {
        let outcome = self.paper.execute(command);
        self.outcomes.push(outcome.clone());
        outcome
    }
    fn mark_price(&self) -> Option<Decimal> {
        self.account.mark_price()
    }
    fn working_orders(&self) -> &[Order] {
        self.account.working_orders()
    }
    fn risk_report(&self) -> (RiskState, bool) {
        self.desk.risk_report(&self.account)
    }
    fn position_summary(&self) -> Option<PositionSummary> {
        self.account.position_summary()
    }
    fn selected_trade_index(&self) -> Option<usize> {
        None
    }
    fn ledger_view(&mut self, change: &LedgerViewChange) -> (LedgerView, bool) {
        let moved = self.view.apply(change);
        (self.view.clone(), moved)
    }
}
