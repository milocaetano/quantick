//! The trades ledger: every closed trade the journal folder holds.
//!
//! The tab beside the report, and the scoping that decides which sessions
//! feed it. The report answers "how did the account do?"; the ledger answers
//! "which trades, in what order, from which file?", so the two read the same
//! journal for different questions and keep their state apart.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui_phosphor::regular as icons;
#[cfg(test)]
use quantick_sim::PerformanceReport;
use quantick_sim::{ClosedTrade, history};
use rust_decimal::Decimal;

use super::rows::{
    LedgerRow, draw_day_header, draw_group_header, draw_ledger_row, draw_more_row, draw_open_row,
    push_by_day,
};
use super::{
    LEDGER_PAGE_TRADES, LEDGER_ROW_HEIGHT_PX, LEDGER_SCOPE_COMBO_PX, ReportEnv, ReportState,
    TOTALS_STRIP_PX,
};
use crate::paper_calendar::CivilDate;
use crate::paper_chrome::{
    caption, fmt_signed_points, list_symbol_folders, points_color, sanitize_symbol,
};
use crate::theme;
use crate::timezone::TzOffset;

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
    pub(super) fn all_days_collapsed(&self, env: &ReportEnv<'_>) -> bool {
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
}

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
    pub(super) fn folder<'a>(&'a self, chart: &'a str) -> Option<&'a str> {
        match self {
            Self::Chart => Some(chart),
            Self::Symbol(symbol) => Some(symbol.as_str()),
            Self::All => None,
        }
    }

    /// What the picker shows for this scope.
    pub(super) fn label(&self, chart: &str) -> String {
        match self {
            Self::Chart if chart.is_empty() => "This chart".to_owned(),
            Self::Chart => format!("This chart · {chart}"),
            Self::Symbol(symbol) => symbol.clone(),
            Self::All => "All symbols".to_owned(),
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

/// The ledger's totals over every saved trade in scope. Summed when the
/// folder is read, never on the frame: walking a year of sessions sixty
/// times a second to print one line is work nobody asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct LedgerTotals {
    pub(super) trades: usize,
    pub(super) wins: usize,
    pub(super) net: Decimal,
}

impl LedgerTotals {
    pub(super) fn of<'a>(trades: impl Iterator<Item = &'a ClosedTrade>) -> Self {
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
    pub(super) fn plus(self, other: Self) -> Self {
        Self {
            trades: self.trades + other.trades,
            wins: self.wins + other.wins,
            net: self.net.saturating_add(other.net),
        }
    }

    /// Whole-percent win rate; `None` when there is nothing to divide by.
    pub(super) fn win_rate(self) -> Option<usize> {
        (self.wins * 100).checked_div(self.trades)
    }
}

/// How much saved history the ledger is showing and how much it is
/// holding back. Pure so the count printed on the "show older" control and
/// the rows above it can never disagree — a button promising trades that
/// are not there is worse than no button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LedgerPage {
    /// Saved trades rendered, newest first.
    pub(super) shown: usize,
    /// Saved trades loaded but not yet revealed.
    pub(super) remaining: usize,
}

impl LedgerPage {
    /// `pages` pages of [`LEDGER_PAGE_TRADES`] each, clamped to `total`.
    /// Page zero is treated as one: the ledger always shows something.
    pub(super) fn of(total: usize, pages: usize) -> Self {
        let shown = LEDGER_PAGE_TRADES.saturating_mul(pages.max(1)).min(total);
        Self {
            shown,
            remaining: total.saturating_sub(shown),
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
    pub(super) const PILLS: [Self; 3] = [Self::Real, Self::Replay, Self::All];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Real => "Real",
            Self::Replay => "Replay",
            // Not "All": the period pills own that word on the same row,
            // and two identical pills a hand-width apart invite the wrong
            // click.
            Self::All => "Both",
        }
    }

    pub(super) fn hover(self) -> &'static str {
        match self {
            Self::Real => {
                "live sessions - files saved before quantick recorded a source count as real"
            }
            Self::Replay => "practice sessions driven by a market-replay recording",
            Self::All => "live and replay together - mixed on purpose",
        }
    }

    /// Whether a row with this recorded source belongs to the filter.
    pub(super) fn admits(self, source: Option<history::SessionSource>) -> bool {
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
