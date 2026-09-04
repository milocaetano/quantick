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

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
use quantick_sim::{ClosedTrade, PerformanceReport, SideReport, history};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

// One date law for every trade surface - see `paper_calendar`.
use crate::paper_calendar::{
    CalendarAction, CalendarState, CivilDate, DAY_MS, DateRange, DayIndex, DaySelection,
    fmt_offset_minute, today,
};
use crate::paper_chrome::{
    PositionSummary, caption, fmt_decimal, fmt_duration_ms, fmt_points, fmt_signed_points,
    list_symbol_folders, pill_toggle, points_color, position_word, sanitize_symbol,
};
use crate::theme;
use crate::timezone::TzOffset;

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
/// Which saved history the ledger lists. Three cases, not two: following
/// the chart is what the panel opens on, but a trader reviewing yesterday
/// wants to name an instrument without retuning the chart to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LedgerScope {
    /// Whatever the chart is showing — the panel's default, and the only
    /// one that moves when the chart does.
    Chart,
    /// One named instrument, whatever the chart shows.
    Symbol(String),
    /// Every instrument in the folder, mixed into one timeline.
    All,
}

impl LedgerScope {
    /// The symbol folder to read, or `None` for the whole folder.
    fn folder<'a>(&'a self, chart: &'a str) -> Option<&'a str> {
        match self {
            Self::Chart => Some(chart),
            Self::Symbol(symbol) => Some(symbol.as_str()),
            Self::All => None,
        }
    }

    /// What the picker shows for this scope.
    fn label(&self, chart: &str) -> String {
        match self {
            Self::Chart if chart.is_empty() => "This chart".to_owned(),
            Self::Chart => format!("This chart · {chart}"),
            Self::Symbol(symbol) => symbol.clone(),
            Self::All => "All symbols".to_owned(),
        }
    }
}

/// The report's period filter, measured back from the newest saved trade
/// in scope — never from a wall clock. The engine has no clock, and a
/// replayed session's trades may be years old; a wall-clock "7 days" would
/// report a perfectly good replay as empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportPeriod {
    Today,
    Week,
    Month,
    Quarter,
    All,
    /// A typed span (`2d`, `12h`…) in milliseconds back from the anchor —
    /// the text box beside the pills.
    Custom(i64),
}

impl ReportPeriod {
    /// Every fixed period, in pill order; `Custom` rides the text box.
    const PILLS: [Self; 5] = [
        Self::Today,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::All,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Quarter => "90d",
            Self::All => "All",
            Self::Custom(_) => "custom",
        }
    }

    /// The anchor-relative phrase the support line reads.
    fn phrase(self) -> String {
        match self {
            Self::Today => "the newest trade's day".to_owned(),
            Self::Week => "the last 7 days".to_owned(),
            Self::Month => "the last 30 days".to_owned(),
            Self::Quarter => "the last 90 days".to_owned(),
            Self::All => "everything saved".to_owned(),
            Self::Custom(period_ms) => format!("the last {}", fmt_period_ms(period_ms)),
        }
    }

    /// Oldest closing time still inside the period, measured back from
    /// `anchor_ms`; `None` keeps everything. "Today" is the anchor's civil
    /// day in the chart's display timezone — the ledger renders local
    /// times, so the day must break where the user sees midnight, not
    /// where UTC does.
    fn cutoff_ms(self, anchor_ms: i64, tz: TzOffset) -> Option<i64> {
        match self {
            // The same civil day the calendar highlights and the ledger
            // stamps: one date law, so a pill and a picked cell can never
            // disagree about where midnight is.
            Self::Today => Some(CivilDate::from_ms(anchor_ms, tz).start_ms(tz)),
            Self::Week => Some(anchor_ms.saturating_sub(7 * DAY_MS)),
            Self::Month => Some(anchor_ms.saturating_sub(30 * DAY_MS)),
            Self::Quarter => Some(anchor_ms.saturating_sub(90 * DAY_MS)),
            Self::All => None,
            Self::Custom(period_ms) => Some(anchor_ms.saturating_sub(period_ms)),
        }
    }
}

/// Parse a typed period: a positive whole number and a unit — `m`
/// (minutes), `h`, `d`, `w` — any case, blanks tolerated. `None` is a
/// refusal the caller must say out loud: a silently-empty report is
/// exactly the confusion the typed field exists to end.
fn parse_period(text: &str) -> Option<i64> {
    let text = text.trim();
    let unit = text.chars().last()?;
    let count = text.get(..text.len() - unit.len_utf8())?.trim_end();
    let count: i64 = count.parse().ok()?;
    if count <= 0 {
        return None;
    }
    let unit_ms: i64 = match unit.to_ascii_lowercase() {
        'm' => 60_000,
        'h' => 3_600_000,
        'd' => 86_400_000,
        'w' => 7 * 86_400_000,
        _ => return None,
    };
    count.checked_mul(unit_ms)
}

/// `45m`, `36h`, `2d`, `1w` — the canonical spelling of a custom period,
/// largest whole unit first.
fn fmt_period_ms(period_ms: i64) -> String {
    const MINUTE_MS: i64 = 60_000;
    const HOUR_MS: i64 = 3_600_000;
    // `DAY_MS` is the calendar's, not a local copy: the pills and the
    // month grid measure a day the same way or they are two features.
    const WEEK_MS: i64 = 7 * DAY_MS;
    let (value, unit) = if period_ms % WEEK_MS == 0 {
        (period_ms / WEEK_MS, 'w')
    } else if period_ms % DAY_MS == 0 {
        (period_ms / DAY_MS, 'd')
    } else if period_ms % HOUR_MS == 0 {
        (period_ms / HOUR_MS, 'h')
    } else {
        (period_ms / MINUTE_MS, 'm')
    };
    format!("{value}{unit}")
}

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

/// Which filter is cutting the report. Exclusive by construction: a
/// picked range takes over from the pills rather than intersecting with
/// them, so exactly one of these is ever in force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReportWindow {
    /// Days the trader picked on the calendar.
    Dates(DateRange),
    /// The anchor-relative period pills.
    Period(ReportPeriod),
}

impl ReportWindow {
    /// The window in one phrase — absolute dates, or the pill's wording.
    pub(crate) fn label(self) -> String {
        match self {
            Self::Dates(range) => range.label(),
            Self::Period(period) => period.phrase(),
        }
    }
}

/// The realized-equity walk `E_0..E_n` (`E_0 = 0` before the first trade),
/// in the two shapes its two readers need: exact points for the trade
/// list's running total, and plot-ready `f32` with its bounds for the
/// curve. Computed once when the view is cut — a window holding a year of
/// trades must not walk them again sixty times a second.
struct EquityWalk {
    /// `n + 1` exact running totals in points.
    points: Vec<Decimal>,
    /// The same walk as the curve plots it.
    plot: Vec<f32>,
    low: f32,
    high: f32,
}

impl EquityWalk {
    fn of(rows: &[HistoryRow]) -> Self {
        let mut points = Vec::with_capacity(rows.len() + 1);
        let mut plot = Vec::with_capacity(rows.len() + 1);
        points.push(Decimal::ZERO);
        plot.push(0.0_f32);
        let (mut low, mut high) = (0.0_f32, 0.0_f32);
        let mut sum = Decimal::ZERO;
        for row in rows {
            sum = sum.saturating_add(row.trade.pnl_points);
            points.push(sum);
            let value = sum.to_f64().unwrap_or_default() as f32;
            low = low.min(value);
            high = high.max(value);
            plot.push(value);
        }
        Self {
            points,
            plot,
            low,
            high,
        }
    }
}

/// One journal row loaded from disk: the trade, the symbol folder it came
/// from, and the session source its file recorded.
#[derive(Clone)]
pub(crate) struct HistoryRow {
    pub(crate) symbol: String,
    /// `None` — a file from before the source was recorded. The report's
    /// Real view includes it: that era *was* live trading, and hiding it
    /// would "lose" the user's history all over again.
    pub(crate) source: Option<history::SessionSource>,
    pub(crate) trade: ClosedTrade,
}

/// Journal rows loaded from disk, each remembering the symbol folder it
/// came from, merged into one closing-order timeline.
pub(crate) struct LoadedHistory {
    /// Rows in closing order across every file read.
    pub(crate) rows: Vec<HistoryRow>,
    pub(crate) files: usize,
    /// Files that were not readable quantick-trades files.
    pub(crate) unreadable_files: usize,
    /// Rows the parser had to report as unreadable (torn tails and such).
    pub(crate) problem_rows: usize,
}

/// The report's session-source filter. Default `Real`: practice runs must
/// never inflate the real track record unasked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceFilter {
    /// Live sessions, plus files from before the source was recorded.
    Real,
    /// Replay-driven practice sessions only.
    Replay,
    /// Everything, mixed.
    All,
}

impl SourceFilter {
    /// Every filter, in pill order.
    const PILLS: [Self; 3] = [Self::Real, Self::Replay, Self::All];

    fn label(self) -> &'static str {
        match self {
            Self::Real => "Real",
            Self::Replay => "Replay",
            // Not "All": the period pills own that word on the same row,
            // and two identical pills a hand-width apart invite the wrong
            // click.
            Self::All => "Both",
        }
    }

    fn hover(self) -> &'static str {
        match self {
            Self::Real => {
                "live sessions - files saved before quantick recorded a source count as real"
            }
            Self::Replay => "practice sessions driven by a market-replay recording",
            Self::All => "live and replay together - mixed on purpose",
        }
    }

    /// Whether a row with this recorded source belongs to the filter.
    fn admits(self, source: Option<history::SessionSource>) -> bool {
        match self {
            // Exhaustive on purpose: a future source variant must not fall
            // into the real track record by default — adding one forces
            // this match to say where it belongs.
            Self::Real => match source {
                None | Some(history::SessionSource::Live) => true,
                Some(history::SessionSource::Replay) => false,
            },
            Self::Replay => source == Some(history::SessionSource::Replay),
            Self::All => true,
        }
    }
}

/// What the Trades ledger asked of its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerAction {
    /// Center the chart on this round trip (`opened_ms`, `closed_ms`).
    Navigate(i64, i64),
    /// Switch the dock to the Trading tab — the empty state's call to
    /// action.
    OpenTicket,
}

/// One virtualised ledger line.
enum LedgerRow<'a> {
    /// A group caption and its count.
    Header(&'static str, usize),
    /// A civil day's caption: the date, how many of the rows under it
    /// closed on that day, what they netted, and whether it is folded
    /// shut. A ledger of bare clock times cannot answer "which session was
    /// that" — the day header is where the answer lives, it carries the
    /// day's result for free, and clicking it folds the day away.
    Day(CivilDate, usize, Decimal, bool),
    /// A closed trade from this session's simulator: selectable, and its
    /// round trip is on the current tape.
    Session(usize, &'a ClosedTrade),
    /// A row loaded from an earlier session's journal — display only; its
    /// tape is not the one on screen.
    Earlier(&'a str, &'a ClosedTrade),
    /// The control that reveals the next page of saved history, carrying
    /// how many trades are still held back.
    More(usize),
}

/// The ledger's totals over every saved trade in scope. Summed when the
/// folder is read, never on the frame: walking a year of sessions sixty
/// times a second to print one line is work nobody asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct LedgerTotals {
    trades: usize,
    wins: usize,
    net: Decimal,
}

impl LedgerTotals {
    fn of<'a>(trades: impl Iterator<Item = &'a ClosedTrade>) -> Self {
        let mut totals = Self::default();
        for trade in trades {
            totals.trades += 1;
            if trade.pnl_points > Decimal::ZERO {
                totals.wins += 1;
            }
            totals.net = totals.net.saturating_add(trade.pnl_points);
        }
        totals
    }

    /// The two sets the strip adds up: what is saved on disk and what this
    /// session has closed since.
    fn plus(self, other: Self) -> Self {
        Self {
            trades: self.trades + other.trades,
            wins: self.wins + other.wins,
            net: self.net.saturating_add(other.net),
        }
    }

    /// Whole-percent win rate; `None` when there is nothing to divide by.
    fn win_rate(self) -> Option<usize> {
        (self.wins * 100).checked_div(self.trades)
    }
}

/// How much saved history the ledger is showing and how much it is
/// holding back. Pure so the count printed on the "show older" control and
/// the rows above it can never disagree — a button promising trades that
/// are not there is worse than no button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LedgerPage {
    /// Saved trades rendered, newest first.
    shown: usize,
    /// Saved trades loaded but not yet revealed.
    remaining: usize,
}

impl LedgerPage {
    /// `pages` pages of [`LEDGER_PAGE_TRADES`] each, clamped to `total`.
    /// Page zero is treated as one: the ledger always shows something.
    fn of(total: usize, pages: usize) -> Self {
        let shown = LEDGER_PAGE_TRADES.saturating_mul(pages.max(1)).min(total);
        Self {
            shown,
            remaining: total.saturating_sub(shown),
        }
    }
}

/// What one painted ledger row reported back.
#[derive(Default)]
struct LedgerRowResponse {
    clicked: bool,
    navigate: bool,
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

impl ReportState {
    // ------------------------------------------------------------------
    // Trades ledger tab
    // ------------------------------------------------------------------

    /// Load the earlier sessions' rows for the ledger, scoped to the
    /// current symbol or the whole folder. The live session's own file is
    /// excluded — its trades are already in the simulator.
    pub(crate) fn reload_ledger(&mut self, env: &ReportEnv<'_>) {
        let symbol = self.ledger_scope.folder(env.symbol);
        let history = load_history(env.dir, symbol, env.session_journal_paths);
        // The strip under the list sums every saved trade, not the revealed
        // page, so it is summed once here rather than on every frame.
        self.saved_totals = LedgerTotals::of(history.rows.iter().map(|row| &row.trade));
        self.history_cache = Some(history);
        self.ledger_symbols = list_symbol_folders(env.dir);
    }

    /// Fold or unfold one civil day in the ledger. A named action taking
    /// data, like every other capability here: the header click calls it,
    /// and so does anything else that ever wants to.
    pub(crate) fn set_day_collapsed(&mut self, day: CivilDate, collapsed: bool) {
        if collapsed {
            self.collapsed_days.insert(day.day_number());
        } else {
            self.collapsed_days.remove(&day.day_number());
        }
    }

    /// Whether every day currently in the ledger is folded shut. Read from
    /// the loaded history, so the control can name what it will do.
    fn all_days_collapsed(&self, env: &ReportEnv<'_>) -> bool {
        let mut any = false;
        for day in self.ledger_days(env) {
            any = true;
            if !self.collapsed_days.contains(&day) {
                return false;
            }
        }
        any
    }

    /// Every civil day the ledger currently lists, saved and live alike.
    fn ledger_days(&self, env: &ReportEnv<'_>) -> std::collections::BTreeSet<i64> {
        let tz = self.ledger_tz;
        let saved = self
            .history_cache
            .iter()
            .flat_map(|cache| cache.rows.iter().map(|row| &row.trade));
        saved
            .chain(env.session_trades.iter())
            .map(|trade| CivilDate::from_ms(trade.closed_ms, tz).day_number())
            .collect()
    }

    /// Fold every day shut, or open every one back up.
    pub(crate) fn toggle_all_days(&mut self, expand: bool, env: &ReportEnv<'_>) {
        if expand {
            self.collapsed_days.clear();
        } else {
            self.collapsed_days = self.ledger_days(env);
        }
    }

    /// Re-read the folder for a *changed scope* — the refresh button and
    /// the symbol/scope switches. Unlike [`Self::reload_ledger`] this also
    /// drops back to the first page: a deep page count cannot survive a
    /// list it was never counted against.
    pub(crate) fn rescope_ledger(&mut self, env: &ReportEnv<'_>) {
        self.ledger_pages = 1;
        self.reload_ledger(env);
    }

