//! Renderer-independent readback of a filtered performance report.
use super::{HistoryRow, ReportPeriod, ReportView, SourceFilter};
use crate::format::PositionSummary;
use quantick_civil::DateRange;
use quantick_sim::ClosedTrade;
use quantick_sim::PerformanceReport;
use rust_decimal::Decimal;
use std::path::{Path, PathBuf};

pub struct ReportSnapshot<'a> {
    pub symbol: Option<&'a str>,
    pub source: SourceFilter,
    pub window: ReportWindow,
    pub hidden_outside: usize,
    pub hidden_by_source: usize,
    pub rows: &'a [HistoryRow],
    pub report: &'a PerformanceReport,
}

impl<'a> ReportSnapshot<'a> {
    pub fn new(symbol: Option<&'a str>, view: &'a ReportView) -> Self {
        Self {
            symbol,
            source: view.source,
            window: view
                .range
                .map_or(ReportWindow::Period(view.period), ReportWindow::Dates),
            hidden_outside: view.hidden_outside,
            hidden_by_source: view.hidden_by_source,
            rows: &view.rows,
            report: &view.report,
        }
    }
}

/// The selected dates supersede the relative period; both cannot apply together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportWindow {
    Dates(DateRange),
    Period(ReportPeriod),
}
impl ReportWindow {
    pub fn label(self) -> String {
        match self {
            Self::Dates(range) => range.label(),
            Self::Period(period) => period.phrase(),
        }
    }
}

/// The open position as the ledger's top row needs it: the summary, the
/// mark it is valued against, and how long it has been held.
///
/// One value rather than three reads. The ledger asks "is there a position
/// and what does its row say"; splitting that into a summary, a mark price
/// and a timestamp invites a caller to supply two of the three.
pub struct OpenRow {
    pub summary: PositionSummary,
    /// The price the open points are marked against; `None` before the
    /// first print.
    pub mark_price: Option<Decimal>,
    /// Mark time less open time; `None` when either is unknown. Not read
    /// from a clock — the tape says how long, so a replay's ages are the
    /// recording's, not this afternoon's.
    pub held_ms: Option<i64>,
    /// Where its trades come from: a live position is never unrecorded.
    pub source: quantick_sim::history::SessionSource,
}

/// What the report and the ledger read about the trading session they
/// belong to, borrowed for one call.
///
/// Every field here is something the host already knows and this module
/// has no way to learn: it holds no venue, opens no folder of its own and
/// reads no clock. Borrowed rather than copied, for `SurfaceEnv`'s reason
/// — a reader that kept its own copy of the symbol would answer for the
/// market the chart used to be on.
pub struct ReportEnv<'a> {
    /// The chart's instrument. Empty before a market is chosen, which is
    /// why every read of it tests for that rather than assuming a name.
    pub symbol: &'a str,
    /// The journal folder every surface here reads from.
    pub dir: &'a Path,
    /// Journal files this session is still writing. Excluded from the
    /// ledger's history load: those trades are already in the simulator,
    /// and reading them back would list every one of them twice.
    pub session_journal_paths: &'a [PathBuf],
    /// This session's closed round trips, oldest first — the ledger's live
    /// rows, and the only trades whose fills the tape on screen can prove.
    pub session_trades: &'a [ClosedTrade],
    /// Where each of those trades was journaled, index-aligned with them.
    pub session_trade_at: &'a [Option<crate::report::sessions::JournaledTrade>],
    /// The source each of those trades closed under, index-aligned.
    pub session_trade_sources: &'a [quantick_sim::history::SessionSource],
    /// The position still open, if there is one. Gathered by the host
    /// because it takes three reads of a venue this module cannot see.
    pub open: Option<OpenRow>,
    /// How every P&L figure here is worded: the ticket's own reader.
    pub pnl: crate::pnl::PnlReader<'a>,
}

impl<'a> ReportEnv<'a> {
    /// This session's closes, as the ledger's cut stretches their sessions'
    /// labels with them.
    #[must_use]
    pub fn live_closes(&self) -> super::ledger_state::LiveCloses<'a> {
        super::ledger_state::LiveCloses {
            trades: self.session_trades,
            at: self.session_trade_at,
            sources: self.session_trade_sources,
        }
    }
}
