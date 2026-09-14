//! The simulated performance report and the trades ledger: what the paper
//! session *produced*, read back from the journal on disk.
//!
//! This is the reading half of paper trading, and it is a separate module
//! from `paper_trading` because the writing half — placing orders,
//! projecting brackets, sizing against capital, journaling a close — is
//! the money path, and every line of calendar tinting and equity-curve
//! plotting sitting beside it is a line someone auditing "was the stop
//! placed at the right price?" has to read past. The two halves share a
//! journal folder and nothing else.
//!
//! Shaped after `paper_calendar`, which took the date law out of the same
//! file:
//!
//! - **It never reaches back.** Nothing here names `PaperTrading`. What
//!   the report reads about the live session — the chart's symbol, the
//!   journal folder, this session's own closed trades, the position still
//!   open — arrives as [`ReportEnv`], borrowed for the call, the way
//!   `SurfaceEnv` hands a floating surface what its host knows. What it
//!   wants *done* leaves as [`ReportResponse`], because a module that
//!   cannot open a file picker must not pretend to.
//! - **The state is one struct.** [`ReportState`] holds the twenty-one
//!   fields the report and the ledger used to spread across the trading
//!   host: the filters, the loaded history, the cut view, the calendar
//!   selection, the ledger's paging and folds.
//! - **Pure below the two entry points.** [`ReportState::draw_window`] and
//!   [`ReportState::draw_trades_tab`] are the only functions here that
//!   need a window. Everything under them — the cut, the equity walk, the
//!   day index, the paging arithmetic, the row layout — is plain functions
//!   over plain values, which is what makes them testable without one.
//!
//! **The numbers are pinned.** `the_report_numbers_are_fixed` at the foot
//! of this file asserts a fixed journal's whole report, byte for byte,
//! across two cuts. It was written before this module existed, against the
//! same code in its old home, and its expected text did not change when
//! the code moved. Data honesty is not a review preference here: this is
//! where a trader's own results are computed, and a rounding that shifts
//! during a refactor is the code lying to them about their trading.

use std::path::{Path, PathBuf};

use quantick_sim::{ClosedTrade, PerformanceReport};
use rust_decimal::Decimal;

// One date law for every trade surface - see `paper_calendar`.
use crate::paper_calendar::{CalendarState, DateRange, DayIndex};
use crate::paper_chrome::PositionSummary;
use crate::timezone::TzOffset;

mod curve;
mod ledger;
mod rows;
mod tables;
mod window;

use curve::EquityWalk;
use ledger::LedgerTotals;
/// The ledger's vocabulary, named by the ticket that hosts the tab.
pub(crate) use ledger::{HistoryRow, LedgerAction, LedgerScope, LoadedHistory, SourceFilter};
/// Named by the ticket's own tests, which drive a real journal folder.
#[cfg(test)]
pub(crate) use ledger::{load_history, report_from_history};
use window::{ReportPeriod, ReportWindow};

/// What the report and the ledger read about the trading session they
/// belong to, borrowed for one call.
///
/// Every field here is something the host already knows and this module
/// has no way to learn: it holds no venue, opens no folder of its own and
/// reads no clock. Borrowed rather than copied, for `SurfaceEnv`'s reason
/// — a reader that kept its own copy of the symbol would answer for the
/// market the chart used to be on.
pub(crate) struct ReportEnv<'a> {
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
    /// The position still open, if there is one. Gathered by the host
    /// because it takes three reads of a venue this module cannot see.
    pub open: Option<OpenRow>,
}

/// The open position as the ledger's top row needs it: the summary, the
/// mark it is valued against, and how long it has been held.
///
/// One value rather than three reads. The ledger asks "is there a position
/// and what does its row say"; splitting that into a summary, a mark price
/// and a timestamp invites a caller to supply two of the three.
pub(crate) struct OpenRow {
    pub summary: PositionSummary,
    /// The price the open points are marked against; `None` before the
    /// first print.
    pub mark_price: Option<Decimal>,
    /// Mark time less open time; `None` when either is unknown. Not read
    /// from a clock — the tape says how long, so a replay's ages are the
    /// recording's, not this afternoon's.
    pub held_ms: Option<i64>,
}