    /// The Trades dock tab: the ledger of closed simulated trades — the
    /// open position pinned on top, this session under it, the saved
    /// history under that, and a totals strip that never scrolls away.
    pub(crate) fn draw_trades_tab(
        &mut self,
        ui: &mut egui::Ui,
        tz: TzOffset,
        env: &ReportEnv<'_>,
    ) -> Option<LedgerAction> {
        if self.history_cache.is_none() {
            self.reload_ledger(env);
        }
        self.ledger_tz = tz;
        let mut action = None;

        // Scope row: which instrument's saved history the ledger lists.
        let mut reload = false;
        let mut picked: Option<LedgerScope> = None;
        ui.horizontal(|ui| {
            let chart = env.symbol.to_owned();
            egui::ComboBox::from_id_salt("paper_ledger_scope")
                .width(LEDGER_SCOPE_COMBO_PX)
                .selected_text(
                    egui::RichText::new(self.ledger_scope.label(&chart))
                        .monospace()
                        .size(11.0),
                )
                .show_ui(ui, |ui| {
                    let mut option = |ui: &mut egui::Ui, scope: LedgerScope| {
                        let on = self.ledger_scope == scope;
                        if ui.selectable_label(on, scope.label(&chart)).clicked() && !on {
                            picked = Some(scope);
                        }
                    };
                    option(ui, LedgerScope::Chart);
                    option(ui, LedgerScope::All);
                    if !self.ledger_symbols.is_empty() {
                        ui.separator();
                    }
                    for symbol in self.ledger_symbols.clone() {
                        option(ui, LedgerScope::Symbol(symbol));
                    }
                })
                .response
                .on_hover_text(
                    "which instrument's saved history this list shows - the chart's, one you \
                     name, or all of them mixed into one timeline",
                );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icons::ARROWS_CLOCKWISE)
                    .on_hover_text("re-read the history folder")
                    .clicked()
                {
                    reload = true;
                }
                // Folding every earlier day at once: the list becomes one
                // line per day, which is how a week is read rather than
                // scrolled.
                let folded = self.all_days_collapsed(env);
                let (icon, hover) = if folded {
                    (icons::ARROWS_OUT_LINE_VERTICAL, "open every day back up")
                } else {
                    (
                        icons::ARROWS_IN_LINE_VERTICAL,
                        "fold every day shut - each keeps its date, count and net",
                    )
                };
                if ui.small_button(icon).on_hover_text(hover).clicked() {
                    self.toggle_all_days(folded, env);
                }
            });
        });
        if let Some(scope) = picked {
            self.ledger_scope = scope;
            reload = true;
        }
        if reload {
            self.rescope_ledger(env);
        }

        ui.horizontal(|ui| {
            ui.label(caption("TRADE"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(caption("PTS"));
            });
        });
        ui.separator();

        // The open position rides above the scroll — panning through
        // history must never hide the trade you are in.
        if let Some(open) = &env.open {
            ui.label(caption("OPEN"));
            let symbol = (!env.symbol.is_empty()).then_some(env.symbol);
            draw_open_row(ui, &open.summary, symbol, open.mark_price, open.held_ms);
        }

        let session: Vec<(usize, &ClosedTrade)> =
            env.session_trades.iter().enumerate().rev().collect();
        let saved_len = self
            .history_cache
            .as_ref()
            .map_or(0, |cache| cache.rows.len());
        // Only the revealed pages are turned into rows; the rest stay
        // loaded and counted, which is what the control below them says.
        // The `take` is the point — a year of sessions must cost the same
        // per frame as a week of them, so the untouched tail is never even
        // walked into a Vec.
        let page = LedgerPage::of(saved_len, self.ledger_pages);
        let earlier: Vec<(&str, &ClosedTrade)> = self
            .history_cache
            .as_ref()
            .map(|cache| {
                cache
                    .rows
                    .iter()
                    .rev()
                    .take(page.shown)
                    .map(|row| (row.symbol.as_str(), &row.trade))
                    .collect()
            })
            .unwrap_or_default();
        let earlier = earlier.as_slice();

        if session.is_empty() && saved_len == 0 {
            if env.open.is_none() {
                ui.add_space(12.0);
                let headline = match self.ledger_scope.folder(env.symbol) {
                    Some(symbol) if !symbol.is_empty() => {
                        format!("No trades for {symbol}.")
                    }
                    _ => "No simulated trades yet.".to_owned(),
                };
                ui.label(egui::RichText::new(headline).color(theme::TEXT_PRIMARY));
                ui.label(
                    egui::RichText::new(
                        "Close a position and it lands here - this session and every saved one.",
                    )
                    .color(theme::TEXT_SUPPORT)
                    .small(),
                );
                ui.add_space(4.0);
                if ui
                    .button("Open the ticket")
                    .on_hover_text("switch to the Trading tab and place an order")
                    .clicked()
                {
                    action = Some(LedgerAction::OpenTicket);
                }
            }
            self.draw_ledger_disclosure(ui);
            return action;
        }

        // Rows are cut newest first, so day headers open each day as the
        // list walks back in time.
        let mut rows = Vec::new();
        if !session.is_empty() {
            rows.push(LedgerRow::Header("THIS SESSION", session.len()));
            push_by_day(
                &mut rows,
                &session,
                tz,
                &self.collapsed_days,
                |item| item.1,
                |item| LedgerRow::Session(item.0, item.1),
            );
        }
        if !earlier.is_empty() {
            rows.push(LedgerRow::Header("EARLIER SESSIONS", saved_len));
            push_by_day(
                &mut rows,
                earlier,
                tz,
                &self.collapsed_days,
                |item| item.1,
                |item| LedgerRow::Earlier(item.0, item.1),
            );
        }
        if page.remaining > 0 {
            rows.push(LedgerRow::More(page.remaining));
        }

        // Totals over everything *in scope*, not everything listed: the
        // rows above are one revealed page and the strip must not swing
        // every time the trader reveals another. The saved half was summed
        // when the folder was read; only this session's own trades — a
        // handful — are counted here.
        let totals = self
            .saved_totals
            .plus(LedgerTotals::of(env.session_trades.iter()));

        let rows_listed = session.len() + earlier.len();
        let list_height = (ui.available_height() - TOTALS_STRIP_PX).max(LEDGER_ROW_HEIGHT_PX);
        let selected = self.selected_trade;
        // Session rows carry the chart's own instrument; a ledger row that
        // does not name its market is unreadable the moment a second tab
        // exists.
        let own_symbol = (!env.symbol.is_empty()).then_some(env.symbol);
        let mut reveal_more = false;
        let mut fold: Option<(CivilDate, bool)> = None;
        let mut clicked: Option<Option<usize>> = None;
        let mut navigate = None;
        egui::ScrollArea::vertical()
            .id_salt("paper_trades_ledger")
            .auto_shrink([false, false])
            .max_height(list_height)
            .show_rows(ui, LEDGER_ROW_HEIGHT_PX, rows.len(), |ui, range| {
                for index in range {
                    match &rows[index] {
                        LedgerRow::Header(label, count) => draw_group_header(ui, label, *count),
                        LedgerRow::Day(date, count, net, folded) => {
                            if draw_day_header(ui, *date, *count, *net, *folded) {
                                fold = Some((*date, !*folded));
                            }
                        }
                        LedgerRow::Session(trade_index, trade) => {
                            let is_selected = selected == Some(*trade_index);
                            let response =
                                draw_ledger_row(ui, trade, own_symbol, is_selected, true, tz);
                            if response.navigate {
                                navigate =
                                    Some(LedgerAction::Navigate(trade.opened_ms, trade.closed_ms));
                            } else if response.clicked {
                                clicked = Some((!is_selected).then_some(*trade_index));
                            }
                        }
                        LedgerRow::Earlier(symbol, trade) => {
                            draw_ledger_row(ui, trade, Some(symbol), false, false, tz);
                        }
                        LedgerRow::More(remaining) => {
                            reveal_more |= draw_more_row(ui, *remaining);
                        }
                    }
                }
            });
        if let Some(selection) = clicked {
            self.selected_trade = selection;
        }
        if navigate.is_some() {
            action = navigate;
        }
        if reveal_more {
            self.ledger_pages = self.ledger_pages.saturating_add(1);
        }
        if let Some((day, collapsed)) = fold {
            self.set_day_collapsed(day, collapsed);
        }

        ui.separator();
        ui.horizontal(|ui| {
            let win_rate = totals
                .win_rate()
                .map_or_else(String::new, |rate| format!(" · {rate}% win"));
            let scope = if page.remaining > 0 {
                // The strip counts more than the list shows, so it says so
                // rather than letting the two look like a contradiction.
                format!(" · {} listed", rows_listed)
            } else {
                String::new()
            };
            ui.label(
                egui::RichText::new(format!("{} trades{win_rate}{scope}", totals.trades))
                    .monospace()
                    .color(theme::TEXT_MUTED),
            )
            .on_hover_text(
                "every trade in scope - this session plus the saved history, whether or not \
                 the list has revealed it yet",
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} pts", fmt_signed_points(totals.net)))
                        .monospace()
                        .strong()
                        .color(points_color(totals.net)),
                );
            });
        });
        self.draw_ledger_disclosure(ui);
        action
    }

    /// The honesty line under the ledger: unreadable files and skipped rows
    /// are counted, never silently dropped.
    fn draw_ledger_disclosure(&self, ui: &mut egui::Ui) {
        let Some(cache) = &self.history_cache else {
            return;
        };
        if cache.unreadable_files == 0 && cache.problem_rows == 0 {
            return;
        }
        ui.label(
            egui::RichText::new(format!(
                "{} file(s) unreadable, {} row(s) skipped - counted, never silently dropped.",
                cache.unreadable_files, cache.problem_rows,
            ))
            .color(theme::WARN)
            .small(),
        );
    }

    // ------------------------------------------------------------------
    // Report window
    // ------------------------------------------------------------------

    /// The `QUANTICK_PAPER_REPORT_AUTOSTART` hook: the report, scoped to
    /// every symbol — an autostart runs before the first feed settles, so
    /// "the current symbol" would be the wrong one anyway.
    pub(crate) fn autostart_report(&mut self, env: &ReportEnv<'_>) {
        self.report_symbol = None;
        self.open_report(env);
    }

    /// Filter the report by dates. The calendar's click and the harness
    /// hook both come through here — one named action taking data, so an
    /// operator that is not holding the mouse reaches exactly the state a
    /// click reaches rather than a parallel one that drifts from it.
    pub(crate) fn pick_report_dates(&mut self, selection: DaySelection) {
        self.calendar.selection = selection;
        self.report_view = None;
    }

    /// Page the month grid to the month holding `date`. Separate from the
    /// pick on purpose: clicking a cell must not yank the grid to another
    /// month, while a hook naming a date must land where that date is.
    pub(crate) fn show_report_month(&mut self, date: CivilDate) {
        self.calendar.month = Some(date.month_start());
    }

    /// What the report is currently showing, as data rather than pixels:
    /// the window in force and the trades inside it. The second operator
    /// reads this instead of the screen — a filter whose result exists
    /// only inside a paint call cannot be reported back to anyone.
    pub(crate) fn snapshot(&self) -> Option<ReportSnapshot<'_>> {
        let view = self.report_view.as_ref()?;
        Some(ReportSnapshot {
            symbol: self.report_symbol.as_deref(),
            source: view.source,
            window: view
                .range
                .map_or(ReportWindow::Period(view.period), ReportWindow::Dates),
            hidden_outside: view.hidden_outside,
            hidden_by_source: view.hidden_by_source,
            rows: &view.rows,
            report: &view.report,
        })
    }

    /// The `QUANTICK_PAPER_CALENDAR` hook: the report open with the month
    /// grid expanded and `selection` picked — the report's own path, so a
    /// scripted run reaches exactly the state a click would.
    pub(crate) fn autostart_calendar(&mut self, selection: DaySelection, env: &ReportEnv<'_>) {
        self.autostart_report(env);
        self.calendar.open = true;
        self.pick_report_dates(selection);
        // A hook naming a date must land on it, not a month away.
        if let Some(range) = selection.range() {
            self.show_report_month(range.start);
        }
    }

    /// The `QUANTICK_LEDGER_SCOPE` hook: the ledger listing that
    /// instrument's saved history — the picker's own path, so a scripted
    /// run lands where a click would.
    pub(crate) fn set_ledger_scope(&mut self, scope: LedgerScope) {
        self.ledger_scope = scope;
        self.history_cache = None;
    }

    /// The `QUANTICK_LEDGER_FOLD` hook: every day in the ledger folded
    /// shut, the one-line-per-day read. Folding is otherwise a click on
    /// each header, which a capture cannot perform.
    pub(crate) fn autostart_folded_days(&mut self, tz: TzOffset, env: &ReportEnv<'_>) {
        if self.history_cache.is_none() {
            self.reload_ledger(env);
        }
        self.ledger_tz = tz;
        self.toggle_all_days(false, env);
    }

    /// The `QUANTICK_LEDGER_PAGES` hook: the ledger already scrolled past
    /// its first page of saved history, which no screenshot could reach
    /// otherwise — the control that gets there is a click.
    pub(crate) fn autostart_ledger_pages(&mut self, pages: usize) {
        self.ledger_pages = pages.max(1);
    }

    /// The `QUANTICK_PAPER_REPORT_LIST` hook: whether the report lists the
    /// trades behind its curve. Open by default, so the hook exists to
    /// reach the collapsed state.
    pub(crate) fn set_report_list_open(&mut self, open: bool) {
        self.report_list_open = open;
    }

    /// Open the report window — the `Report…` button's path.
    fn open_report(&mut self, env: &ReportEnv<'_>) {
        self.report_open = true;
        if self.report.is_none() && !env.symbol.is_empty() {
            // First open lands on the chart's own symbol; later opens keep
            // whatever the user last chose.
            self.report_symbol = Some(env.symbol.to_owned());
        }
        self.reload_report(env);
    }

    pub(crate) fn reload_report(&mut self, env: &ReportEnv<'_>) {
        self.report = Some(load_history(env.dir, self.report_symbol.as_deref(), &[]));
        self.report_generation = self.report_generation.wrapping_add(1);
        self.report_view = None;
        self.report_symbols = list_symbol_folders(env.dir);
    }

    /// Rebuild the filtered view when a filter, the timezone or the loaded
    /// history changed. The anchor is the newest trade in scope — after
    /// the Source filter, never a clock.
    pub(crate) fn ensure_report_view(&mut self, tz: TzOffset) {
        let range = self.calendar.selection.range();
        let fresh = self.report_view.as_ref().is_some_and(|view| {
            view.period == self.report_period
                && view.source == self.report_source
                && view.range == range
                && view.tz == tz
        });
        if fresh {
            return;
        }
        let Some(history) = &self.report else {
            self.report_view = None;
            self.report_days = DayIndex::default();
            self.report_days_key = None;
            return;
        };
        let in_scope: Vec<&HistoryRow> = history
            .rows
            .iter()
            .filter(|row| self.report_source.admits(row.source))
            .collect();
        let hidden_by_source = history.rows.len().saturating_sub(in_scope.len());
        let anchor_ms = in_scope.last().map(|row| row.trade.closed_ms);

        // Which days hold trades depends on the loaded history, the Source
        // filter and the timezone — not on the picked range. Rebuilding it
        // on every day click would walk months of trades for a highlight
        // that did not change.
        let days_key = (self.report_generation, self.report_source, tz);
        if self.report_days_key != Some(days_key) {
            self.report_days = DayIndex::build(in_scope.iter().map(|row| &row.trade), tz);
            self.report_days_key = Some(days_key);
            // The days under the grid just changed — a new symbol, a new
            // Source, another timezone. Follow them: leaving the grid
            // parked on the month the *previous* scope ended in shows an
            // empty August for a market that last traded in February.
            self.calendar.month = self.report_days.last().map(CivilDate::month_start);
        }

        // A picked range is an explicit answer to "which days"; it takes
        // over from the pills rather than intersecting with them, so a
        // chosen date can never come back empty because a pill the user
        // had forgotten about was cutting too.
        let cutoff = match range {
            Some(_) => None,
            None => anchor_ms.and_then(|anchor| self.report_period.cutoff_ms(anchor, tz)),
        };
        let rows: Vec<HistoryRow> = in_scope
            .iter()
            .filter(|row| {
                range.is_none_or(|range| range.contains_ms(row.trade.closed_ms, tz))
                    && cutoff.is_none_or(|cutoff| row.trade.closed_ms >= cutoff)
            })
            .map(|row| (*row).clone())
            .collect();
        let hidden_outside = in_scope.len().saturating_sub(rows.len());
        let trades: Vec<ClosedTrade> = rows.iter().map(|row| row.trade.clone()).collect();
        let report = PerformanceReport::from_trades(&trades);
        let equity = EquityWalk::of(&rows);
        self.report_view = Some(ReportView {
            period: self.report_period,
            source: self.report_source,
            range,
            tz,
            anchor_ms,
            hidden_outside,
            hidden_by_source,
            rows,
            equity,
            report,
        });
        // The cut states itself as data. A trader reads the window; an
        // operator that cannot see it reads this line — and it is the same
        // snapshot either of them would be handed.
        if let Some(snapshot) = self.snapshot() {
            tracing::info!(
                target: "quantick::app",
                schema_version = 1_u8,
                event_code = "PAPER_REPORT_CUT",
                symbol = snapshot.symbol.unwrap_or("*"),
                source = snapshot.source.label(),
                window = %snapshot.window.label(),
                trades = snapshot.rows.len(),
                hidden_outside = snapshot.hidden_outside,
                hidden_by_source = snapshot.hidden_by_source,
                net_points = %snapshot.report.net_points,
                "the simulated performance report was re-cut"
            );
        }
    }

    /// The performance report, computed from what is actually on disk.
    /// Non-modal by the app's contract — dimming the chart while a
    /// simulated position is open would be dangerous.
    pub(crate) fn draw_window(
        &mut self,
        ctx: &egui::Context,
        tz: TzOffset,
        env: &ReportEnv<'_>,
    ) -> ReportResponse {
        let mut asked = ReportResponse {
            // Anything the filter row posted before this frame leaves now,
            // whether or not the window is still open: a refusal the trader
            // earned must not be swallowed by closing the thing that raised
            // it.
            toast: self.toast.take(),
            ..ReportResponse::default()
        };
        if !self.report_open {
            return asked;
        }
        self.ensure_report_view(tz);
        let mut open = true;
        let mut reload = false;
        let mut toggle_list = false;
        egui::Window::new("Simulated performance")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            // Wide enough for every column of the trade list and tall
            // enough for the tiles, the curve and a readable page of it.
            // Both are still only a starting size — the window resizes.
            .default_size(egui::vec2(REPORT_DEFAULT_W_PX, REPORT_DEFAULT_H_PX))
            .min_width(REPORT_MIN_WIDTH_PX)
            // An expanded month grid needs its own room. Without this the
            // report at its smallest pushed the tiles, the curve and the
            // disclosure lines out through the bottom of the window.
            .min_height(if self.calendar.open {
                REPORT_MIN_HEIGHT_CALENDAR_PX
            } else {
                REPORT_MIN_HEIGHT_PX
            })
            .show(ctx, |ui| {
                reload = self.draw_report_filters(ui, &mut asked);
                self.draw_report_calendar(ui, tz);
                // The rows above can retire the view — a picked day, a
                // pill, a cleared range. Re-cutting it here rather than
                // next frame is what keeps a click from flashing the empty
                // state and collapsing the window for one frame.
                self.ensure_report_view(tz);
                ui.separator();
                let list_open = self.report_list_open;
                let calendar_open = self.calendar.open;
                match &self.report_view {
                    Some(view) if !view.rows.is_empty() => {
                        draw_report_tiles(ui, &view.report);
                        ui.add_space(8.0);
                        draw_equity_curve(
                            ui,
                            view,
                            if calendar_open {
                                CURVE_MIN_H_CALENDAR_PX
                            } else {
                                CURVE_MIN_H_PX
                            },
                        );
                        ui.add_space(4.0);
                        // The grids fill whatever height the user gave the
                        // window and scroll inside it. Letting them take
                        // their content height instead (auto-shrink) made
                        // the window itself grow to fit and refuse to
                        // resize down — the "stuck huge" report.
                        egui::ScrollArea::vertical()
                            .id_salt("paper_report_grids")
                            .auto_shrink([false, false])
                            .max_height(
                                (ui.available_height() - REPORT_FOOTER_RESERVE_PX)
                                    .max(REPORT_GRID_MIN_H_PX),
                            )
                            .show(ui, |ui| {
                                // The trades come first: the curve above is
                                // a shape, and this is the story behind it.
                                toggle_list = draw_trade_list(ui, view, list_open);
                                draw_report_grid(ui, &view.report);
                                draw_side_grid(ui, &view.report);
                                draw_exit_reason_grid(ui, &view.report);
                            });
                    }
                    _ => {
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("No saved trades for this filter.")
                                .color(theme::TEXT_PRIMARY),
                        );
                        let scope = self
                            .report_symbol
                            .as_deref()
                            .unwrap_or("any symbol")
                            .to_owned();
                        let hidden_by_source = self
                            .report_view
                            .as_ref()
                            .map_or(0, |view| view.hidden_by_source);
                        let picked = self.calendar.selection.range();
                        let text = if let Some(range) = picked {
                            // The dates are the user's own pick, so the
                            // refusal names them and the way back out.
                            format!(
                                "{scope} closed no trades in {} - pick another day, or clear \
                                 the date filter to go back to the period pills.",
                                range.label(),
                            )
                        } else if hidden_by_source > 0 {
                            // The trades exist; the one control that would
                            // reveal them must be named, not implied.
                            format!(
                                "{scope} has {hidden_by_source} trade(s) behind the Source \
                                 filter. Try Source \"Both\".",
                            )
                        } else {
                            format!(
                                "{scope} has no trades in {}. Try \"All\", or another symbol.",
                                self.report_period.phrase(),
                            )
                        };
                        ui.label(egui::RichText::new(text).color(theme::TEXT_SUPPORT).small());
                        let default_symbol =
                            (!env.symbol.is_empty()).then(|| env.symbol.to_owned());
                        if (self.report_period != ReportPeriod::All
                            || self.report_symbol != default_symbol
                            || picked.is_some())
                            && ui
                                .button("Clear filters")
                                .on_hover_text("back to this symbol, all time, no dates")
                                .clicked()
                        {
                            self.report_symbol = default_symbol;
                            self.report_period = ReportPeriod::All;
                            self.report_source = SourceFilter::Real;
                            self.pick_report_dates(DaySelection::None);
                            reload = true;
                        }
                    }
                }
                ui.add_space(6.0);
                if let Some(history) = &self.report {
                    if history.unreadable_files > 0 || history.problem_rows > 0 {
                        ui.label(
                            egui::RichText::new(format!(
                                "{} file(s) unreadable, {} row(s) skipped - counted, never \
                                 silently dropped.",
                                history.unreadable_files, history.problem_rows,
                            ))
                            .color(theme::WARN)
                            .small(),
                        );
                    }
                    ui.label(
                        egui::RichText::new(format!(
                            "{} trade(s) across {} file(s) in scope",
                            history.rows.len(),
                            history.files
                        ))
                        .color(theme::TEXT_MUTED)
                        .small(),
                    );
                }
                ui.label(
                    egui::RichText::new(
                        "All figures are simulated, in points (price units × quantity) - \
                         the workspace knows no per-instrument currency value.",
                    )
                    .color(theme::TEXT_MUTED)
                    .small(),
                );
            });
        if toggle_list {
            self.report_list_open = !self.report_list_open;
        }
        if reload {
            self.reload_report(env);
        }
        if !open {
            self.report_open = false;
        }
        // The filter row may have posted a refusal during this very frame.
        if let Some(message) = self.toast.take() {
            asked.toast = Some(message);
        }
        asked
    }

    /// The filter row (symbol combo + period pills + refresh) and the
    /// support line stating the anchor out loud. Returns whether the
    /// history must be re-read; anything it wants the *host* to do goes
    /// into `asked`.
    fn draw_report_filters(&mut self, ui: &mut egui::Ui, asked: &mut ReportResponse) -> bool {
        let mut reload = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Symbol")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            let selected = self
                .report_symbol
                .clone()
                .unwrap_or_else(|| "All symbols".to_owned());
            let symbols = self.report_symbols.clone();
            egui::ComboBox::from_id_salt("paper_report_symbol")
                .width(140.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.report_symbol.is_none(), "All symbols")
                        .clicked()
                    {
                        self.report_symbol = None;
                        reload = true;
                    }
                    for symbol in symbols {
                        let on = self.report_symbol.as_deref() == Some(symbol.as_str());
                        if ui.selectable_label(on, &symbol).clicked() {
                            self.report_symbol = Some(symbol);
                            reload = true;
                        }
                    }
                });
            ui.separator();
            ui.label(
                egui::RichText::new("Source")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            for source in SourceFilter::PILLS {
                let on = self.report_source == source;
                if pill_toggle(ui, source.label(), on, source.hover()).clicked() && !on {
                    self.report_source = source;
                    self.report_view = None;
                }
            }
            ui.separator();
            ui.label(
                egui::RichText::new("Period")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            // A picked calendar range takes the cut over, so no pill may
            // keep looking armed: a lit "Today" beside a date chip reads as
            // the filter that produced the numbers, and it is not.
            let ranged = self.calendar.selection.range().is_some();
            for period in ReportPeriod::PILLS {
                let on = !ranged && self.report_period == period;
                let hover = if ranged {
                    "standing down while a date range is picked - clear the dates to use the \
                     period pills again"
                } else {
                    "measured back from the newest saved trade in scope, not the wall clock"
                };
                if pill_toggle(ui, period.label(), on, hover).clicked() && !on {
                    // Reaching for a pill is a decision to stop filtering
                    // by date, so it says so rather than doing nothing.
                    self.pick_report_dates(DaySelection::None);
                    self.report_period = period;
                    self.report_view = None;
                }
            }
            let response = ui
                .add(
                    egui::TextEdit::singleline(&mut self.report_custom_text)
                        .desired_width(CUSTOM_PERIOD_FIELD_PX)
                        .hint_text("2d"),
                )
                .on_hover_text("type a period - 45m, 12h, 2d or 1w - and press Enter");
            if response.lost_focus() {
                match parse_period(&self.report_custom_text) {
                    // A valid entry applies on blur as well as on Enter —
                    // a typed "2d" must never do nothing quietly.
                    Some(period_ms) => {
                        self.report_period = ReportPeriod::Custom(period_ms);
                        self.report_view = None;
                    }
                    // Only Enter earns the refusal toast: clicking away
                    // from an abandoned half-entry is not a submission.
                    None if ui.input(|input| input.key_pressed(egui::Key::Enter)) => self
                        .show_toast(format!(
                            "SIM: could not read `{}` as a period - use 45m, 12h, 2d or 1w",
                            self.report_custom_text.trim(),
                        )),
                    None => {}
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(icons::ARROWS_CLOCKWISE)
                    .on_hover_text("re-read the history folder")
                    .clicked()
                {
                    reload = true;
                }
                if ui
                    .small_button(icons::DOWNLOAD_SIMPLE)
                    .on_hover_text(
                        "import trades from another folder - copies, the folder keeps its files",
                    )
                    .clicked()
                {
                    asked.start_import = true;
                }
            });
        });
        if let Some(view) = &self.report_view {
            let text = match (view.range, view.anchor_ms) {
                // A picked range names itself in absolute dates: the whole
                // point of the calendar is a window the trader can state.
                (Some(range), _) => {
                    let mut text = format!(
                        "{} ({} day(s), the dates you picked - the period pills are standing \
                         down)",
                        range.label(),
                        range.days(),
                    );
                    if view.hidden_outside > 0 {
                        text.push_str(&format!(
                            " - {} saved trade(s) outside these dates",
                            view.hidden_outside,
                        ));
                    }
                    if view.hidden_by_source > 0 {
                        text.push_str(&format!(
                            " - {} trade(s) behind the Source filter",
                            view.hidden_by_source,
                        ));
                    }
                    text
                }
                (None, Some(anchor)) => {
                    let mut text = format!(
                        "{} up to {} (newest saved trade, not the wall clock)",
                        view.period.phrase(),
                        CivilDate::from_ms(anchor, view.tz).iso(),
                    );
                    if view.hidden_outside > 0 {
                        // "Where did my old trades go" gets a literal
                        // answer: they are saved, before this window.
                        text.push_str(&format!(
                            " - {} older saved trade(s) before this window",
                            view.hidden_outside,
                        ));
                    }
                    if view.hidden_by_source > 0 {
                        text.push_str(&format!(
                            " - {} trade(s) behind the Source filter",
                            view.hidden_by_source,
                        ));
                    }
                    text
                }
                (None, None) if view.hidden_by_source > 0 => format!(
                    "no saved trades in this scope - {} trade(s) sit behind the Source \
                     filter (try Both)",
                    view.hidden_by_source,
                ),
                (None, None) => "no saved trades in this scope".to_owned(),
            };
            ui.label(egui::RichText::new(text).color(theme::TEXT_SUPPORT).small());
        }
        reload
    }

    /// The calendar row: a toggle that names what is picked, and - when
    /// expanded - the month grid itself. Collapsed, the report keeps the
    /// layout it always had; expanded it answers "which days did I trade,
    /// and what happened on the 12th" without a text field.
    fn draw_report_calendar(&mut self, ui: &mut egui::Ui, tz: TzOffset) {
        ui.horizontal(|ui| {
            let picked = self.calendar.selection.range();
            let label = match picked {
                Some(range) => format!("{} {}", icons::CALENDAR_BLANK, range.label()),
                None => format!("{} Dates", icons::CALENDAR_BLANK),
            };
            if pill_toggle(
                ui,
                &label,
                self.calendar.open || picked.is_some(),
                "pick a day, or click a second day for a range - highlighted days hold trades",
            )
            .clicked()
            {
                self.calendar.open = !self.calendar.open;
            }
            if let Some(range) = picked {
                ui.label(
                    egui::RichText::new(format!(
                        "{} day(s) · {} trade(s)",
                        range.days(),
                        self.report_view.as_ref().map_or(0, |view| view.rows.len()),
                    ))
                    .color(theme::TEXT_MUTED)
                    .small(),
                );
                if ui
                    .small_button(icons::X)
                    .on_hover_text("clear the date filter and go back to the period pills")
                    .clicked()
                {
                    self.pick_report_dates(DaySelection::None);
                }
            } else if self.report_days.is_empty() {
                ui.label(
                    egui::RichText::new("no days on record in this scope")
                        .color(theme::TEXT_MUTED)
                        .small(),
                );
            } else if let (Some(oldest), Some(newest)) =
                (self.report_days.first(), self.report_days.last())
            {
                // The span on record, stated: it is the answer to "how far
                // back can I even ask" before the first click is made.
                ui.label(
                    egui::RichText::new(format!(
                        // "to", not an arrow: this label is drawn in the
                        // proportional UI font, which has no glyph for → and
                        // renders a tofu box in its place (the same reason
                        // DateRange::label spells its span with a word).
                        "{} day(s) with trades · {} to {}",
                        self.report_days.len(),
                        oldest.iso(),
                        newest.iso(),
                    ))
                    .color(theme::TEXT_MUTED)
                    .small(),
                );
            }
        });
        if !self.calendar.open {
            return;
        }
        // The grid is drawn from a cached day index, so an open calendar
        // costs a fixed 42 cells per frame however long the history is.
        let mut calendar = self.calendar;
        let action = crate::paper_calendar::draw_month(
            ui,
            &self.report_days,
            &mut calendar,
            egui::vec2(CALENDAR_CELL_W_PX, CALENDAR_CELL_H_PX),
            // Only reached when nothing on disk names a month. The
            // calendar module is deliberately clock-free, so the host —
            // which may read a clock — says where "no history at all"
            // should open.
            today(tz),
        );
        // The grid reports a pick; applying it is the named action's job,
        // never the paint's — the click and the hook take one path.
        self.calendar.month = calendar.month;
        if action == Some(CalendarAction::SelectionChanged) {
            self.pick_report_dates(calendar.selection);
        }
    }
}

