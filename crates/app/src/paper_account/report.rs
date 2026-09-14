//! The seam between the account and the performance report.
//!
//! The window, the calendar and the trades tab live in `paper_report`. What
//! is here is what only this host knows - the journal folder, the symbol and
//! the venue - gathered into a `ReportEnv` and handed over, plus the
//! one-line wrappers that keep every control-plane and harness name exactly
//! where its callers already look.

use super::{PaperAccount, account_env};
use crate::paper_calendar::DaySelection;
use crate::paper_report::LedgerScope;
use crate::timezone::TzOffset;

impl PaperAccount {
    // ------------------------------------------------------------------
    // Report and ledger
    //
    // The window, the calendar and the trades tab live in `paper_report`.
    // What stays here is the seam: this host owns the journal folder, the
    // symbol and the venue, so it gathers those into a `ReportEnv` and
    // hands them over. Every wrapper below is one line for that reason and
    // not because a layer was added for its own sake - the control plane
    // and the harness hooks call these names, and a name the operator
    // already knows must not move because the code behind it did.
    // ------------------------------------------------------------------

    /// Test-only: the report state *and* the environment this host would
    /// hand it, together.
    ///
    /// Together deliberately. Every method on the state that reads the
    /// session takes the env, so a test needs both at once - and asking
    /// for them one at a time is exactly the borrow conflict `report_env!`
    /// exists to avoid. Destructuring is what makes the two borrows
    /// visibly disjoint.
    #[cfg(test)]
    pub(crate) fn report_parts(
        &mut self,
    ) -> (
        &mut crate::paper_report::ReportState,
        crate::paper_report::ReportEnv<'_>,
    ) {
        let open = self.open_row();
        let Self {
            report,
            symbol,
            dir,
            session_journal_paths,
            venue,
            ..
        } = self;
        (
            report,
            crate::paper_report::ReportEnv {
                symbol,
                dir,
                session_journal_paths,
                session_trades: venue.closed_trades(),
                open,
            },
        )
    }

    /// Test-only: the report state this host holds.
    ///
    /// The report's own tests moved out with the report, and a handful of
    /// them still need a host that journals to a real folder, because they
    /// are about the journal rather than the arithmetic. Reading, not
    /// reaching: the state's fields stay private to its own module, so a
    /// test in `paper_report` gets at them exactly the way that module
    /// does.
    #[cfg(test)]
    pub(crate) fn report_state(&self) -> &crate::paper_report::ReportState {
        &self.report
    }

    /// Test-only: the report state, mutably.
    #[cfg(test)]
    pub(crate) fn report_state_mut(&mut self) -> &mut crate::paper_report::ReportState {
        &mut self.report
    }

    /// The open position as the ledger's top row needs it, or `None` while
    /// flat. Three venue reads gathered into one value so the reader can
    /// never be handed two of the three.
    pub(crate) fn open_row(&self) -> Option<crate::paper_report::OpenRow> {
        let summary = self.position_summary()?;
        let held_ms = self
            .venue
            .mark_timestamp_ms()
            .zip(self.venue.position().map(|position| position.opened_ms))
            .map(|(mark, opened)| mark.saturating_sub(opened));
        Some(crate::paper_report::OpenRow {
            summary,
            mark_price: self.venue.mark_price(),
            held_ms,
        })
    }

    /// Open the report window (`QUANTICK_PAPER_REPORT_AUTOSTART`).
    pub(crate) fn autostart_report(&mut self) {
        let env = account_env!(self);
        self.report.autostart_report(&env);
    }

    /// Open the report with its month grid expanded (`QUANTICK_PAPER_CALENDAR`).
    pub(crate) fn autostart_calendar(&mut self, selection: DaySelection) {
        let env = account_env!(self);
        self.report.autostart_calendar(selection, &env);
    }

    /// Point the ledger at one instrument's saved history, or all of them.
    pub(crate) fn set_ledger_scope(&mut self, scope: LedgerScope) {
        self.report.set_ledger_scope(scope);
    }

    /// Fold every day in the ledger shut (`QUANTICK_LEDGER_FOLD`).
    pub(crate) fn autostart_folded_days(&mut self, tz: TzOffset) {
        let env = account_env!(self);
        self.report.autostart_folded_days(tz, &env);
    }

    /// Reveal `pages` pages of saved history (`QUANTICK_LEDGER_PAGES`).
    pub(crate) fn autostart_ledger_pages(&mut self, pages: usize) {
        self.report.autostart_ledger_pages(pages);
    }

    /// Open or collapse the report's trade list (`QUANTICK_PAPER_REPORT_LIST`).
    pub(crate) fn set_report_list_open(&mut self, open: bool) {
        self.report.set_report_list_open(open);
    }
}