/// What the report asked its host to do.
///
/// One field today, and a struct rather than a `bool` because the reason
/// it exists is the rule and not the count: this module can decide that a
/// folder picker should open and must not be the thing that opens it.
#[derive(Default)]
pub(crate) struct ReportResponse {
    /// The trader pressed Import. The host owns the dialog and the copy.
    pub start_import: bool,
    /// An acknowledgement to show, if the window produced one. It leaves
    /// for the same outbox every other paper acknowledgement uses, which
    /// is the point: this module must not grow a second toast lane on a
    /// second clock - exactly the divergence the panel's private toast was
    /// converged away from.
    pub toast: Option<String>,
}

/// Fixed ledger row height — two lines plus their padding, held constant so
/// the trade list can virtualise through `ScrollArea::show_rows`.
const LEDGER_ROW_HEIGHT_PX: f32 = 34.0;
/// The side rail on ledger rows — the position HUD card's rail width.
const SIDE_RAIL_WIDTH_PX: f32 = 3.0;
/// Height reserved under the ledger for the pinned totals strip.
const TOTALS_STRIP_PX: f32 = 26.0;

/// Saved trades one ledger page reveals. A folder holding a year of
/// sessions must not paint a year of rows to show today's; the "show
/// older" control adds another page and says how many are left.
const LEDGER_PAGE_TRADES: usize = 50;

/// The ledger's instrument picker. Wide enough for "This chart · BTCUSDT"
/// without pushing the refresh and fold controls off the row.
const LEDGER_SCOPE_COMBO_PX: f32 = 150.0;

/// Gap between a detail line and the date stamp anchored opposite it.
const DETAIL_GAP_PX: f32 = 8.0;

/// Right margin the detail line and its stamp both respect.
const DETAIL_RIGHT_PAD_PX: f32 = 6.0;

/// How far a day header's tinted band sits below the row's top edge — the
/// gap is what separates one day's block from the previous day's last row.
const DAY_HEADER_INSET_PX: f32 = 6.0;

/// Where a day header's date starts, clear of its fold caret.
const DAY_HEADER_TEXT_X_PX: f32 = 20.0;
/// The report's floor while the month grid is expanded: the grid's own
/// six rows plus a weekday rule, on top of what the collapsed report had
/// to fit. Kept under the app's own 560 px minimum height so the report
/// can never be taller than the window it lives in.
const REPORT_MIN_HEIGHT_CALENDAR_PX: f32 = 540.0;

/// The size the report opens at. The trade list is eleven columns wide and
/// the curve wants room above it; opening cramped and making the trader
/// drag the corner every session is not a default.
const REPORT_DEFAULT_W_PX: f32 = 900.0;
/// See [`REPORT_DEFAULT_W_PX`].
const REPORT_DEFAULT_H_PX: f32 = 720.0;

/// The floor the equity curve shrinks to while the month grid is open. The
/// curve is a shape to glance at; the list under it is the answer to
/// "which trades", so when the two compete for a short window the curve is
/// what gives way.
const CURVE_MIN_H_CALENDAR_PX: f32 = 104.0;

/// One calendar day cell. Seven of them plus the report's own margins fit
/// inside `REPORT_MIN_WIDTH_PX`, so the grid never forces the window wider
/// than the trader sized it.
const CALENDAR_CELL_W_PX: f32 = 34.0;

/// See [`CALENDAR_CELL_W_PX`]. Tall enough for the day number and the
/// trade count under it.
const CALENDAR_CELL_H_PX: f32 = 28.0;