/// The three headline tiles: NET (the one coloured number in the window),
/// WIN RATE, PROFIT FACTOR — each with its denominator under it, so every
/// tile is a self-explaining fact rather than a floating statistic.
fn draw_report_tiles(ui: &mut egui::Ui, report: &PerformanceReport) {
    let width = ((ui.available_width() - 2.0 * TILE_GUTTER_PX) / 3.0).max(80.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = TILE_GUTTER_PX;
        draw_tile(
            ui,
            width,
            "NET",
            fmt_signed_points(report.net_points),
            "pts",
            points_color(report.net_points),
            format!("{} trades", report.trades),
        );
        draw_tile(
            ui,
            width,
            "WIN RATE",
            report
                .win_rate_pct
                .map_or_else(|| "—".to_owned(), |rate| fmt_decimal(rate.round_dp(0))),
            "%",
            theme::TEXT_PRIMARY,
            format!(
                "{} W · {} L · {} scratch",
                report.wins, report.losses, report.scratches
            ),
        );
        draw_tile(
            ui,
            width,
            "PROFIT FACTOR",
            report
                .profit_factor
                .map_or_else(|| "—".to_owned(), fmt_points),
            "",
            theme::TEXT_PRIMARY,
            format!(
                "+{} / -{}",
                fmt_points(report.gross_profit),
                fmt_points(report.gross_loss)
            ),
        );
    });
}

