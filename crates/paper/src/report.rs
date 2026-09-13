//! The report's numbers: the journal read back, cut to a window, and walked.
//!
//! The reading half of the journal the account writes. [`load_history`]
//! reads every session file under the journal folder back into one
//! closing-order timeline; [`ReportView::cut`] filters it by session source
//! and by a period (measured back from the newest trade, never from a clock)
//! or by a picked span of civil days, aggregates it with
//! [`PerformanceReport::from_trades`] and walks its realized equity once.
//!
//! The window, the calendar grid, the trade list and every colour that shows
//! these numbers stay in `app`; what is here is what a backtest or an
//! operator with no window needs to get the same answer the window paints.

use std::path::{Path, PathBuf};

use quantick_sim::{ClosedTrade, PerformanceReport, history};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use crate::civil::{CivilDate, DAY_MS, DateRange, TzOffset};
use crate::format::sanitize_symbol;

/// One journal row loaded from disk: the trade, the symbol folder it came
/// from, and the session source its file recorded.
#[derive(Clone)]
pub struct HistoryRow {
    pub symbol: String,
    /// `None` — a file from before the source was recorded. The report's
    /// Real view includes it: that era *was* live trading, and hiding it
    /// would "lose" the user's history all over again.
    pub source: Option<history::SessionSource>,
    pub trade: ClosedTrade,
}

/// Journal rows loaded from disk, each remembering the symbol folder it
/// came from, merged into one closing-order timeline.
pub struct LoadedHistory {
    /// Rows in closing order across every file read.
    pub rows: Vec<HistoryRow>,
    pub files: usize,
    /// Files that were not readable quantick-trades files.
    pub unreadable_files: usize,
    /// Rows the parser had to report as unreadable (torn tails and such).
    pub problem_rows: usize,
}

/// The report's session-source filter. Default `Real`: practice runs must
/// never inflate the real track record unasked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFilter {
    /// Live sessions, plus files from before the source was recorded.
    Real,
    /// Replay-driven practice sessions only.
    Replay,
    /// Everything, mixed.
    All,
}

impl SourceFilter {
    /// Every filter, in pill order.
    pub const PILLS: [Self; 3] = [Self::Real, Self::Replay, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::Real => "Real",
            Self::Replay => "Replay",
            // Not "All": the period pills own that word on the same row,
            // and two identical pills a hand-width apart invite the wrong
            // click.
            Self::All => "Both",
        }
    }

    pub fn hover(self) -> &'static str {
        match self {
            Self::Real => {
                "live sessions - files saved before quantick recorded a source count as real"
            }
            Self::Replay => "practice sessions driven by a market-replay recording",
            Self::All => "live and replay together - mixed on purpose",
        }
    }

    /// Whether a row with this recorded source belongs to the filter.
    pub fn admits(self, source: Option<history::SessionSource>) -> bool {
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

/// Read every history file under `dir` (one symbol's folder, or all of
/// them), remembering each row's symbol and skipping every path in
/// `exclude` (the live session's own files — their trades are already in
/// the simulator). Missing folders are simply empty, not an error.
pub fn load_history(dir: &Path, symbol: Option<&str>, exclude: &[PathBuf]) -> LoadedHistory {
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

/// The report's period filter, measured back from the newest saved trade
/// in scope — never from a wall clock. The engine has no clock, and a
/// replayed session's trades may be years old; a wall-clock "7 days" would
/// report a perfectly good replay as empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportPeriod {
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
    pub const PILLS: [Self; 5] = [
        Self::Today,
        Self::Week,
        Self::Month,
        Self::Quarter,
        Self::All,
    ];

    pub fn label(self) -> &'static str {
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
    pub fn phrase(self) -> String {
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
    pub fn cutoff_ms(self, anchor_ms: i64, tz: TzOffset) -> Option<i64> {
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
pub fn parse_period(text: &str) -> Option<i64> {
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
pub fn fmt_period_ms(period_ms: i64) -> String {
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

/// The realized-equity walk `E_0..E_n` (`E_0 = 0` before the first trade),
/// in the two shapes its two readers need: exact points for the trade
/// list's running total, and plot-ready `f32` with its bounds for the
/// curve. Computed once when the view is cut — a window holding a year of
/// trades must not walk them again sixty times a second.
pub struct EquityWalk {
    /// `n + 1` exact running totals in points.
    pub points: Vec<Decimal>,
    /// The same walk as the curve plots it.
    pub plot: Vec<f32>,
    pub low: f32,
    pub high: f32,
}

impl EquityWalk {
    pub fn of(rows: &[HistoryRow]) -> Self {
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

/// The report as filtered for display: the period's trades in closing
/// order, their aggregation, and the anchor the period was measured from.
pub struct ReportView {
    pub period: ReportPeriod,
    pub source: SourceFilter,
    /// The calendar span in force. `Some` puts the report on absolute
    /// dates and takes the anchor-relative pills out of the cut; `None`
    /// leaves them in charge, which is exactly what the report did before
    /// a calendar existed.
    pub range: Option<DateRange>,
    /// The display timezone the view was cut with — "Today" moves with it,
    /// and so does which civil day a trade closed on.
    pub tz: TzOffset,
    /// Newest closing time in scope — what the period counts back from.
    pub anchor_ms: Option<i64>,
    /// Saved trades the window keeps out — the honest answer to "where did
    /// my old trades go": they exist, the filter just stops short of them.
    pub hidden_outside: usize,
    /// Saved trades the Source filter keeps out of this view.
    pub hidden_by_source: usize,
    /// The filtered trades, each still carrying the symbol folder and the
    /// session source it was journaled under — the report lists them, and
    /// a list that could not name its instrument would be the very gap
    /// this window exists to close.
    pub rows: Vec<HistoryRow>,
    /// The realized-equity walk over `rows`, cut with the view rather than
    /// re-walked on every frame.
    pub equity: EquityWalk,
    pub report: PerformanceReport,
}

impl ReportView {
    /// Cut `history` to one view: the rows the Source filter admits, then
    /// the rows inside `range` when a span of days is picked, or inside
    /// `period` measured back from the newest admitted trade when none is.
    ///
    /// A picked range is an explicit answer to "which days"; it takes over
    /// from the period rather than intersecting with it, so a chosen date
    /// can never come back empty because a pill the trader had forgotten
    /// about was cutting too. The anchor is the newest trade in scope —
    /// after the Source filter, never a clock.
    #[must_use]
    pub fn cut(
        history: &LoadedHistory,
        period: ReportPeriod,
        source: SourceFilter,
        range: Option<DateRange>,
        tz: TzOffset,
    ) -> Self {
        let in_scope: Vec<&HistoryRow> = history
            .rows
            .iter()
            .filter(|row| source.admits(row.source))
            .collect();
        let hidden_by_source = history.rows.len().saturating_sub(in_scope.len());
        let anchor_ms = in_scope.last().map(|row| row.trade.closed_ms);
        let cutoff = match range {
            Some(_) => None,
            None => anchor_ms.and_then(|anchor| period.cutoff_ms(anchor, tz)),
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
        Self {
            period,
            source,
            range,
            tz,
            anchor_ms,
            hidden_outside,
            hidden_by_source,
            rows,
            equity,
            report,
        }
    }
}

#[cfg(test)]
mod tests;