/// The tallest the report's trade list grows before it scrolls inside
/// itself. Bounded so a thousand-trade window still leaves the metric
/// grids under it reachable by scrolling the page rather than the list.
const REPORT_LIST_MAX_H_PX: f32 = 220.0;
/// Headline tile geometry: a caption, a 22 px value, a sub-line.
const TILE_HEIGHT_PX: f32 = 62.0;
/// Gap between headline tiles.
const TILE_GUTTER_PX: f32 = 8.0;
/// The report's hero numbers — the one type size above 16 in the app.
const HEADLINE_FONT_PX: f32 = 22.0;
/// The equity curve's readable floor.
const CURVE_MIN_H_PX: f32 = 160.0;
/// Stops the curve from eating a resized window.
const CURVE_MAX_H_PX: f32 = 240.0;
/// Alpha of the equity area fill — ground under the line, not a mark.
const CURVE_FILL_ALPHA: u8 = 31;
/// Alpha of the equity curve's gridlines — the chart's own grid token at
/// half strength, recessive under one quiet line.
const CURVE_GRID_LINE_ALPHA: u8 = 128;
/// At most this many curve points are drawn; the *drawing* downsamples
/// past it (and says so), the metrics always use every trade.
const CURVE_MAX_POINTS: usize = 1000;
/// Space kept under the curve for the metric grids before the curve
/// height clamps.
const CURVE_GRID_RESERVE_PX: f32 = 230.0;
/// The report window's smallest usable size — everything scrolls or
/// clamps below its defaults, so the window resizes freely down to this.
const REPORT_MIN_WIDTH_PX: f32 = 440.0;
/// See [`REPORT_MIN_WIDTH_PX`].
const REPORT_MIN_HEIGHT_PX: f32 = 360.0;
/// Height the report keeps for its honesty footer under the grids.
const REPORT_FOOTER_RESERVE_PX: f32 = 64.0;
/// Width of the typed-period field beside the pills — room for "999h".
const CUSTOM_PERIOD_FIELD_PX: f32 = 44.0;
/// The grids' readable floor inside a squeezed window.
const REPORT_GRID_MIN_H_PX: f32 = 80.0;
/// Width of the equity curve's y-tick gutter.
const CURVE_GUTTER_PX: f32 = 52.0;
/// The report as filtered for display: the period's trades in closing
/// order, their aggregation, and the anchor the period was measured from.
struct ReportView {
    period: ReportPeriod,
    source: SourceFilter,
    /// The calendar span in force. `Some` puts the report on absolute
    /// dates and takes the anchor-relative pills out of the cut; `None`
    /// leaves them in charge, which is exactly what the report did before
    /// a calendar existed.
    range: Option<DateRange>,
    /// The display timezone the view was cut with — "Today" moves with it,
    /// and so does which civil day a trade closed on.
    tz: TzOffset,
    /// Newest closing time in scope — what the period counts back from.
    anchor_ms: Option<i64>,
    /// Saved trades the window keeps out — the honest answer to "where did
    /// my old trades go": they exist, the filter just stops short of them.
    hidden_outside: usize,
    /// Saved trades the Source filter keeps out of this view.
    hidden_by_source: usize,
    /// The filtered trades, each still carrying the symbol folder and the
    /// session source it was journaled under — the report lists them, and
    /// a list that could not name its instrument would be the very gap
    /// this window exists to close.
    rows: Vec<HistoryRow>,
    /// The realized-equity walk over `rows`, cut with the view rather than
    /// re-walked on every frame.
    equity: EquityWalk,
    report: PerformanceReport,
}

/// The report as data: what is being asked, and what came back. Handed
/// out by [`ReportState::snapshot`] so an operator that cannot see the
/// window can still say which trades produced which numbers.
pub(crate) struct ReportSnapshot<'a> {
    /// `None` — every symbol folder in scope.
    pub(crate) symbol: Option<&'a str>,
    pub(crate) source: SourceFilter,
    /// What cut this report. One value, not a pair of options: the two
    /// filters are exclusive, and a pair could spell "both" or "neither" —
    /// states the report has no meaning for.
    pub(crate) window: ReportWindow,
    /// Saved trades the window keeps out, and those the Source filter does.
    pub(crate) hidden_outside: usize,
    pub(crate) hidden_by_source: usize,
    pub(crate) rows: &'a [HistoryRow],
    pub(crate) report: &'a PerformanceReport,
}