/// One headline tile: caption, hero value with its unit, denominator line.
fn draw_tile(
    ui: &mut egui::Ui,
    width: f32,
    label: &str,
    value: String,
    unit: &str,
    value_color: egui::Color32,
    subline: String,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, TILE_HEIGHT_PX), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::same(4.0), theme::INSET);
    painter.rect_stroke(
        rect,
        egui::Rounding::same(4.0),
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    painter.text(
        rect.min + egui::vec2(12.0, 6.0),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    let value_galley = painter.layout_no_wrap(
        value,
        egui::FontId::monospace(HEADLINE_FONT_PX),
        value_color,
    );
    let value_w = value_galley.size().x;
    let baseline = rect.top() + 38.0;
    painter.galley(
        egui::pos2(rect.left() + 12.0, baseline - value_galley.size().y),
        value_galley,
        value_color,
    );
    if !unit.is_empty() {
        painter.text(
            egui::pos2(rect.left() + 12.0 + value_w + 4.0, baseline - 2.0),
            egui::Align2::LEFT_BOTTOM,
            unit,
            egui::FontId::monospace(11.0),
            theme::TEXT_MUTED,
        );
    }
    painter.text(
        egui::pos2(rect.left() + 12.0, rect.bottom() - 6.0),
        egui::Align2::LEFT_BOTTOM,
        subline,
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
}

/// The realized equity curve, `E_k` by trade index — the closing order
/// that defines the drawdown, so calling the axis "time" would misstate
/// what is plotted. One quiet line; a diverging fill against the zero
/// baseline answers "was I ever under water?" at a glance; the deepest
/// drawdown is annotated so the number in the grid below is locatable.
fn draw_equity_curve(ui: &mut egui::Ui, view: &ReportView, floor: f32) {
    let n = view.rows.len();
    if n == 0 {
        return;
    }
    ui.label(caption("REALIZED EQUITY"));
    let height = (ui.available_height() - CURVE_GRID_RESERVE_PX).clamp(floor, CURVE_MAX_H_PX);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter_at(rect);

    // The walk E_0..E_n was cut with the view; the frame only plots it.
    let equity = &view.equity.plot;
    let (low, high) = (view.equity.low, view.equity.high);
    let span = (high - low).max(f32::EPSILON);
    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + CURVE_GUTTER_PX, rect.top() + 4.0),
        egui::pos2(rect.right() - 4.0, rect.bottom() - 14.0),
    );
    let x_at = |k: usize| plot.left() + plot.width() * (k as f32) / (n as f32);
    let y_at = |value: f32| plot.bottom() - (value - low) / span * plot.height();
    let zero_y = y_at(0.0);

    // Gridlines + y ticks at the floor, zero and the ceiling.
    let mut ticks = vec![low, 0.0, high];
    ticks.dedup_by(|a, b| (*a - *b).abs() < f32::EPSILON);
    for tick in ticks {
        let y = y_at(tick);
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(
                1.0_f32,
                egui::Color32::from_rgba_unmultiplied(
                    theme::CONTROL.r(),
                    theme::CONTROL.g(),
                    theme::CONTROL.b(),
                    CURVE_GRID_LINE_ALPHA,
                ),
            ),
        );
        painter.text(
            egui::pos2(plot.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            fmt_points(Decimal::from_f64_retain(f64::from(tick)).unwrap_or_default()),
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
    }

    // Diverging fill to the zero baseline: gains ground in BUY, losses in
    // SELL, split exactly at each crossing.
    let stride = n.div_ceil(CURVE_MAX_POINTS).max(1);
    let fill_of = |value: f32| {
        let color = if value >= 0.0 {
            theme::BUY
        } else {
            theme::SELL
        };
        egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), CURVE_FILL_ALPHA)
    };
    let mut drawn_points = vec![egui::pos2(x_at(0), y_at(equity[0]))];
    let mut previous = 0usize;
    let mut next = stride;
    while previous < n {
        let k = next.min(n);
        let (a, b) = (equity[previous], equity[k]);
        let (x0, x1) = (x_at(previous), x_at(k));
        if a == 0.0 && b == 0.0 {
            // Nothing to fill on the baseline itself.
        } else if (a >= 0.0) == (b >= 0.0) {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, y_at(a)),
                    egui::pos2(x1, y_at(b)),
                    egui::pos2(x1, zero_y),
                    egui::pos2(x0, zero_y),
                ],
                fill_of(if a == 0.0 { b } else { a }),
                egui::Stroke::NONE,
            ));
        } else {
            // The step crosses zero: split it at the exact crossing.
            let t = a / (a - b);
            let xc = x0 + (x1 - x0) * t;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(x0, y_at(a)),
                    egui::pos2(xc, zero_y),
                    egui::pos2(x0, zero_y),
                ],
                fill_of(a),
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(xc, zero_y),
                    egui::pos2(x1, y_at(b)),
                    egui::pos2(x1, zero_y),
                ],
                fill_of(b),
                egui::Stroke::NONE,
            ));
        }
        drawn_points.push(egui::pos2(x1, y_at(b)));
        previous = k;
        next += stride;
    }

    // The zero baseline over the fill, then the line over everything.
    painter.line_segment(
        [
            egui::pos2(plot.left(), zero_y),
            egui::pos2(plot.right(), zero_y),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    painter.add(egui::Shape::line(
        drawn_points,
        egui::Stroke::new(1.5_f32, theme::TEXT_PRIMARY),
    ));
    if n == 1 {
        painter.circle_filled(
            egui::pos2(x_at(1), y_at(equity[1])),
            2.0,
            theme::TEXT_PRIMARY,
        );
        painter.text(
            plot.center(),
            egui::Align2::CENTER_CENTER,
            "one trade — no curve yet",
            egui::FontId::monospace(10.0),
            theme::TEXT_FAINT,
        );
    }

    // The deepest drawdown, locatable: its peak level dotted, the drop
    // solid, the number on a chip at the drop's midpoint.
    if let Some((peak, trough)) = view.report.max_drawdown_span {
        let (peak, trough) = (peak as usize, trough as usize);
        if trough <= n {
            let peak_y = y_at(equity[peak]);
            let trough_x = x_at(trough);
            let trough_y = y_at(equity[trough]);
            painter.extend(egui::Shape::dashed_line(
                &[egui::pos2(x_at(peak), peak_y), egui::pos2(trough_x, peak_y)],
                egui::Stroke::new(1.0_f32, theme::SELL),
                2.0,
                3.0,
            ));
            painter.line_segment(
                [egui::pos2(trough_x, peak_y), egui::pos2(trough_x, trough_y)],
                egui::Stroke::new(1.0_f32, theme::SELL),
            );
            let label = format!("-{} pts", fmt_points(view.report.max_drawdown_points));
            let galley =
                painter.layout_no_wrap(label, egui::FontId::monospace(10.0), theme::CHIP_INK);
            let center = egui::pos2((x_at(peak) + trough_x) / 2.0, (peak_y + trough_y) / 2.0);
            let bg = egui::Rect::from_center_size(center, galley.size() + egui::vec2(8.0, 4.0));
            painter.rect_filled(bg, egui::Rounding::same(2.0), theme::SELL);
            painter.galley(bg.min + egui::vec2(4.0, 2.0), galley, theme::CHIP_INK);
        }
    }

    // Axis words: first and last trade index, with the unit between them.
    painter.text(
        egui::pos2(plot.left(), rect.bottom()),
        egui::Align2::LEFT_BOTTOM,
        "1",
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(plot.right(), rect.bottom()),
        egui::Align2::RIGHT_BOTTOM,
        n.to_string(),
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(plot.center().x, rect.bottom()),
        egui::Align2::CENTER_BOTTOM,
        "trades",
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
    if stride > 1 {
        painter.text(
            egui::pos2(plot.left() + 4.0, plot.top() + 2.0),
            egui::Align2::LEFT_TOP,
            format!(
                "curve downsampled to ~{CURVE_MAX_POINTS} points; every metric uses every trade"
            ),
            egui::FontId::proportional(10.0),
            theme::TEXT_SUPPORT,
        );
    }

    // Hover: snap to the nearest trade index and answer in the ledger's
    // vocabulary, so the two surfaces never disagree.
    if let Some(pointer) = response.hover_pos()
        && plot.contains(pointer)
    {
        let k = (((pointer.x - plot.left()) / plot.width()) * (n as f32))
            .round()
            .clamp(0.0, n as f32) as usize;
        let x = x_at(k);
        painter.line_segment(
            [egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())],
            egui::Stroke::new(1.0_f32, theme::TEXT_FAINT),
        );
        let head = if k == 0 {
            "start".to_owned()
        } else {
            let row = &view.rows[k - 1];
            format!(
                "#{k} · {} · {} {} · {} pts",
                row.symbol,
                position_word(row.trade.side),
                fmt_decimal(row.trade.quantity),
                fmt_signed_points(row.trade.pnl_points),
            )
        };
        let lines = [
            head,
            format!(
                "equity {} pts",
                fmt_signed_points(
                    Decimal::from_f64_retain(f64::from(equity[k])).unwrap_or_default()
                )
            ),
            if k == 0 {
                String::new()
            } else {
                // The curve's stamp reads on the same clock as the list
                // under it, not on UTC: two dates for one trade in one
                // window is the confusion this goal exists to end.
                fmt_offset_minute(view.rows[k - 1].trade.closed_ms, view.tz)
            },
        ];
        draw_hover_card(&painter, rect, pointer, &lines);
    }
}

/// A small TAG_BG hover card with up to three lines, clamped into `rect`.
fn draw_hover_card(
    painter: &egui::Painter,
    rect: egui::Rect,
    pointer: egui::Pos2,
    lines: &[String],
) {
    let font = egui::FontId::monospace(10.0);
    let galleys: Vec<_> = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| painter.layout_no_wrap(line.clone(), font.clone(), theme::TEXT_PRIMARY))
        .collect();
    if galleys.is_empty() {
        return;
    }
    let width = galleys
        .iter()
        .map(|galley| galley.size().x)
        .fold(0.0_f32, f32::max)
        + 12.0;
    let height: f32 = galleys.iter().map(|galley| galley.size().y).sum::<f32>() + 10.0;
    let mut origin = pointer + egui::vec2(10.0, -height - 6.0);
    origin.x = origin.x.min(rect.right() - width).max(rect.left());
    origin.y = origin.y.max(rect.top());
    let card = egui::Rect::from_min_size(origin, egui::vec2(width, height));
    painter.rect_filled(card, egui::Rounding::same(4.0), theme::TAG_BG);
    let mut y = card.top() + 5.0;
    for galley in galleys {
        let advance = galley.size().y;
        painter.galley(
            egui::pos2(card.left() + 6.0, y),
            galley,
            theme::TEXT_PRIMARY,
        );
        y += advance;
    }
}

/// The trade list's columns: caption and width in pixels, in paint order.
/// One table, so the header row and every trade row can only ever agree
/// about where a column begins.
const TRADE_LIST_COLUMNS: [(&str, f32); 11] = [
    ("#", 38.0),
    ("DATE", 70.0),
    ("TIME", 58.0),
    ("SYMBOL", 66.0),
    ("SIDE", 56.0),
    ("ENTRY → EXIT", 122.0),
    ("HELD", 52.0),
    ("EXIT", 74.0),
    ("PTS", 52.0),
    ("EQUITY", 60.0),
    // Not a hover: under Source "Both" a practice trade and a real one
    // are otherwise the same row, and a replay result readable as a real
    // one is the worst thing this window could do.
    ("SOURCE", 58.0),
];

/// One trade-list row's height. Tight on purpose: this is a table to scan,
/// not a list to browse.
const REPORT_LIST_ROW_H_PX: f32 = 17.0;

/// Left padding inside a trade-list cell.
const TRADE_LIST_CELL_PAD_PX: f32 = 4.0;

/// The trade list's full width — the sum of its columns.
fn trade_list_width() -> f32 {
    TRADE_LIST_COLUMNS.iter().map(|(_, width)| width).sum()
}

/// Paint one row of cells on the shared column grid, each cut to its own
/// column rather than allowed to run into the next one.
fn paint_list_row(
    painter: &egui::Painter,
    rect: egui::Rect,
    glyph_w: f32,
    cells: &[(String, egui::Color32)],
) {
    let font = egui::FontId::monospace(10.0);
    let mut x = rect.left();
    for ((text, color), (_, width)) in cells.iter().zip(TRADE_LIST_COLUMNS) {
        let budget = ((width - 2.0 * TRADE_LIST_CELL_PAD_PX) / glyph_w)
            .floor()
            .max(0.0) as usize;
        painter.text(
            egui::pos2(x + TRADE_LIST_CELL_PAD_PX, rect.center().y),
            egui::Align2::LEFT_CENTER,
            elide_tail(text, budget),
            font.clone(),
            *color,
        );
        x += width;
    }
}

/// The trades behind the curve: every trade the filters kept, in closing
/// order, each naming its date, instrument, side, round trip, result and
/// why it ended. A performance screen without this is a shape with no
/// story — "I see the chart and the number, but not which trades those
/// were" is exactly the gap it closes.
///
/// Virtualised: the list holds whatever the filters kept — an "All" window
/// can be thousands of trades — and only the rows on screen are ever laid
/// out. Returns whether the section's collapse control was clicked.
fn draw_trade_list(ui: &mut egui::Ui, view: &ReportView, open: bool) -> bool {
    let mut toggled = false;
    ui.horizontal(|ui| {
        ui.label(caption("TRADES BEHIND THIS CURVE"));
        ui.label(
            egui::RichText::new(format!("{}", view.rows.len()))
                .color(theme::TEXT_MUTED)
                .small(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let icon = if open {
                icons::CARET_UP
            } else {
                icons::CARET_DOWN
            };
            if ui
                .small_button(icon)
                .on_hover_text(if open {
                    "collapse the trade list"
                } else {
                    "list every trade in this window"
                })
                .clicked()
            {
                toggled = true;
            }
        });
    });
    if !open {
        return toggled;
    }
    let width = trade_list_width();
    let glyph_w = ui
        .painter()
        .layout_no_wrap(
            "0".to_owned(),
            egui::FontId::monospace(10.0),
            theme::TEXT_MUTED,
        )
        .size()
        .x
        .max(1.0);
    // Wide by nature — ten columns of facts — and long by nature too. It
    // scrolls both ways inside a bounded strip rather than pushing the
    // window wider, clipping a number in half, or growing until the metric
    // grids beneath it are out of reach. Row zero is the header, so it
    // rides the same column grid and the same horizontal scroll.
    egui::ScrollArea::both()
        .id_salt("paper_report_trade_list")
        .max_height(REPORT_LIST_MAX_H_PX)
        // Vertically it shrinks to its content: a five-trade window must
        // not reserve the whole strip and push the grids off the screen.
        .auto_shrink([false, true])
        .show_rows(
            ui,
            REPORT_LIST_ROW_H_PX,
            view.rows.len() + 1,
            |ui, range| {
                for index in range {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(width, REPORT_LIST_ROW_H_PX),
                        egui::Sense::hover(),
                    );
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    if index == 0 {
                        let cells: Vec<(String, egui::Color32)> = TRADE_LIST_COLUMNS
                            .iter()
                            .map(|(label, _)| ((*label).to_owned(), theme::TEXT_FAINT))
                            .collect();
                        paint_list_row(ui.painter(), rect, glyph_w, &cells);
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left(), rect.bottom()),
                                egui::pos2(rect.right(), rect.bottom()),
                            ],
                            egui::Stroke::new(1.0_f32, theme::BORDER),
                        );
                        continue;
                    }
                    let ordinal = index - 1;
                    let row = &view.rows[ordinal];
                    let trade = &row.trade;
                    // The running total was cut with the view, so a row
                    // deep in the list costs no more than the first.
                    let equity = view
                        .equity
                        .points
                        .get(index)
                        .copied()
                        .unwrap_or(Decimal::ZERO);
                    if response.hovered() {
                        ui.painter()
                            .rect_filled(rect, egui::Rounding::ZERO, theme::BORDER);
                    } else if ordinal % 2 == 1 {
                        ui.painter()
                            .rect_filled(rect, egui::Rounding::ZERO, theme::INSET);
                    }
                    let muted = theme::TEXT_MUTED;
                    let cells = [
                        (format!("{}", ordinal + 1), theme::TEXT_FAINT),
                        (
                            CivilDate::from_ms(trade.closed_ms, view.tz).iso(),
                            theme::TEXT_PRIMARY,
                        ),
                        (crate::plot_area::fmt_time(trade.closed_ms, view.tz), muted),
                        (row.symbol.clone(), theme::TEXT_PRIMARY),
                        (
                            format!(
                                "{} {}",
                                position_word(trade.side),
                                fmt_decimal(trade.quantity)
                            ),
                            theme::side_color(trade.side),
                        ),
                        (
                            format!(
                                "{} → {}",
                                fmt_decimal(trade.entry_price),
                                fmt_decimal(trade.exit_price)
                            ),
                            muted,
                        ),
                        (
                            fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
                            muted,
                        ),
                        (trade.exit_reason.as_str().replace('_', " "), muted),
                        (
                            fmt_signed_points(trade.pnl_points),
                            points_color(trade.pnl_points),
                        ),
                        (fmt_signed_points(equity), points_color(equity)),
                        match row.source {
                            Some(history::SessionSource::Live) => {
                                ("live".to_owned(), theme::TEXT_FAINT)
                            }
                            Some(history::SessionSource::Replay) => {
                                ("replay".to_owned(), theme::WARN)
                            }
                            // Unrecorded is not "live": a file from before
                            // the source line existed says so with a mark
                            // that reads as absence, never as a fact.
                            None => ("—".to_owned(), theme::TEXT_FAINT),
                        },
                    ];
                    paint_list_row(ui.painter(), rect, glyph_w, &cells);
                    // Every fact the columns cannot hold whole, on hover -
                    // including the session source, which is never guessed.
                    // Spelled with words, not an arrow: a hover card is
                    // drawn in the proportional UI font, which has no glyph
                    // for → and paints a tofu box. The arrow survives in the
                    // painted row above, which is monospace.
                    response.on_hover_text(format!(
                        "#{} · {} {} · {} to {} · {} pts · {} · held {} · {}",
                        ordinal + 1,
                        position_word(trade.side),
                        fmt_decimal(trade.quantity),
                        fmt_decimal(trade.entry_price),
                        fmt_decimal(trade.exit_price),
                        fmt_signed_points(trade.pnl_points),
                        trade.exit_reason.as_str().replace('_', " "),
                        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
                        match row.source {
                            Some(source) => format!("{} session", source.as_str()),
                            None => "saved before quantick recorded a session source".to_owned(),
                        },
                    ));
                }
            },
        );
    ui.add_space(6.0);
    toggled
}

