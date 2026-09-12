//! The performance-report window: its chrome, its filters and its calendar.
//!
//! What period is being reported on, which sessions and symbols are in it,
//! and how the answer is laid out. The numbers themselves come from
//! `quantick_sim`; the tiles, curve and grids that show them are in
//! [`super::tables`] and [`super::curve`].

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_sim::{ClosedTrade, PerformanceReport};

use super::curve::{EquityWalk, draw_equity_curve};
use super::ledger::{LedgerScope, SourceFilter, load_history};
use super::tables::{
    draw_exit_reason_grid, draw_report_grid, draw_report_tiles, draw_side_grid, draw_trade_list,
};
use super::{
    CALENDAR_CELL_H_PX, CALENDAR_CELL_W_PX, CURVE_MIN_H_CALENDAR_PX, CURVE_MIN_H_PX,
    CUSTOM_PERIOD_FIELD_PX, HistoryRow, REPORT_DEFAULT_H_PX, REPORT_DEFAULT_W_PX,
    REPORT_FOOTER_RESERVE_PX, REPORT_GRID_MIN_H_PX, REPORT_MIN_HEIGHT_CALENDAR_PX,
    REPORT_MIN_HEIGHT_PX, REPORT_MIN_WIDTH_PX, ReportEnv, ReportResponse, ReportSnapshot,
    ReportState, ReportView,
};
use crate::paper_calendar::{
    CalendarAction, CivilDate, DAY_MS, DateRange, DayIndex, DaySelection, today,
};
use crate::paper_chrome::{list_symbol_folders, pill_toggle};
use crate::theme;
use crate::timezone::TzOffset;

impl ReportState {
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
    pub(super) fn open_report(&mut self, env: &ReportEnv<'_>) {
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

    pub(super) fn label(self) -> &'static str {
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
    pub(super) fn cutoff_ms(self, anchor_ms: i64, tz: TzOffset) -> Option<i64> {
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
pub(super) fn parse_period(text: &str) -> Option<i64> {
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
pub(super) fn fmt_period_ms(period_ms: i64) -> String {
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