/// The report window's and the trades ledger's whole state.
///
/// Twenty-one fields that used to sit on the trading host, where they made
/// up better than a quarter of it. They are here because they answer one
/// question together — "what did the saved journal produce, cut this way?"
/// — and because none of them is read by an order, a bracket or a fill.
pub(crate) struct ReportState {
    /// An acknowledgement waiting to leave through [`ReportResponse`].
    toast: Option<String>,
    report_open: bool,
    /// Report symbol filter: `None` is every symbol, `Some` one folder.
    report_symbol: Option<String>,
    report_period: ReportPeriod,
    /// The report's session-source scope; opens on `Real`.
    report_source: SourceFilter,
    /// What the typed-period field holds; applied on Enter, kept verbatim
    /// so a refused entry stays visible for fixing.
    report_custom_text: String,
    /// Symbol folders on disk, for the report's combo box.
    report_symbols: Vec<String>,
    /// The report's history in scope, loaded fresh from disk.
    report: Option<LoadedHistory>,
    /// Bumped on every history reload, so the day index below knows its
    /// input changed without comparing months of trades.
    report_generation: u64,
    /// The report filtered to the period or the calendar range — rebuilt
    /// when a filter, the timezone or the loaded history changes.
    report_view: Option<ReportView>,
    /// The calendar: which month is on screen and which days are picked.
    calendar: CalendarState,
    /// Which civil days hold trades, for the calendar's highlighting.
    /// Built from the source-filtered history and rebuilt only when that
    /// history, the Source filter or the display timezone moves — walking
    /// months of trades per frame is exactly what a cache is for.
    report_days: DayIndex,
    /// The `(generation, source, timezone)` the day index was built for.
    report_days_key: Option<(u64, SourceFilter, TzOffset)>,
    /// Whether the report lists the trades behind its curve. On by
    /// default: a curve whose trades are hidden is the confusion this
    /// window was asked to end.
    report_list_open: bool,
    // Trades ledger.
    /// Which saved history the ledger lists — see [`LedgerScope`].
    ledger_scope: LedgerScope,
    /// Symbol folders on disk, for the ledger's own picker. Read with the
    /// history, never scanned on the frame.
    ledger_symbols: Vec<String>,
    /// Civil days the trader has folded shut in the ledger, by day number.
    /// A folded day keeps its header — the date, the count and the net —
    /// so collapsing summarises rather than hides.
    collapsed_days: std::collections::BTreeSet<i64>,
    /// The display timezone the ledger last drew with. The fold controls
    /// group by civil day and run outside the draw call, so they need the
    /// same clock the rows were stamped on or they would fold a day the
    /// list never showed.
    ledger_tz: TzOffset,
    /// Earlier sessions' journal rows, read on first draw and on demand —
    /// the live session file is excluded (its trades are already in the
    /// simulator).
    /// How many pages of saved history the ledger has revealed. Starts at
    /// one and grows by the "show older" control — a folder holding a year
    /// of sessions must not paint a year of rows to show today's.
    ledger_pages: usize,
    history_cache: Option<LoadedHistory>,
    /// Totals over the saved history above, summed with the load.
    saved_totals: LedgerTotals,
    /// Index into the session's closed trades selected in the ledger; the
    /// chart emphasizes that round trip.
    selected_trade: Option<usize>,
}

impl Default for ReportState {
    /// Hand-written rather than derived: three of these do not open at
    /// their type's default. The report lists its trades (a curve whose
    /// trades are hidden is the confusion the window exists to end), the
    /// ledger starts one page in, and its scope follows the chart.
    fn default() -> Self {
        Self {
            toast: None,
            report_open: false,
            report_symbol: None,
            report_period: ReportPeriod::All,
            report_source: SourceFilter::Real,
            report_custom_text: String::new(),
            report_symbols: Vec::new(),
            report: None,
            report_generation: 0,
            report_view: None,
            calendar: CalendarState::default(),
            report_days: DayIndex::default(),
            report_days_key: None,
            report_list_open: true,
            ledger_scope: LedgerScope::Chart,
            ledger_symbols: Vec::new(),
            collapsed_days: std::collections::BTreeSet::new(),
            ledger_tz: TzOffset::new(0),
            ledger_pages: 1,
            history_cache: None,
            saved_totals: LedgerTotals::default(),
            selected_trade: None,
        }
    }
}

impl ReportState {
    // ------------------------------------------------------------------
    // What the trading host still needs to say
    //
    // Five named events and three reads, rather than public fields. The
    // host owns the journal folder, the symbol and the venue, so it is the
    // only thing that can know when one of them moved; what that *means*
    // for a loaded history, a revealed page or a cut view is this module's
    // business, and each method below is one sentence of it.
    // ------------------------------------------------------------------

    /// Post an acknowledgement for the host to hand on. An outbox, not a
    /// toast: this module owns no lane and no clock, and the message
    /// leaves through [`ReportResponse::toast`].
    fn show_toast(&mut self, message: String) {
        self.toast = Some(message);
    }