/// The metric grid: one metric per row, the explanation on hover, honest
/// blanks (`—`) where a ratio has no denominator. Every value here has a
/// structurally fixed sign, so none of them is coloured.
fn draw_report_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    let blank = || "—".to_owned();
    let pts = |value: Decimal| format!("{} pts", fmt_points(value));
    let opt_pts = |value: Option<Decimal>| {
        value.map_or_else(blank, |value| format!("{} pts", fmt_points(value)))
    };
    let opt_plain = |value: Option<Decimal>| value.map_or_else(blank, fmt_points);
    let duration = |value: Option<i64>| value.map_or_else(blank, fmt_duration_ms);
    egui::Grid::new("paper_report_grid")
        .num_columns(2)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            let mut row = |label: &str, value: String, explain: &str| {
                ui.label(egui::RichText::new(label).color(theme::TEXT_MUTED))
                    .on_hover_text(explain.to_owned());
                ui.label(value);
                ui.end_row();
            };
            row(
                "trades",
                format!(
                    "{} ({} long / {} short)",
                    report.trades, report.long_trades, report.short_trades
                ),
                "closed round trips under the current filter",
            );
            row(
                "max drawdown",
                pts(report.max_drawdown_points),
                "deepest drop of realized equity below its running peak, in closing order",
            );
            row(
                "max run-up",
                pts(report.max_runup_points),
                "highest rise of realized equity above its running trough — the drawdown's mirror",
            );
            row(
                "recovery factor",
                opt_plain(report.recovery_factor),
                "net profit divided by the max drawdown; blank while nothing drew down",
            );
            row(
                "gross profit / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.gross_profit),
                    fmt_points(report.gross_loss)
                ),
                "winners' points and losers' magnitude, before netting",
            );
            row(
                "avg win / loss",
                format!(
                    "{} / {} pts",
                    opt_plain(report.avg_win),
                    opt_plain(report.avg_loss),
                ),
                "mean winner and mean loser magnitude",
            );
            row(
                "payoff ratio",
                opt_plain(report.payoff_ratio),
                "average win divided by average loss — how much a winner pays for a loser",
            );
            row(
                "expectancy",
                report
                    .expectancy_points
                    .map_or_else(blank, |value| format!("{} pts/trade", fmt_points(value))),
                "net profit per trade: what one average trade pays",
            );
            row(
                "std deviation",
                opt_pts(report.stddev_points),
                "sample standard deviation of trade points (N−1); blank below two trades",
            );
            row(
                "longest streaks",
                format!(
                    "{} wins / {} losses",
                    report.max_consecutive_wins, report.max_consecutive_losses
                ),
                "longest consecutive runs, in closing order; a scratch breaks both",
            );
            row(
                "avg / median duration",
                format!(
                    "{} / {}",
                    duration(report.avg_duration_ms),
                    duration(report.median_duration_ms),
                ),
                "trade lifetimes in venue time",
            );
            row(
                "winner vs loser duration",
                format!(
                    "{} / {}",
                    duration(report.avg_win_duration_ms),
                    duration(report.avg_loss_duration_ms),
                ),
                "how long winners run against how long losers are held",
            );
            row(
                "largest win / loss",
                format!(
                    "{} / {} pts",
                    fmt_points(report.largest_win),
                    fmt_points(report.largest_loss)
                ),
                "best single trade and worst single trade magnitude",
            );
            row(
                "avg winner MAE",
                report.avg_winner_mae_points.map_or_else(blank, |value| {
                    format!(
                        "{} pts (over {} of {})",
                        fmt_points(value),
                        report.winners_with_mae,
                        report.wins
                    )
                }),
                "how far the average winner first ran against you; only trades that \
                 recorded an excursion count, and the denominator says how many did",
            );
            row(
                "avg loser MFE",
                report.avg_loser_mfe_points.map_or_else(blank, |value| {
                    format!(
                        "{} pts (over {} of {})",
                        fmt_points(value),
                        report.losers_with_mfe,
                        report.losses
                    )
                }),
                "how far the average loser was in profit before it lost; same disclosure",
            );
        });
}

/// Long and short, side by side, with the core metrics in each column.
fn draw_side_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    ui.add_space(8.0);
    ui.label(caption("LONG VS SHORT"));
    fn opt_plain(value: Option<Decimal>) -> String {
        value.map_or_else(|| "—".to_owned(), fmt_points)
    }
    /// One side-by-side row's value, computed per column.
    type SideValue = Box<dyn Fn(&SideReport) -> String>;
    let side_rows: [(&str, SideValue); 7] = [
        ("trades", Box::new(|side| side.trades.to_string())),
        (
            "net",
            Box::new(|side| format!("{} pts", fmt_signed_points(side.net_points))),
        ),
        (
            "win rate",
            Box::new(|side| {
                side.win_rate_pct
                    .map_or_else(|| "—".to_owned(), |rate| format!("{}%", fmt_points(rate)))
            }),
        ),
        (
            "profit factor",
            Box::new(|side| opt_plain(side.profit_factor)),
        ),
        ("avg win", Box::new(|side| opt_plain(side.avg_win))),
        ("avg loss", Box::new(|side| opt_plain(side.avg_loss))),
        (
            "expectancy",
            Box::new(|side| opt_plain(side.expectancy_points)),
        ),
    ];
    egui::Grid::new("paper_report_sides")
        .num_columns(3)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            ui.label(egui::RichText::new("LONG").color(theme::BUY).small());
            ui.label(egui::RichText::new("SHORT").color(theme::SELL).small());
            ui.end_row();
            for (label, value_of) in &side_rows {
                ui.label(egui::RichText::new(*label).color(theme::TEXT_MUTED));
                ui.label(value_of(&report.long));
                ui.label(value_of(&report.short));
                ui.end_row();
            }
        });
}

/// How trades that left one way performed — count and net per exit reason.
fn draw_exit_reason_grid(ui: &mut egui::Ui, report: &PerformanceReport) {
    if report.by_exit_reason.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.label(caption("BY EXIT REASON"));
    egui::Grid::new("paper_report_reasons")
        .num_columns(3)
        .spacing([16.0, 4.0])
        .show(ui, |ui| {
            for reason in &report.by_exit_reason {
                ui.label(
                    egui::RichText::new(reason.reason.as_str().replace('_', " "))
                        .color(theme::TEXT_MUTED),
                );
                ui.label(format!("{} trade(s)", reason.trades));
                ui.label(format!("{} pts", fmt_signed_points(reason.net_points)));
                ui.end_row();
            }
        });
}

/// Read every history file under `dir` (one symbol's folder, or all of
/// them), remembering each row's symbol and skipping every path in
/// `exclude` (the live session's own files — their trades are already in
/// the simulator). Missing folders are simply empty, not an error.
pub(crate) fn load_history(dir: &Path, symbol: Option<&str>, exclude: &[PathBuf]) -> LoadedHistory {
    let mut folders = Vec::new();
    match symbol {
        Some(symbol) => folders.push(dir.join(sanitize_symbol(symbol))),
        None => {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        folders.push(path);
                    }
                }
                folders.sort();
            }
        }
    }
    let mut rows = Vec::new();
    let mut files = 0usize;
    let mut unreadable_files = 0usize;
    let mut problem_rows = 0usize;
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        let folder_symbol = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == history::FILE_EXTENSION)
            })
            .collect();
        paths.sort();
        for path in paths {
            if exclude.contains(&path) {
                continue;
            }
            files += 1;
            match std::fs::read_to_string(&path).map_err(|error| error.to_string()) {
                Ok(text) => match history::parse(&text) {
                    Ok(parsed) => {
                        problem_rows += parsed.problems.len();
                        let symbol = parsed.symbol.unwrap_or_else(|| folder_symbol.clone());
                        let source = parsed.source;
                        rows.extend(parsed.trades.into_iter().map(|trade| HistoryRow {
                            symbol: symbol.clone(),
                            source,
                            trade,
                        }));
                    }
                    Err(_) => unreadable_files += 1,
                },
                Err(_) => unreadable_files += 1,
            }
        }
    }
    // Files are per-session; merge into one closing-order timeline so the
    // drawdown walk is honest across sessions.
    rows.sort_by_key(|row| (row.trade.closed_ms, row.trade.opened_ms));
    LoadedHistory {
        rows,
        files,
        unreadable_files,
        problem_rows,
    }
}

/// Aggregate loaded history rows — the tests' shortcut from a journal on
/// disk to a report.
#[cfg(test)]
pub(crate) fn report_from_history(history: &LoadedHistory) -> PerformanceReport {
    let trades: Vec<ClosedTrade> = history.rows.iter().map(|row| row.trade.clone()).collect();
    PerformanceReport::from_trades(&trades)
}
fn push_by_day<'a, T>(
    rows: &mut Vec<LedgerRow<'a>>,
    items: &'a [T],
    tz: TzOffset,
    collapsed: &std::collections::BTreeSet<i64>,
    trade_of: impl Fn(&'a T) -> &'a ClosedTrade,
    row_of: impl Fn(&'a T) -> LedgerRow<'a>,
) {
    let mut start = 0;
    while start < items.len() {
        let day = CivilDate::from_ms(trade_of(&items[start]).closed_ms, tz);
        let mut end = start;
        let mut net = Decimal::ZERO;
        while end < items.len() && CivilDate::from_ms(trade_of(&items[end]).closed_ms, tz) == day {
            net = net.saturating_add(trade_of(&items[end]).pnl_points);
            end += 1;
        }
        let folded = collapsed.contains(&day.day_number());
        rows.push(LedgerRow::Day(day, end - start, net, folded));
        // A folded day builds no trade rows at all — the header keeps the
        // date, the count and the net, so the day is summarised rather
        // than merely hidden, and the frame does not pay for what it does
        // not show.
        if !folded {
            rows.extend(items[start..end].iter().map(&row_of));
        }
        start = end;
    }
}

/// A group caption inside the virtualised list, sharing the fixed row
/// height so `show_rows` stays honest about where every row is.
fn draw_group_header(ui: &mut egui::Ui, label: &str, count: usize) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    ui.painter().text(
        egui::pos2(rect.left() + 2.0, rect.bottom() - 4.0),
        egui::Align2::LEFT_BOTTOM,
        format!("{label} · {count}"),
        egui::FontId::monospace(10.0),
        theme::TEXT_FAINT,
    );
}

/// A civil day's caption: a fold caret and the date on the left, the
/// day's trade count and net on the right, tinted by the result. The row
/// is the ledger's answer to "which day am I looking at" while scrolling
/// back through months, and clicking it folds that day to this one line.
/// Returns whether it was clicked.
fn draw_day_header(
    ui: &mut egui::Ui,
    date: CivilDate,
    count: usize,
    net: Decimal,
    folded: bool,
) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let painter = ui.painter();
    let band = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + DAY_HEADER_INSET_PX),
        rect.right_bottom(),
    );
    painter.rect_filled(
        band,
        egui::Rounding::ZERO,
        if response.hovered() {
            theme::BORDER
        } else {
            theme::INSET
        },
    );
    painter.line_segment(
        [
            egui::pos2(rect.left(), band.top()),
            egui::pos2(rect.right(), band.top()),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    // The caret is the affordance: a header that folds must look like it
    // folds, or the click is a secret.
    painter.text(
        egui::pos2(rect.left() + 6.0, band.center().y),
        egui::Align2::LEFT_CENTER,
        if folded {
            icons::CARET_RIGHT
        } else {
            icons::CARET_DOWN
        },
        egui::FontId::proportional(10.0),
        theme::TEXT_FAINT,
    );
    painter.text(
        egui::pos2(rect.left() + DAY_HEADER_TEXT_X_PX, band.center().y),
        egui::Align2::LEFT_CENTER,
        date.long(),
        egui::FontId::monospace(10.0),
        if folded {
            theme::TEXT_FAINT
        } else {
            theme::TEXT_MUTED
        },
    );
    painter.text(
        egui::pos2(rect.right() - 6.0, band.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{count} · {}", fmt_signed_points(net)),
        egui::FontId::monospace(10.0),
        points_color(net),
    );
    response
        .on_hover_text(format!(
            "{} · {count} trade(s) closed · {} pts on the day - click to {}",
            date.iso(),
            fmt_signed_points(net),
            if folded { "open it" } else { "fold it shut" },
        ))
        .clicked()
}

/// The "show older" control at the foot of the saved history: it names how
/// many trades are still held back, so the end of the list is never
/// mistaken for the end of the history.
fn draw_more_row(ui: &mut egui::Ui, remaining: usize) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::click(),
    );
    if !ui.is_rect_visible(rect) {
        return response.clicked();
    }
    let body = rect.shrink2(egui::vec2(6.0, 4.0));
    ui.painter().rect_filled(
        body,
        egui::Rounding::same(4.0),
        if response.hovered() {
            theme::BORDER
        } else {
            theme::CONTROL
        },
    );
    ui.painter().text(
        body.center(),
        egui::Align2::CENTER_CENTER,
        format!("{}  show older · {remaining} more saved", icons::CARET_DOWN),
        egui::FontId::monospace(10.0),
        theme::TEXT_MUTED,
    );
    response
        .on_hover_text(format!(
            "reveal the next {LEDGER_PAGE_TRADES} saved trade(s) - {remaining} still held back"
        ))
        .clicked()
}

/// The pinned open-position row: sunken, live open points on the right,
/// the current mark standing in for the exit.
fn draw_open_row(
    ui: &mut egui::Ui,
    summary: &PositionSummary,
    symbol: Option<&str>,
    mark: Option<Decimal>,
    held_ms: Option<i64>,
) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width, LEDGER_ROW_HEIGHT_PX),
        egui::Sense::hover(),
    );
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, egui::Rounding::ZERO, theme::INSET);
    for y in [rect.top(), rect.bottom()] {
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(1.0_f32, theme::BORDER),
        );
    }
    painter.rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        theme::side_color(summary.side),
    );
    let exit = mark.map_or_else(|| "…".to_owned(), fmt_decimal);
    let held = held_ms.map_or_else(String::new, |ms| format!("open {}", fmt_duration_ms(ms)));
    draw_row_lines(
        painter,
        rect,
        RowLines {
            side: summary.side,
            head: format!(
                "{} {}",
                position_word(summary.side),
                fmt_decimal(summary.quantity)
            ),
            route: format!("{} → {}", fmt_decimal(summary.avg_price), exit),
            points: summary.open_points,
            detail: format!("{held} · at the last print"),
            // No stamp: the open position closes at a time nobody knows
            // yet, and a date on a trade still running would be a guess.
            stamp: None,
            tag: symbol.map(str::to_owned),
        },
    );
}

/// The two text lines every ledger row shares.
struct RowLines {
    side: Side,
    head: String,
    route: String,
    points: Option<Decimal>,
    detail: String,
    /// A stamp pinned to the right end of the detail line — the trade's
    /// date. It gets the space no other field wants (under the points),
    /// and anchoring it opposite the detail means the two can only ever
    /// collide in the middle, where the elision is visible.
    stamp: Option<String>,
    /// The instrument, riding the empty stretch of the head line between
    /// the round trip and the points. It sat on the detail line first, and
    /// the exit reason paid for it in elided characters — "stop …" is not
    /// a fact, and the room to say "stop loss" was free one line up.
    tag: Option<String>,
}

fn draw_row_lines(painter: &egui::Painter, rect: egui::Rect, lines: RowLines) {
    let font = egui::FontId::monospace(11.0);
    let x = rect.left() + SIDE_RAIL_WIDTH_PX + 6.0;
    let y1 = rect.top() + 9.0;
    let y2 = rect.top() + 24.0;
    let color = theme::side_color(lines.side);
    let head = painter.layout_no_wrap(lines.head, font.clone(), color);
    let head_w = head.size().x;
    painter.galley(egui::pos2(x, y1 - head.size().y / 2.0), head, color);
    painter.text(
        egui::pos2(x + head_w + 8.0, y1),
        egui::Align2::LEFT_CENTER,
        lines.route,
        font.clone(),
        theme::TEXT_MUTED,
    );
    let mut head_right = rect.right() - DETAIL_RIGHT_PAD_PX;
    if let Some(points) = lines.points {
        let galley = painter.layout_no_wrap(
            fmt_signed_points(points),
            font.clone(),
            points_color(points),
        );
        let width = galley.size().x;
        painter.galley(
            egui::pos2(head_right - width, y1 - galley.size().y / 2.0),
            galley,
            points_color(points),
        );
        head_right -= width + DETAIL_GAP_PX;
    }
    if let Some(tag) = lines.tag {
        let galley = painter.layout_no_wrap(tag, font.clone(), theme::TEXT_FAINT);
        painter.galley(
            egui::pos2(head_right - galley.size().x, y1 - galley.size().y / 2.0),
            galley,
            theme::TEXT_FAINT,
        );
    }
    // The detail line and its stamp share one row from opposite ends.
    let detail_font = egui::FontId::monospace(10.0);
    let mut detail_limit = rect.right() - DETAIL_RIGHT_PAD_PX - x;
    if let Some(stamp) = lines.stamp {
        let galley = painter.layout_no_wrap(stamp, detail_font.clone(), theme::TEXT_FAINT);
        let stamp_w = galley.size().x;
        painter.galley(
            egui::pos2(
                rect.right() - DETAIL_RIGHT_PAD_PX - stamp_w,
                y2 - galley.size().y / 2.0,
            ),
            galley,
            theme::TEXT_FAINT,
        );
        detail_limit -= stamp_w + DETAIL_GAP_PX;
    }
    // Monospace, so one measured glyph gives the budget for all of them.
    let glyph_w = painter
        .layout_no_wrap("0".to_owned(), detail_font.clone(), theme::TEXT_FAINT)
        .size()
        .x
        .max(1.0);
    let budget = (detail_limit / glyph_w).floor().max(0.0) as usize;
    painter.text(
        egui::pos2(x, y2),
        egui::Align2::LEFT_CENTER,
        elide_tail(&lines.detail, budget),
        detail_font,
        theme::TEXT_FAINT,
    );
}

/// Cut `text` to `budget` characters, marking the cut with `…` so a
/// shortened exit reason can never be read as a complete one. Below the
/// ellipsis plus one character there is nothing honest left to say, so the
/// text is dropped entirely rather than reduced to a lone `…`.
fn elide_tail(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget {
        return text.to_owned();
    }
    if budget < 2 {
        return String::new();
    }
    let mut out: String = text.chars().take(budget - 1).collect();
    out.push('…');
    out
}

/// One closed trade in the ledger. Session rows select on click and reveal
/// a jump-to-chart control on hover; rows from earlier sessions are
/// display-only — their tape is not the one on screen, so the chart has
/// nothing honest to point at.
fn draw_ledger_row(
    ui: &mut egui::Ui,
    trade: &ClosedTrade,
    symbol: Option<&str>,
    selected: bool,
    session: bool,
    tz: TzOffset,
) -> LedgerRowResponse {
    let width = ui.available_width();
    let sense = if session {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, LEDGER_ROW_HEIGHT_PX), sense);
    if !ui.is_rect_visible(rect) {
        return LedgerRowResponse::default();
    }
    if selected {
        ui.painter().rect_filled(
            rect,
            egui::Rounding::ZERO,
            theme::active_tint(theme::ACCENT),
        );
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, egui::Rounding::ZERO, theme::BORDER);
    }
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            rect.left_top(),
            egui::pos2(rect.left() + SIDE_RAIL_WIDTH_PX, rect.bottom()),
        ),
        egui::Rounding::ZERO,
        theme::side_color(trade.side),
    );
    let detail = ledger_detail(trade, tz);
    draw_row_lines(
        ui.painter(),
        rect,
        RowLines {
            side: trade.side,
            head: format!(
                "{} {}",
                position_word(trade.side),
                fmt_decimal(trade.quantity)
            ),
            route: format!(
                "{} → {}",
                fmt_decimal(trade.entry_price),
                fmt_decimal(trade.exit_price)
            ),
            points: Some(trade.pnl_points),
            detail,
            // Compact: the day header directly above carries the year.
            stamp: Some(CivilDate::from_ms(trade.closed_ms, tz).short()),
            tag: symbol.map(str::to_owned),
        },
    );
    let mut navigate = false;
    if session && response.hovered() {
        let nav_rect = egui::Rect::from_center_size(
            egui::pos2(rect.right() - 14.0, rect.top() + 24.0),
            egui::vec2(16.0, 14.0),
        );
        let nav = ui
            .interact(
                nav_rect,
                ui.id()
                    .with(("ledger_nav", trade.opened_ms, trade.closed_ms)),
                egui::Sense::click(),
            )
            .on_hover_text("center the chart on this trade");
        ui.painter().text(
            nav_rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::ARROW_UP_RIGHT,
            egui::FontId::proportional(11.0),
            if nav.hovered() {
                theme::TEXT_PRIMARY
            } else {
                theme::TEXT_MUTED
            },
        );
        navigate = nav.clicked();
    }
    LedgerRowResponse {
        clicked: response.clicked() && !navigate,
        navigate,
    }
}

/// The detail line under a ledger row: the closing clock, how long the
/// trade was held, and why it ended. The instrument rides the head line
/// (see `RowLines::tag`) and the date the right-hand stamp, so this line
/// spends every character it has on the reason. Pure, so a test can assert
/// exactly what a trader will read.
fn ledger_detail(trade: &ClosedTrade, tz: TzOffset) -> String {
    format!(
        "{} · {} · {}",
        crate::plot_area::fmt_time(trade.closed_ms, tz),
        fmt_duration_ms(trade.closed_ms.saturating_sub(trade.opened_ms)),
        trade.exit_reason.as_str().replace('_', " "),
    )
}

#[cfg(test)]
mod tests {
    use quantick_sim::ExitReason;
    use rust_decimal::Decimal;

    use super::*;
    use crate::paper_trading::PaperTrading;