    /// Whether the report window is on screen.
    ///
    /// Read on the per-trade path, which is the whole reason it exists: a
    /// close re-reads the journal only for a window somebody is looking
    /// at, and the caller needs to know that *before* it gathers a
    /// `ReportEnv` it would then throw away.
    pub(crate) fn is_open(&self) -> bool {
        self.report_open
    }

    /// Test-only. The saved journal rows the ledger has loaded, or `None`
    /// while it has never been drawn. `None` and "loaded, and empty" are
    /// different answers, and a caller that could not tell them apart would
    /// report a folder as empty because nobody had looked at it yet.
    ///
    /// This and the two below exist for the ten report tests that stayed
    /// with `paper_trading` because they drive a real journal on disk. They
    /// let those tests ask a question instead of reading a field, which is
    /// what keeps this struct's state private now that it has any.
    #[cfg(test)]
    pub(crate) fn saved_rows_loaded(&self) -> Option<&[HistoryRow]> {
        self.history_cache
            .as_ref()
            .map(|cache| cache.rows.as_slice())
    }

    /// Test-only. How many pages of saved history the ledger has revealed.
    #[cfg(test)]
    pub(crate) fn revealed_pages(&self) -> usize {
        self.ledger_pages
    }

    /// Test-only. The trades inside the report's current cut, or `None`
    /// before one has been made.
    #[cfg(test)]
    pub(crate) fn view_rows(&self) -> Option<&[HistoryRow]> {
        self.report_view.as_ref().map(|view| view.rows.as_slice())
    }

    /// The journal moved to a new folder. Everything loaded describes the
    /// old one, so it all goes — and an open window re-reads at once,
    /// because cleared caches alone left it claiming "no saved trades"
    /// until a manual refresh.
    pub(crate) fn trades_dir_changed(&mut self, env: &ReportEnv<'_>) {
        self.history_cache = None;
        self.ledger_pages = 1;
        self.report = None;
        self.report_view = None;
        if self.report_open {
            self.reload_report(env);
        }
    }

    /// The chart moved to another market. The saved history was read for
    /// the old one, and the selected round trip belonged to it.
    ///
    /// The revealed page is deliberately left alone — see the caller, which
    /// runs on every frame.
    pub(crate) fn symbol_changed(&mut self) {
        self.history_cache = None;
        self.selected_trade = None;
    }

    /// A close was journaled while the window is open: re-read now, or the
    /// report shows yesterday until a manual refresh — the "my trade is
    /// missing" report.
    ///
    /// Call only when [`Self::is_open`] says so. It is stated that way
    /// round rather than tested again here because this runs per closed
    /// trade, and the caller has to build a `ReportEnv` to reach it —
    /// work worth skipping entirely for a window nobody has open.
    pub(crate) fn journal_changed(&mut self, env: &ReportEnv<'_>) {
        debug_assert!(self.report_open, "the caller checks `is_open` first");
        self.reload_report(env);
    }

    /// An import copied files into the journal folder.
    pub(crate) fn history_imported(&mut self, env: &ReportEnv<'_>) {
        self.history_cache = None;
        self.ledger_pages = 1;
        if self.report_open {
            self.reload_report(env);
        }
    }

    /// Open the report window — the ticket's "Report…" button.
    pub(crate) fn open(&mut self, env: &ReportEnv<'_>) {
        self.open_report(env);
    }

    /// Index of the ledger's selected round trip, for the chart to
    /// emphasize.
    pub(crate) fn selected_trade(&self) -> Option<usize> {
        self.selected_trade
    }

    /// Drop the ledger's selection, reporting whether there was one —
    /// Escape's answer to "did I just undo something?".
    pub(crate) fn clear_selected_trade(&mut self) -> bool {
        self.selected_trade.take().is_some()
    }

    /// Every saved row in the ledger's scope, loading them first if the
    /// ledger has not been drawn yet. The export writes these beside the
    /// live session's own trades, and an export that silently skipped the
    /// saved half because nobody had opened the tab would be worse than a
    /// slow one.
    pub(crate) fn saved_rows(&mut self, env: &ReportEnv<'_>) -> &[HistoryRow] {
        if self.history_cache.is_none() {
            self.reload_ledger(env);
        }
        self.history_cache
            .as_ref()
            .map_or(&[][..], |cache| cache.rows.as_slice())
    }
}

#[cfg(test)]
mod tests;