    /// A closed trade that netted `pnl` points, closing at `closed_ms`.
    fn trade_at(closed_ms: i64, pnl: i64) -> ClosedTrade {
        ClosedTrade {
            side: Side::Buy,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100 + pnl),
            opened_ms: closed_ms - 1000,
            closed_ms,
            pnl_points: Decimal::from(pnl),
            exit_reason: ExitReason::Manual,
            entry_agg_id: None,
            exit_agg_id: None,
            mae_points: None,
            mfe_points: None,
        }
    }

    fn row(symbol: &str, source: Option<history::SessionSource>, trade: ClosedTrade) -> HistoryRow {
        HistoryRow {
            symbol: symbol.to_owned(),
            source,
            trade,
        }
    }

    #[test]
    fn the_report_opens_on_real_and_keeps_replay_out_until_asked() {
        let utc = TzOffset::new(0);
        let day = 86_400_000_i64;
        let mut paper = PaperTrading::new();
        assert_eq!(
            paper.report_state().report_source,
            SourceFilter::Real,
            "practice runs never inflate the real track record unasked"
        );
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![
                row("X", Some(history::SessionSource::Live), trade_at(day, 5)),
                row("X", None, trade_at(2 * day, 3)),
                row(
                    "X",
                    Some(history::SessionSource::Replay),
                    trade_at(3 * day, 100),
                ),
            ],
            files: 3,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_state_mut().ensure_report_view(utc);
        let view = paper
            .report_state()
            .report_view
            .as_ref()
            .expect("view built");
        assert_eq!(view.rows.len(), 2, "live + unrecorded-legacy count as real");
        assert_eq!(
            view.hidden_by_source, 1,
            "the replay trade sits behind the filter"
        );
        assert_eq!(
            view.anchor_ms,
            Some(2 * day),
            "the anchor comes from the filtered scope, not the replay trade"
        );
        assert_eq!(view.report.net_points, Decimal::from(8));

        paper.report_state_mut().report_source = SourceFilter::Replay;
        paper.report_state_mut().ensure_report_view(utc);
        let view = paper.report_state().report_view.as_ref().expect("rebuilt");
        assert_eq!(view.rows.len(), 1, "the practice run, alone");
        assert_eq!(view.report.net_points, Decimal::from(100));
        assert_eq!(view.hidden_by_source, 2);

        paper.report_state_mut().report_source = SourceFilter::All;
        paper.report_state_mut().ensure_report_view(utc);
        let view = paper.report_state().report_view.as_ref().expect("rebuilt");
        assert_eq!(view.rows.len(), 3, "All mixes on purpose");
        assert_eq!(view.hidden_by_source, 0);
    }

    #[test]
    fn report_periods_anchor_to_the_given_trade_never_a_clock() {
        let utc = TzOffset::new(0);
        // 2026-03-16 13:01:08 UTC.
        let anchor = 1_773_666_068_000_i64;
        assert_eq!(ReportPeriod::All.cutoff_ms(anchor, utc), None);
        assert_eq!(
            ReportPeriod::Today.cutoff_ms(anchor, utc),
            Some(1_773_619_200_000),
            "the anchor's own UTC midnight"
        );
        assert_eq!(
            ReportPeriod::Week.cutoff_ms(anchor, utc),
            Some(anchor - 7 * 86_400_000)
        );
        assert_eq!(
            ReportPeriod::Custom(2 * 86_400_000).cutoff_ms(anchor, utc),
            Some(anchor - 2 * 86_400_000),
            "a typed 2d reaches back exactly two days"
        );
    }

    #[test]
    fn today_breaks_at_the_displayed_midnight_not_utc() {
        let day = 86_400_000_i64;
        // 03 Jan 01:00 UTC is 02 Jan 22:00 in UTC-03:00.
        let anchor = 2 * day + 3_600_000;
        let sao_paulo = TzOffset::new(-180);
        assert_eq!(
            ReportPeriod::Today.cutoff_ms(anchor, sao_paulo),
            Some(day + 10_800_000),
            "midnight of the anchor's displayed day, expressed in UTC"
        );
        assert_eq!(
            ReportPeriod::Today.cutoff_ms(anchor, TzOffset::new(0)),
            Some(2 * day),
            "UTC viewers keep the UTC midnight"
        );
    }

    #[test]
    fn typed_periods_parse_strictly_and_format_back() {
        assert_eq!(parse_period("2d"), Some(2 * 86_400_000));
        assert_eq!(parse_period(" 3D "), Some(3 * 86_400_000));
        assert_eq!(parse_period("12h"), Some(12 * 3_600_000));
        assert_eq!(parse_period("45m"), Some(45 * 60_000));
        assert_eq!(parse_period("1w"), Some(7 * 86_400_000));
        for refused in ["", "d", "2", "2x", "0d", "-2d", "2.5d", "d2"] {
            assert_eq!(parse_period(refused), None, "{refused:?} must be refused");
        }
        assert_eq!(fmt_period_ms(2 * 86_400_000), "2d");
        assert_eq!(fmt_period_ms(36 * 3_600_000), "36h");
        assert_eq!(fmt_period_ms(45 * 60_000), "45m");
        assert_eq!(fmt_period_ms(14 * 86_400_000), "2w");
    }

    /// The report answers as data, not only as pixels: the snapshot names
    /// the window in force and hands back the very rows the tiles, the
    /// curve and the list were computed from. An operator that cannot see
    /// the screen reads this.
    #[test]
    fn the_report_reports_itself_as_data() {
        let tz = TzOffset::new(-180);
        let day = CivilDate::from_ymd(2026, 8, 17);
        let mut paper = PaperTrading::new();
        assert!(
            paper.report_state().snapshot().is_none(),
            "nothing loaded, nothing to report"
        );
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![
                row("WINV26", None, trade_at(day.start_ms(tz) + 60_000, 5)),
                row(
                    "WINV26",
                    Some(history::SessionSource::Replay),
                    trade_at(day.start_ms(tz) + 120_000, 9),
                ),
                row(
                    "WINV26",
                    None,
                    trade_at(day.offset_days(1).start_ms(tz) + 60_000, -3),
                ),
            ],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_state_mut().report_symbol = Some("WINV26".to_owned());

        // The pills in force, no dates picked.
        paper.report_state_mut().report_period = ReportPeriod::All;
        paper.report_state_mut().ensure_report_view(tz);
        let snapshot = paper.report_state().snapshot().expect("a snapshot");
        assert_eq!(snapshot.symbol, Some("WINV26"));
        assert_eq!(snapshot.source, SourceFilter::Real);
        assert_eq!(snapshot.window, ReportWindow::Period(ReportPeriod::All));
        assert_eq!(snapshot.rows.len(), 2, "the practice run is filtered out");
        assert_eq!(snapshot.hidden_by_source, 1);
        assert_eq!(snapshot.report.net_points, Decimal::from(2));
        assert_eq!(snapshot.window.label(), "everything saved");

        // The named action a script would call, then the same read-back.
        paper
            .report_state_mut()
            .pick_report_dates(DaySelection::None.click(day));
        paper.report_state_mut().ensure_report_view(tz);
        let snapshot = paper.report_state().snapshot().expect("a snapshot");
        assert_eq!(
            snapshot.window,
            ReportWindow::Dates(DateRange {
                start: day,
                end: day
            }),
            "a range and a period are never both in force"
        );
        assert_eq!(snapshot.rows.len(), 1);
        assert_eq!(snapshot.rows[0].symbol, "WINV26");
        assert_eq!(snapshot.hidden_outside, 1, "the next day sits outside");
        assert_eq!(snapshot.window.label(), day.iso(), "and it names itself");
        // The numbers the tiles show come from the very rows handed back.
        let net: Decimal = snapshot
            .rows
            .iter()
            .map(|row| row.trade.pnl_points)
            .sum::<Decimal>();
        assert_eq!(snapshot.report.net_points, net);
    }

    /// The equity walk is cut with the view, not re-walked per frame, so
    /// the curve above the list and the running total beside each trade
    /// are literally the same numbers.
    #[test]
    fn the_equity_walk_is_cut_once_and_both_readers_share_it() {
        let rows = vec![
            row("X", None, trade_at(1_000, 5)),
            row("X", None, trade_at(2_000, -12)),
            row("X", None, trade_at(3_000, 4)),
        ];
        let walk = EquityWalk::of(&rows);
        assert_eq!(walk.points.len(), rows.len() + 1, "E_0 rides in front");
        assert_eq!(walk.points[0], Decimal::ZERO, "flat before the first trade");
        assert_eq!(walk.points[1], Decimal::from(5));
        assert_eq!(walk.points[2], Decimal::from(-7));
        assert_eq!(walk.points[3], Decimal::from(-3));
        assert_eq!(walk.plot.len(), walk.points.len());
        assert!((walk.low - (-7.0)).abs() < f32::EPSILON, "the trough");
        assert!((walk.high - 5.0).abs() < f32::EPSILON, "the peak");
        // A view with no trades still has a walk, and it is the flat one.
        let empty = EquityWalk::of(&[]);
        assert_eq!(empty.points, vec![Decimal::ZERO]);
        assert_eq!((empty.low, empty.high), (0.0, 0.0));
    }

    /// The ledger lists the chart's instrument, one the trader names, or
    /// all of them — and the picker's label always says which.
    #[test]
    fn the_ledger_lists_the_chart_a_named_market_or_all_of_them() {
        assert_eq!(LedgerScope::Chart.folder("BTCUSDT"), Some("BTCUSDT"));
        assert_eq!(
            LedgerScope::Symbol("WINV26".to_owned()).folder("BTCUSDT"),
            Some("WINV26"),
            "a named market does not follow the chart"
        );
        assert_eq!(LedgerScope::All.folder("BTCUSDT"), None, "the whole folder");
        assert_eq!(LedgerScope::Chart.label("BTCUSDT"), "This chart · BTCUSDT");
        assert_eq!(
            LedgerScope::Chart.label(""),
            "This chart",
            "before a feed settles there is no market to name"
        );
        assert_eq!(
            LedgerScope::Symbol("WINV26".to_owned()).label("BTCUSDT"),
            "WINV26"
        );
        assert_eq!(LedgerScope::All.label("BTCUSDT"), "All symbols");
    }

    /// Folding is per civil day and reversible, and the "fold everything"
    /// control reports honestly whether it has anything left to fold.
    #[test]
    fn days_fold_shut_one_at_a_time_or_all_at_once() {
        let tz = TzOffset::new(-180);
        let day = CivilDate::from_ymd(2026, 8, 17);
        let mut paper = PaperTrading::new();
        paper.report_state_mut().ledger_tz = tz;
        paper.report_state_mut().history_cache = Some(LoadedHistory {
            rows: vec![
                row("X", None, trade_at(day.start_ms(tz) + 60_000, 5)),
                row("X", None, trade_at(day.end_ms(tz) - 1, -2)),
                row("X", None, trade_at(day.offset_days(1).start_ms(tz), 7)),
            ],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        assert!(
            !{
                let (state, env) = paper.report_parts();
                state.all_days_collapsed(&env)
            },
            "nothing is folded to begin with"
        );

        paper.report_state_mut().set_day_collapsed(day, true);
        assert!(
            paper
                .report_state()
                .collapsed_days
                .contains(&day.day_number())
        );
        assert!(
            !{
                let (state, env) = paper.report_parts();
                state.all_days_collapsed(&env)
            },
            "the next day is still open, so the control still offers to fold"
        );

        {
            let (state, env) = paper.report_parts();
            state.toggle_all_days(false, &env)
        };
        assert!(
            {
                let (state, env) = paper.report_parts();
                state.all_days_collapsed(&env)
            },
            "both days shut"
        );
        assert_eq!(paper.report_state().collapsed_days.len(), 2);

        {
            let (state, env) = paper.report_parts();
            state.toggle_all_days(true, &env)
        };
        assert!(paper.report_state().collapsed_days.is_empty());
        assert!(!{
            let (state, env) = paper.report_parts();
            state.all_days_collapsed(&env)
        });

        // An empty ledger has nothing folded *and* nothing to fold: the
        // control must not claim everything is already shut.
        let mut empty = PaperTrading::new();
        assert!(!{
            let (state, env) = empty.report_parts();
            state.all_days_collapsed(&env)
        });
    }

    /// The rows the ledger builds each frame are bounded by the revealed
    /// page, and so is every other per-frame pass it makes. A trader with
    /// a year of sessions must pay the same per-frame cost as one with a
    /// week — which means the totals strip cannot walk the history either.
    #[test]
    fn the_ledger_builds_a_bounded_number_of_rows_however_deep_the_history() {
        let tz = TzOffset::new(-180);
        let day = CivilDate::from_ymd(2026, 8, 17);
        // Five thousand saved trades over fifty days.
        let saved: Vec<ClosedTrade> = (0..5_000)
            .map(|index| {
                trade_at(
                    day.offset_days(-(index / 100)).start_ms(tz) + (index % 100) * 60_000,
                    1,
                )
            })
            .collect();
        let items: Vec<(&str, &ClosedTrade)> =
            saved.iter().map(|trade| ("WINV26", trade)).collect();

        let page = LedgerPage::of(items.len(), 1);
        assert_eq!(page.shown, LEDGER_PAGE_TRADES);
        assert_eq!(page.remaining, 4_950, "and the control says so out loud");
        let mut rows = Vec::new();
        push_by_day(
            &mut rows,
            &items[..page.shown],
            tz,
            &std::collections::BTreeSet::new(),
            |item| item.1,
            |item| LedgerRow::Earlier(item.0, item.1),
        );
        // Fifty trades plus at most one day caption each — nowhere near
        // the five thousand rows the pre-paging ledger would have built.
        assert!(
            rows.len() <= 2 * LEDGER_PAGE_TRADES,
            "{} rows for a page of {LEDGER_PAGE_TRADES}",
            rows.len()
        );
        // Revealing pages grows the list by one page at a time, never all
        // at once.
        let deeper = LedgerPage::of(items.len(), 2);
        assert_eq!(deeper.shown - page.shown, LEDGER_PAGE_TRADES);

        // And the strip under the list is summed with the load, not on the
        // frame: the totals are a stored value, so the frame reads them
        // rather than walking five thousand trades to print one line.
        let totals = LedgerTotals::of(saved.iter());
        assert_eq!(totals.trades, 5_000);
        assert_eq!(totals.wins, 5_000, "every fixture trade is a winner");
        assert_eq!(totals.net, Decimal::from(5_000));
        assert_eq!(totals.win_rate(), Some(100));
        // Nothing saved plus this session's own trades still adds up.
        assert_eq!(
            LedgerTotals::default().plus(totals),
            totals,
            "an empty half must be the identity"
        );
        assert_eq!(LedgerTotals::default().win_rate(), None, "0/0 is not 0%");
    }

    /// Everything a trader must be able to read off one ledger row: which
    /// market, when on the clock, how long, and why it ended — plus the
    /// date, which rides the row's right-hand stamp rather than the detail
    /// line so the two can only ever collide where the elision shows.
    #[test]
    fn a_ledger_row_names_its_market_its_clock_its_age_and_its_ending() {
        let utc = TzOffset::new(0);
        let mut trade = trade_at(1_773_666_068_000, -25);
        trade.opened_ms = trade.closed_ms - 246_000;
        trade.exit_reason = quantick_sim::ExitReason::TakeProfit;
        // The detail line spends every character it has on the reason:
        // the instrument rides the head line and the date the right-hand
        // stamp, precisely so "take profit" is never cut to "take prof…".
        assert_eq!(
            ledger_detail(&trade, utc),
            "13:01:08 · 4m 06s · take profit"
        );
        assert_eq!(
            CivilDate::from_ms(trade.closed_ms, utc).short(),
            "16 Mar",
            "the stamp opposite the detail carries the date"
        );
        assert_eq!(
            CivilDate::from_ms(trade.closed_ms, utc).long(),
            "Mon 16 Mar 2026",
            "and the day header above it carries the year"
        );
        // The display timezone moves the clock and the stamp together.
        assert_eq!(
            ledger_detail(&trade, TzOffset::new(-180)),
            "10:01:08 · 4m 06s · take profit"
        );
        assert_eq!(
            CivilDate::from_ms(trade.closed_ms, TzOffset::new(-180)).short(),
            "16 Mar"
        );
    }

    /// A detail line that outgrows its share of the row is cut with an
    /// ellipsis, never clipped mid-glyph: a shortened "take prof" must not
    /// be readable as a complete exit reason.
    #[test]
    fn a_detail_line_too_long_for_its_row_is_elided_not_clipped() {
        assert_eq!(elide_tail("take profit", 11), "take profit");
        assert_eq!(elide_tail("take profit", 12), "take profit");
        assert_eq!(elide_tail("take profit", 6), "take …");
        assert_eq!(elide_tail("take profit", 2), "t…");
        // Below the ellipsis plus a character there is nothing honest left
        // to say, so the line says nothing.
        assert_eq!(elide_tail("take profit", 1), "");
        assert_eq!(elide_tail("take profit", 0), "");
        // Multi-byte characters are counted as characters, not bytes.
        assert_eq!(elide_tail("WINV26 · 13:01", 8), "WINV26 …");
    }

    /// The ledger reveals saved history one page at a time and states how
    /// much it is holding back — a list that simply ends looks like the
    /// end of the history, which is the confusion the control exists for.
    #[test]
    fn the_ledger_reveals_saved_history_one_page_at_a_time() {
        let page = LedgerPage::of(120, 1);
        assert_eq!(page.shown, LEDGER_PAGE_TRADES);
        assert_eq!(page.remaining, 120 - LEDGER_PAGE_TRADES);
        let page = LedgerPage::of(120, 2);
        assert_eq!(page.shown, 2 * LEDGER_PAGE_TRADES);
        assert_eq!(page.remaining, 120 - 2 * LEDGER_PAGE_TRADES);
        // The last page shows the tail and offers nothing more.
        let page = LedgerPage::of(120, 3);
        assert_eq!(page.shown, 120);
        assert_eq!(page.remaining, 0);
        let page = LedgerPage::of(120, 99);
        assert_eq!(page.shown, 120, "extra pages cannot invent trades");
        assert_eq!(page.remaining, 0);
        // A short history fits in one page and never offers "show older".
        let page = LedgerPage::of(7, 1);
        assert_eq!((page.shown, page.remaining), (7, 0));
        // Page zero is treated as one: the ledger always shows something.
        assert_eq!(LedgerPage::of(7, 0), LedgerPage::of(7, 1));
        assert_eq!(
            LedgerPage::of(0, 1),
            LedgerPage {
                shown: 0,
                remaining: 0
            }
        );
    }

    /// Rows are grouped under the civil day they closed on, and each day's
    /// caption carries that day's own count and net.
    #[test]
    fn ledger_rows_open_a_new_day_caption_when_the_day_changes() {
        let tz = TzOffset::new(-180);
        let day = CivilDate::from_ymd(2026, 8, 17);
        // Newest first, the order the ledger cuts in.
        let items = [
            (
                "WINV26",
                trade_at(day.offset_days(1).start_ms(tz) + 3_600_000, 8),
            ),
            ("WINV26", trade_at(day.end_ms(tz) - 1, -25)),
            ("WINV26", trade_at(day.start_ms(tz) + 60_000, 139)),
        ];
        let items: Vec<(&str, &ClosedTrade)> = items
            .iter()
            .map(|(symbol, trade)| (*symbol, trade))
            .collect();
        let mut rows = Vec::new();
        push_by_day(
            &mut rows,
            &items,
            tz,
            &std::collections::BTreeSet::new(),
            |item| item.1,
            |item| LedgerRow::Earlier(item.0, item.1),
        );
        assert_eq!(rows.len(), 5, "two day captions over three trades");
        match rows[0] {
            LedgerRow::Day(date, count, net, folded) => {
                assert_eq!(date, day.offset_days(1));
                assert_eq!(count, 1);
                assert_eq!(net, Decimal::from(8));
                assert!(!folded);
            }
            _ => panic!("the list opens on a day caption"),
        }
        assert!(matches!(rows[1], LedgerRow::Earlier(..)));
        match rows[2] {
            LedgerRow::Day(date, count, net, _) => {
                assert_eq!(date, day);
                assert_eq!(count, 2, "both of the 17th's trades");
                assert_eq!(net, Decimal::from(114), "and what the day netted");
            }
            _ => panic!("the next day opens its own caption"),
        }
        assert!(matches!(rows[3], LedgerRow::Earlier(..)));
        assert!(matches!(rows[4], LedgerRow::Earlier(..)));

        // Folded, the 17th keeps its caption — with its count and its net
        // intact — and contributes no trade rows at all.
        let mut folded_days = std::collections::BTreeSet::new();
        folded_days.insert(day.day_number());
        let mut rows = Vec::new();
        push_by_day(
            &mut rows,
            &items,
            tz,
            &folded_days,
            |item| item.1,
            |item| LedgerRow::Earlier(item.0, item.1),
        );
        assert_eq!(rows.len(), 3, "two captions and only the open day's row");
        match rows[2] {
            LedgerRow::Day(date, count, net, folded) => {
                assert_eq!(date, day);
                assert!(folded, "and it says it is folded");
                assert_eq!(count, 2, "a folded day still counts its trades");
                assert_eq!(net, Decimal::from(114), "and still states its net");
            }
            _ => panic!("a folded day keeps its caption"),
        }
    }

    /// A picked day cuts the report to that civil day, and a picked span
    /// to both its ends inclusive. The pills stand down while it holds, so
    /// a chosen date can never come back empty because a forgotten pill
    /// was cutting too.
    #[test]
    fn a_picked_calendar_range_cuts_the_report_and_the_pills_stand_down() {
        let tz = TzOffset::new(-180);
        let first = CivilDate::from_ymd(2026, 8, 12);
        let mut paper = PaperTrading::new();
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![
                row("X", None, trade_at(first.start_ms(tz) + 3_600_000, 5)),
                row(
                    "X",
                    None,
                    trade_at(first.offset_days(3).start_ms(tz) + 60_000, -2),
                ),
                // The last millisecond of the 17th, local — inside a range
                // that ends on the 17th.
                row("X", None, trade_at(first.offset_days(5).end_ms(tz) - 1, 7)),
                row("X", None, trade_at(first.offset_days(6).start_ms(tz), 11)),
            ],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        // A pill that would hide the older trades is deliberately left on:
        // the range must win outright, not intersect.
        paper.report_state_mut().report_period = ReportPeriod::Today;

        let picked = paper.report_state().calendar.selection.click(first);
        paper.report_state_mut().pick_report_dates(picked);
        paper.report_state_mut().ensure_report_view(tz);
        let view = paper.report_state().report_view.as_ref().expect("a view");
        assert_eq!(view.rows.len(), 1, "one day, one trade");
        assert_eq!(
            view.range.map(DateRange::label),
            Some("2026-08-12".to_owned())
        );
        assert_eq!(view.hidden_outside, 3, "and it counts what it hides");
        assert_eq!(view.report.net_points, Decimal::from(5), "the tiles agree");

        let picked = paper
            .report_state()
            .calendar
            .selection
            .click(first.offset_days(5));
        paper.report_state_mut().pick_report_dates(picked);
        paper.report_state_mut().ensure_report_view(tz);
        let view = paper.report_state().report_view.as_ref().expect("a view");
        assert_eq!(view.rows.len(), 3, "both ends of the span are inside");
        assert_eq!(
            view.range.map(DateRange::label),
            Some("2026-08-12 to 2026-08-17".to_owned())
        );
        assert_eq!(view.hidden_outside, 1, "only the 18th sits outside");
        assert_eq!(view.report.net_points, Decimal::from(10));

        // Clearing hands the window back to the pills, unchanged.
        paper
            .report_state_mut()
            .pick_report_dates(DaySelection::None);
        paper.report_state_mut().report_period = ReportPeriod::All;
        paper.report_state_mut().ensure_report_view(tz);
        let view = paper.report_state().report_view.as_ref().expect("a view");
        assert!(view.range.is_none());
        assert_eq!(view.rows.len(), 4, "every saved trade is back");
        assert_eq!(view.hidden_outside, 0);
    }

    /// A day the trader picked that holds nothing is answered, not hidden:
    /// the view is empty, the range still names itself, and every saved
    /// trade is counted as sitting outside it.
    #[test]
    fn a_picked_day_with_no_trades_reports_an_honest_empty() {
        let tz = TzOffset::new(0);
        let day = CivilDate::from_ymd(2026, 8, 12);
        let mut paper = PaperTrading::new();
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![row("X", None, trade_at(day.offset_days(2).start_ms(tz), 5))],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        let picked = paper.report_state().calendar.selection.click(day);
        paper.report_state_mut().pick_report_dates(picked);
        paper.report_state_mut().ensure_report_view(tz);
        let view = paper.report_state().report_view.as_ref().expect("a view");
        assert!(view.rows.is_empty());
        assert_eq!(
            view.range.map(DateRange::label),
            Some("2026-08-12".to_owned())
        );
        assert_eq!(view.hidden_outside, 1, "the trade exists, just not here");
    }

    /// The calendar highlights days from the same trades the report would
    /// show — after the Source filter, on the display timezone's clock.
    #[test]
    fn the_day_index_follows_the_source_filter_and_the_timezone() {
        let tz = TzOffset::new(-180);
        let day = CivilDate::from_ymd(2026, 8, 17);
        let mut paper = PaperTrading::new();
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![
                row(
                    "X",
                    Some(history::SessionSource::Live),
                    trade_at(day.start_ms(tz) + 60_000, 5),
                ),
                row(
                    "X",
                    Some(history::SessionSource::Replay),
                    trade_at(day.offset_days(1).start_ms(tz) + 60_000, 9),
                ),
            ],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_state_mut().ensure_report_view(tz);
        assert_eq!(
            paper.report_state().report_days.len(),
            1,
            "Real hides the practice day"
        );
        assert!(paper.report_state().report_days.stat(day).is_some());
        assert!(
            paper
                .report_state()
                .report_days
                .stat(day.offset_days(1))
                .is_none()
        );

        paper.report_state_mut().report_source = SourceFilter::All;
        paper.report_state_mut().ensure_report_view(tz);
        assert_eq!(
            paper.report_state().report_days.len(),
            2,
            "Both lights up both days"
        );

        // The same trades, read on another clock, land on other days.
        paper
            .report_state_mut()
            .ensure_report_view(TzOffset::new(0));
        assert_eq!(paper.report_state().report_days.len(), 2);
        assert!(
            paper
                .report_state()
                .report_days
                .stat(CivilDate::from_ymd(2026, 8, 17))
                .is_some(),
            "03:01 UTC on the 17th is still the 17th in UTC"
        );
    }

    /// The expected report, byte for byte. Regenerated only when a
    /// *behaviour* change is intended and argued for.
    const GOLDEN_REPORT: &str = r#"== ALL ==
rows: 8
hidden_outside: 0
hidden_by_source: 1
anchor_ms: Some(1728000000)
equity.points: [0, 12, 7, 4, 4, 13, 5, 9, 15]
equity.plot: [0.0, 12.0, 7.0, 4.0, 4.0, 13.0, 5.0, 9.0, 15.0]
equity.low: 0.0  equity.high: 15.0
  report: PerformanceReport {
      trades: 8,
      wins: 4,
      losses: 3,
      scratches: 1,
      long_trades: 4,
      short_trades: 4,
      net_points: 15,
      gross_profit: 31,
      gross_loss: 16,
      win_rate_pct: Some(
          50,
      ),
      loss_rate_pct: Some(
          37.50,
      ),
      profit_factor: Some(
          1.9375,
      ),
      payoff_ratio: Some(
          1.453125000000000000000,
      ),
      expectancy_points: Some(
          1.875,
      ),
      max_drawdown_points: 8,
      max_drawdown_span: Some(
          (
              1,
              3,
          ),
      ),
      max_runup_points: 15,
      recovery_factor: Some(
          1.875,
      ),
      max_consecutive_wins: 2,
      max_consecutive_losses: 2,
      stddev_points: Some(
          7.0394,
      ),
      largest_win: 12,
      largest_loss: 8,
      avg_win: Some(
          7.75,
      ),
      avg_loss: Some(
          5.3333333333333333333333333333,
      ),
      avg_duration_ms: Some(
          2700000,
      ),
      median_duration_ms: Some(
          2100000,
      ),
      avg_win_duration_ms: Some(
          4500000,
      ),
      avg_loss_duration_ms: Some(
          1000000,
      ),
      avg_winner_mae_points: Some(
          2.50,
      ),
      winners_with_mae: 4,
      avg_loser_mfe_points: Some(
          1.50,
      ),
      losers_with_mfe: 2,
      long: SideReport {
          trades: 4,
          net_points: 3,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              1.2307692307692307692307692308,
          ),
          avg_win: Some(
              8,
          ),
          avg_loss: Some(
              6.50,
          ),
          expectancy_points: Some(
              0.75,
          ),
      },
      short: SideReport {
          trades: 4,
          net_points: 12,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              5,
          ),
          avg_win: Some(
              7.50,
          ),
          avg_loss: Some(
              3,
          ),
          expectancy_points: Some(
              3,
          ),
      },
      by_exit_reason: [
          ReasonReport {
              reason: StopLoss,
              trades: 3,
              net_points: -16,
          },
          ReasonReport {
              reason: TakeProfit,
              trades: 3,
              net_points: 27,
          },
          ReasonReport {
              reason: Manual,
              trades: 2,
              net_points: 4,
          },
      ],
  }
== WEEK ==
rows: 5
hidden_outside: 3
hidden_by_source: 1
anchor_ms: Some(1728000000)
equity.points: [0, 0, 9, 1, 5, 11]
equity.plot: [0.0, 0.0, 9.0, 1.0, 5.0, 11.0]
equity.low: 0.0  equity.high: 11.0
  report: PerformanceReport {
      trades: 5,
      wins: 3,
      losses: 1,
      scratches: 1,
      long_trades: 2,
      short_trades: 3,
      net_points: 11,
      gross_profit: 19,
      gross_loss: 8,
      win_rate_pct: Some(
          60,
      ),
      loss_rate_pct: Some(
          20,
      ),
      profit_factor: Some(
          2.375,
      ),
      payoff_ratio: Some(
          0.7916666666666666666666666667,
      ),
      expectancy_points: Some(
          2.20,
      ),
      max_drawdown_points: 8,
      max_drawdown_span: Some(
          (
              2,
              3,
          ),
      ),
      max_runup_points: 11,
      recovery_factor: Some(
          1.375,
      ),
      max_consecutive_wins: 2,
      max_consecutive_losses: 1,
      stddev_points: Some(
          6.5727,
      ),
      largest_win: 9,
      largest_loss: 8,
      avg_win: Some(
          6.3333333333333333333333333333,
      ),
      avg_loss: Some(
          8,
      ),
      avg_duration_ms: Some(
          3060000,
      ),
      median_duration_ms: Some(
          2400000,
      ),
      avg_win_duration_ms: Some(
          4800000,
      ),
      avg_loss_duration_ms: Some(
          300000,
      ),
      avg_winner_mae_points: Some(
          2,
      ),
      winners_with_mae: 3,
      avg_loser_mfe_points: None,
      losers_with_mfe: 0,
      long: SideReport {
          trades: 2,
          net_points: -4,
          win_rate_pct: Some(
              50,
          ),
          profit_factor: Some(
              0.50,
          ),
          avg_win: Some(
              4,
          ),
          avg_loss: Some(
              8,
          ),
          expectancy_points: Some(
              -2,
          ),
      },
      short: SideReport {
          trades: 3,
          net_points: 15,
          win_rate_pct: Some(
              66.666666666666666666666666667,
          ),
          profit_factor: None,
          avg_win: Some(
              7.50,
          ),
          avg_loss: None,
          expectancy_points: Some(
              5,
          ),
      },
      by_exit_reason: [
          ReasonReport {
              reason: StopLoss,
              trades: 1,
              net_points: -8,
          },
          ReasonReport {
              reason: TakeProfit,
              trades: 2,
              net_points: 15,
          },
          ReasonReport {
              reason: Manual,
              trades: 2,
              net_points: 4,
          },
      ],
  }
"#;

    /// The fixture the golden report is computed from: nine closed trades
    /// over eleven days, chosen so that every branch of the arithmetic has
    /// something to say. Both sides trade; there are wins, losses and a
    /// scratch; the equity curve draws down and runs up again; two exit
    /// reasons appear and three do not; and two rows carry no MAE/MFE, the
    /// way a version-1 history file loads, so the disclosed denominators
    /// are not simply the trade count.
    ///
    /// It is fixed on purpose. The numbers below are the trader's own
    /// results as this repository computes them, and a move that shifts one
    /// of them is the code telling them a lie about their trading. Nothing
    /// here may be edited to make a failing assertion pass: a change to this
    /// fixture invalidates the golden, and a change to the golden has to be
    /// argued for as a change in behaviour.
    fn golden_history() -> LoadedHistory {
        let day = 86_400_000_i64;
        let t = |closed_ms: i64,
                 side: Side,
                 pnl: i64,
                 held_ms: i64,
                 reason: quantick_sim::ExitReason,
                 excursions: Option<(i64, i64)>| ClosedTrade {
            side,
            quantity: Decimal::ONE,
            entry_price: Decimal::from(100),
            exit_price: Decimal::from(100 + pnl),
            opened_ms: closed_ms - held_ms,
            closed_ms,
            pnl_points: Decimal::from(pnl),
            exit_reason: reason,
            entry_agg_id: None,
            exit_agg_id: None,
            mae_points: excursions.map(|(mae, _)| Decimal::from(mae)),
            mfe_points: excursions.map(|(_, mfe)| Decimal::from(mfe)),
        };
        use quantick_sim::ExitReason::{Manual, StopLoss, TakeProfit};
        LoadedHistory {
            rows: vec![
                row(
                    "AAAUSDT",
                    Some(history::SessionSource::Live),
                    t(
                        10 * day,
                        Side::Buy,
                        12,
                        3_600_000,
                        TakeProfit,
                        Some((4, 15)),
                    ),
                ),
                row(
                    "AAAUSDT",
                    Some(history::SessionSource::Live),
                    t(11 * day, Side::Buy, -5, 900_000, StopLoss, Some((7, 2))),
                ),
                row(
                    "AAAUSDT",
                    Some(history::SessionSource::Live),
                    t(12 * day, Side::Sell, -3, 1_800_000, StopLoss, Some((6, 1))),
                ),
                row(
                    "AAAUSDT",
                    None,
                    t(13 * day, Side::Sell, 0, 600_000, Manual, None),
                ),
                row(
                    "BBBUSDT",
                    Some(history::SessionSource::Live),
                    t(
                        14 * day,
                        Side::Sell,
                        9,
                        7_200_000,
                        TakeProfit,
                        Some((2, 11)),
                    ),
                ),
                row(
                    "BBBUSDT",
                    None,
                    t(15 * day, Side::Buy, -8, 300_000, StopLoss, None),
                ),
                // A practice run. The default Source filter keeps it out
                // of the real track record, so the golden covers that
                // filter too rather than assuming it never fires.
                row(
                    "BBBUSDT",
                    Some(history::SessionSource::Replay),
                    t(
                        16 * day,
                        Side::Buy,
                        40,
                        1_200_000,
                        TakeProfit,
                        Some((1, 44)),
                    ),
                ),
                row(
                    "BBBUSDT",
                    Some(history::SessionSource::Live),
                    t(19 * day, Side::Buy, 4, 2_400_000, Manual, Some((3, 6))),
                ),
                row(
                    "BBBUSDT",
                    Some(history::SessionSource::Live),
                    t(20 * day, Side::Sell, 6, 4_800_000, TakeProfit, Some((1, 8))),
                ),
            ],
            files: 3,
            unreadable_files: 0,
            problem_rows: 0,
        }
    }

    /// The report and its equity walk as one block of text, every field
    /// named. `{:#?}` over the report rather than a hand-written field list
    /// on purpose: a hand-written list can silently omit a metric, and the
    /// one metric nobody thought to write down is exactly the one a move
    /// would break unnoticed. The equity walk is dumped beside it because
    /// the curve and the trade list's running total read it, and it is
    /// computed here rather than in `quantick-sim`.
    fn dump_report(view: &ReportView) -> String {
        let mut out = String::new();
        out.push_str(&format!("rows: {}\n", view.rows.len()));
        out.push_str(&format!("hidden_outside: {}\n", view.hidden_outside));
        out.push_str(&format!("hidden_by_source: {}\n", view.hidden_by_source));
        out.push_str(&format!("anchor_ms: {:?}\n", view.anchor_ms));
        out.push_str(&format!("equity.points: {:?}\n", view.equity.points));
        out.push_str(&format!("equity.plot: {:?}\n", view.equity.plot));
        out.push_str(&format!(
            "equity.low: {:?}  equity.high: {:?}\n",
            view.equity.low, view.equity.high
        ));
        // Indented, and not merely for looks. `{:#?}` closes its braces in
        // column 0, and the size guard finds the end of a `#[cfg(test)]`
        // module by scanning for a line that is exactly `}` — so an
        // unindented dump inside the golden below walks that scan out of
        // the test module and scores three thousand lines of test code as
        // production. Two spaces keep the ratchet honest, and cost the
        // golden nothing: it is the same text, byte for byte.
        for line in format!("report: {:#?}", view.report).lines() {
            out.push_str("  ");
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    /// The numbers must not move.
    ///
    /// A fixed journal in, the whole report out, asserted byte for byte —
    /// across two cuts, so the filter arithmetic is under the golden too
    /// and not only `PerformanceReport::from_trades`. `All` covers the
    /// unfiltered aggregation; `Week` covers the anchor-relative cutoff,
    /// which is the part that lives in this crate rather than in
    /// `quantick-sim`.
    ///
    /// This test is written before the report moves out of this file and
    /// its expected text does not change when it does. That is the whole
    /// point: an extraction that alters a rounding, a filter boundary or an
    /// equity walk fails here rather than in front of the trader.
    #[test]
    fn the_report_numbers_are_fixed() {
        let utc = TzOffset::new(0);
        let mut paper = PaperTrading::new();
        paper.report_state_mut().report = Some(golden_history());

        paper.report_state_mut().report_period = ReportPeriod::All;
        paper.report_state_mut().ensure_report_view(utc);
        let all = dump_report(
            paper
                .report_state()
                .report_view
                .as_ref()
                .expect("the All cut"),
        );

        paper.report_state_mut().report_period = ReportPeriod::Week;
        paper.report_state_mut().ensure_report_view(utc);
        let week = dump_report(
            paper
                .report_state()
                .report_view
                .as_ref()
                .expect("the Week cut"),
        );

        assert_eq!(
            format!("== ALL ==\n{all}== WEEK ==\n{week}"),
            GOLDEN_REPORT,
            "the report's numbers moved"
        );
    }

    #[test]
    fn the_report_view_filters_by_period_from_the_newest_trade() {
        let utc = TzOffset::new(0);
        let day = 86_400_000_i64;
        let mut paper = PaperTrading::new();
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![
                row("X", None, trade_at(10 * day, 5)),
                row("X", None, trade_at(18 * day, -2)),
                row("X", None, trade_at(20 * day + 3_600_000, 7)),
            ],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_state_mut().report_period = ReportPeriod::Week;
        paper.report_state_mut().ensure_report_view(utc);
        let view = paper
            .report_state()
            .report_view
            .as_ref()
            .expect("view built");
        assert_eq!(
            view.anchor_ms,
            Some(20 * day + 3_600_000),
            "anchored to the newest saved trade, not a clock"
        );
        assert_eq!(view.rows.len(), 2, "the 10-day-old trade is outside 7d");
        assert_eq!(view.hidden_outside, 1, "and the view counts what it hides");
        assert_eq!(view.report.net_points, Decimal::from(5));

        paper.report_state_mut().report_period = ReportPeriod::All;
        paper.report_state_mut().ensure_report_view(utc);
        assert_eq!(
            paper
                .report_state()
                .report_view
                .as_ref()
                .expect("rebuilt")
                .rows
                .len(),
            3,
            "All sees everything again"
        );
    }

    #[test]
    fn all_symbols_hides_older_markets_but_says_so_and_scope_restores_them() {
        let utc = TzOffset::new(0);
        let day = 86_400_000_i64;
        let mut paper = PaperTrading::new();
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![
                row("OLDSYM", None, trade_at(day, 5)),
                row("NEWSYM", None, trade_at(60 * day, 7)),
            ],
            files: 2,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_state_mut().report_period = ReportPeriod::Month;
        paper.report_state_mut().ensure_report_view(utc);
        let view = paper
            .report_state()
            .report_view
            .as_ref()
            .expect("view built");
        assert_eq!(
            view.rows.len(),
            1,
            "30d back from the newest trade hides the older market entirely"
        );
        assert_eq!(view.hidden_outside, 1, "the support line can say so");

        // Narrowing the combo re-anchors on the old market's own newest
        // trade — the "my trades came back" behaviour, now spelled out.
        paper.report_state_mut().report = Some(LoadedHistory {
            rows: vec![row("OLDSYM", None, trade_at(day, 5))],
            files: 1,
            unreadable_files: 0,
            problem_rows: 0,
        });
        paper.report_state_mut().report_view = None;
        paper.report_state_mut().ensure_report_view(utc);
        let view = paper.report_state().report_view.as_ref().expect("rebuilt");
        assert_eq!(view.rows.len(), 1, "its own scope shows the old market");
        assert_eq!(view.hidden_outside, 0);
    }
    /// A refusal the report raises reaches the window's one toast.
    ///
    /// The typed-period field answers a value it cannot read with an
    /// acknowledgement - "a typed `2d` must never do nothing quietly". When
    /// the report lived on the host that was one call to `show_toast`; now
    /// it is an outbox, a `ReportResponse` and a host that has to forward
    /// it, which is three places for the message to die. It died in the
    /// first draft of exactly that seam, and no test in the suite noticed,
    /// so this is the test that would have.
    ///
    /// Asserted with the window *shut*, which is the harder half: the
    /// message is raised, the trader closes the window, and the refusal
    /// they earned must still arrive rather than leaving with the thing
    /// that raised it.
    #[test]
    fn a_refusal_the_report_raises_reaches_the_windows_one_toast() {
        let mut paper = PaperTrading::new();
        paper
            .report_state_mut()
            .show_toast("SIM: could not read `2x` as a period".to_owned());

        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            paper.draw_report_window(ctx, TzOffset::new(0));
        });

        assert_eq!(
            paper.take_toast().as_deref(),
            Some("SIM: could not read `2x` as a period"),
            "the report's refusal must reach the window's acknowledgement lane"
        );
        assert_eq!(
            paper.take_toast(),
            None,
            "and once only - the outbox is a slot that is handed over, not a copy"
        );
    }
}
