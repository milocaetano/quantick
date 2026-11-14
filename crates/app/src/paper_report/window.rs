//! The performance-report window: its chrome, its filters and its calendar.
//!
//! What period is being reported on, which sessions and symbols are in it,
//! and how the answer is laid out. The numbers themselves come from
//! `quantick_sim`; the tiles, curve and grids that show them are in
//! [`super::tables`] and [`super::curve`].

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_paper::report::ReportPeriod;
use quantick_ui_model::paper_report::Intent;

use super::curve::draw_equity_curve;
use super::tables::{
    draw_exit_reason_grid, draw_report_grid, draw_report_tiles, draw_side_grid, draw_trade_list,
};
use super::{
    CALENDAR_CELL_H_PX, CALENDAR_CELL_W_PX, CURVE_MIN_H_CALENDAR_PX, CURVE_MIN_H_PX,
    CUSTOM_PERIOD_FIELD_PX, Paint, REPORT_DEFAULT_H_PX, REPORT_DEFAULT_W_PX,
    REPORT_FOOTER_RESERVE_PX, REPORT_GRID_MIN_H_PX, REPORT_MIN_HEIGHT_CALENDAR_PX,
    REPORT_MIN_HEIGHT_PX, REPORT_MIN_WIDTH_PX, ReportEnv, SourceFilter,
};
use crate::paper_calendar::{CivilDate, DaySelection, today};
use crate::paper_chrome::pill_toggle;
use crate::theme;
use crate::timezone::TzOffset;

impl Paint<'_, '_> {
    /// The performance report, computed from what is actually on disk.
    /// Non-modal by the app's contract — dimming the chart while a
    /// simulated position is open would be dangerous.
    pub(crate) fn draw_window(&mut self, ctx: &egui::Context, tz: TzOffset, env: &ReportEnv<'_>) {
        self.update(Intent::DrainToast);
        if !self.report_open() {
            return;
        }
        self.ensure_report_view(tz, env.pnl);
        let mut open = true;
        let mut reload = false;
        let mut toggle_list = false;
        let style = ctx.style();
        let title = egui::TextStyle::Heading.resolve(&style);
        let title_height = ctx
            .fonts(|fonts| fonts.row_height(&title))
            .max(style.spacing.interact_size.y);
        let frame = egui::Frame::window(&style);
        let max_height = (ctx.available_rect().height()
            - title_height
            - frame.total_margin().sum().y
            - frame.inner_margin.sum().y)
            .max(0.0);
        let id = egui::Id::new("Simulated performance");
        crate::popup::centred(egui::Window::new("Simulated performance"), id, ctx)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            // Keep every section reachable when typography exceeds the viewport budget.
            .vscroll(true)
            .max_height(max_height)
            // Wide enough for every column of the trade list and tall
            // enough for the tiles, the curve and a readable page of it.
            // Both are still only a starting size — the window resizes.
            .default_size(egui::vec2(REPORT_DEFAULT_W_PX, REPORT_DEFAULT_H_PX))
            .min_width(REPORT_MIN_WIDTH_PX)
            // An expanded month grid needs its own room. Without this the
            // report at its smallest pushed the tiles, the curve and the
            // disclosure lines out through the bottom of the window.
            .min_height(
                (if self.calendar().open {
                    REPORT_MIN_HEIGHT_CALENDAR_PX
                } else {
                    REPORT_MIN_HEIGHT_PX
                })
                .min(max_height),
            )
            .show(ctx, |ui| {
                #[cfg(any(feature = "scenario-harness", test))]
                self.native.scroll_probe.report(ui);
                reload = self.draw_report_filters(ui);
                self.draw_report_calendar(ui, tz);
                // The rows above can retire the view — a picked day, a
                // pill, a cleared range. Re-cutting it here rather than
                // next frame is what keeps a click from flashing the empty
                // state and collapsing the window for one frame.
                self.ensure_report_view(tz, env.pnl);
                ui.separator();
                let list_open = self.report_list_open();
                let calendar_open = self.calendar().open;
                match self.updates.state().report_view() {
                    Some(view) if !view.rows.is_empty() => {
                        draw_report_tiles(ui, view);
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
                                toggle_list = draw_trade_list(
                                    ui,
                                    view,
                                    list_open,
                                    &mut self.native.report_table_layout,
                                );
                                draw_report_grid(ui, &view.report);
                                draw_side_grid(ui, &view.report);
                                draw_exit_reason_grid(ui, &view.report);
                            });
                        #[cfg(any(feature = "scenario-harness", test))]
                        self.native.scroll_probe.rows(ui);
                    }
                    _ => reload |= self.draw_report_empty(ui, env),
                }
                self.draw_report_footer(ui, env);
            });
        if toggle_list {
            self.update(Intent::ListOpen(!self.report_list_open()));
        }
        if reload {
            self.update(Intent::ReloadReport);
        }
        if !open {
            self.update(Intent::Close);
        }
        self.update(Intent::DrainToast);
    }

    /// Why nothing is listed and the way back; true when history must reload.
    fn draw_report_empty(&mut self, ui: &mut egui::Ui, env: &ReportEnv<'_>) -> bool {
        let palette = theme::get(ui.ctx());
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("No saved trades for this filter.").color(palette.text_primary()),
        );
        let scope = self
            .report_symbol()
            .as_deref()
            .unwrap_or("any symbol")
            .to_owned();
        let hidden_by_source = self
            .report_view()
            .as_ref()
            .map_or(0, |view| view.hidden_by_source);
        let picked = self.calendar().selection.range();
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
                self.report_period().phrase(),
            )
        };
        ui.label(egui::RichText::new(text).color(palette.text_support()));
        let default_symbol = (!env.symbol.is_empty()).then(|| env.symbol.to_owned());
        if (self.report_period() != ReportPeriod::All
            || self.report_symbol() != &default_symbol
            || picked.is_some())
            && ui
                .button("Clear filters")
                .on_hover_text("back to this symbol, all time, no dates")
                .clicked()
        {
            self.update(Intent::ClearFilters(env.symbol.to_owned()));
            return true;
        }
        false
    }

    /// The unreadable-file count, the scope line and the P&L footnote.
    fn draw_report_footer(&mut self, ui: &mut egui::Ui, env: &ReportEnv<'_>) {
        let palette = theme::get(ui.ctx());
        ui.add_space(6.0);
        if let Some(history) = self.report() {
            if history.unreadable_files > 0 || history.problem_rows > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "{} file(s) unreadable, {} row(s) skipped - counted, never \
                         silently dropped.",
                        history.unreadable_files, history.problem_rows,
                    ))
                    .color(palette.warn())
                    .small(),
                );
            }
            ui.label(
                egui::RichText::new(format!(
                    "{} trade(s) across {} file(s) in scope",
                    history.rows.len(),
                    history.files
                ))
                .color(palette.text_muted())
                .small(),
            );
        }
        let _footer =
            ui.label(egui::RichText::new(env.pnl.footnote()).color(palette.text_support()));
        #[cfg(any(feature = "scenario-harness", test))]
        self.native.scroll_probe.footer(ui, _footer.rect);
    }

    /// The filter row (symbol combo + period pills + refresh) and the
    /// support line stating the anchor out loud. Returns whether the
    /// history must be re-read; anything it wants the *host* to do goes
    /// into `asked`.
    fn draw_report_filters(&mut self, ui: &mut egui::Ui) -> bool {
        let palette = theme::get(ui.ctx());
        let mut reload = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("Symbol")
                    .color(palette.text_muted())
                    .small(),
            );
            let selected = self
                .report_symbol()
                .clone()
                .unwrap_or_else(|| "All symbols".to_owned());
            let symbols = self.report_symbols().to_vec();
            egui::ComboBox::from_id_salt("paper_report_symbol")
                .width(140.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(self.report_symbol().is_none(), "All symbols")
                        .clicked()
                    {
                        self.update(Intent::Symbol(None));
                        reload = true;
                    }
                    for symbol in symbols {
                        let on = self.report_symbol().as_deref() == Some(symbol.as_str());
                        if ui.selectable_label(on, &symbol).clicked() {
                            self.update(Intent::Symbol(Some(symbol)));
                            reload = true;
                        }
                    }
                });
            ui.separator();
            ui.label(
                egui::RichText::new("Source")
                    .color(palette.text_muted())
                    .small(),
            );
            for source in SourceFilter::PILLS {
                let on = self.report_source() == source;
                if pill_toggle(ui, source.label(), on, source.hover()).clicked() && !on {
                    self.update(Intent::Source(source));
                }
            }
            ui.separator();
            ui.label(
                egui::RichText::new("Period")
                    .color(palette.text_muted())
                    .small(),
            );
            // A picked calendar range takes the cut over, so no pill may
            // keep looking armed: a lit "Today" beside a date chip reads as
            // the filter that produced the numbers, and it is not.
            let ranged = self.calendar().selection.range().is_some();
            for period in ReportPeriod::PILLS {
                let on = !ranged && self.report_period() == period;
                let hover = if ranged {
                    "standing down while a date range is picked - clear the dates to use the \
                     period pills again"
                } else {
                    "measured back from the newest saved trade in scope, not the wall clock"
                };
                if pill_toggle(ui, period.label(), on, hover).clicked() && !on {
                    // Reaching for a pill is a decision to stop filtering
                    // by date, so it says so rather than doing nothing.
                    self.update(Intent::Period(period));
                }
            }
            let mut custom_text = self.report_custom_text().to_owned();
            let response = ui
                .add(
                    egui::TextEdit::singleline(&mut custom_text)
                        .desired_width(CUSTOM_PERIOD_FIELD_PX)
                        .hint_text("2d"),
                )
                .on_hover_text("type a period - 45m, 12h, 2d or 1w - and press Enter");
            if custom_text != self.report_custom_text() {
                self.update(Intent::CustomText(custom_text));
            }
            if response.lost_focus() {
                self.update(Intent::SubmitCustom {
                    enter: ui.input(|input| input.key_pressed(egui::Key::Enter)),
                });
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
                    self.update(Intent::Import);
                }
            });
        });
        if let Some(view) = self.report_view() {
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
            ui.label(egui::RichText::new(text).color(palette.text_support()));
        }
        reload
    }

    /// The calendar row: a toggle that names what is picked, and - when
    /// expanded - the month grid itself. Collapsed, the report keeps the
    /// layout it always had; expanded it answers "which days did I trade,
    /// and what happened on the 12th" without a text field.
    fn draw_report_calendar(&mut self, ui: &mut egui::Ui, tz: TzOffset) {
        let palette = theme::get(ui.ctx());
        ui.horizontal_wrapped(|ui| {
            let picked = self.calendar().selection.range();
            let label = match picked {
                Some(range) => format!("{} {}", icons::CALENDAR_BLANK, range.label()),
                None => format!("{} Dates", icons::CALENDAR_BLANK),
            };
            if pill_toggle(
                ui,
                &label,
                self.calendar().open || picked.is_some(),
                "pick a day, or click a second day for a range - highlighted days hold trades",
            )
            .clicked()
            {
                let mut calendar = self.calendar();
                calendar.open = !calendar.open;
                self.update(Intent::Calendar(calendar));
            }
            if let Some(range) = picked {
                ui.label(
                    egui::RichText::new(format!(
                        "{} day(s) · {} trade(s)",
                        range.days(),
                        self.report_view()
                            .as_ref()
                            .map_or(0, |view| view.rows.len()),
                    ))
                    .color(palette.text_muted())
                    .small(),
                );
                if ui
                    .small_button(icons::X)
                    .on_hover_text("clear the date filter and go back to the period pills")
                    .clicked()
                {
                    self.update(Intent::Dates(DaySelection::None));
                }
            } else if self.report_days().is_empty() {
                ui.label(
                    egui::RichText::new("no days on record in this scope")
                        .color(palette.text_muted())
                        .small(),
                );
            } else if let (Some(oldest), Some(newest)) =
                (self.report_days().first(), self.report_days().last())
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
                        self.report_days().len(),
                        oldest.iso(),
                        newest.iso(),
                    ))
                    .color(palette.text_muted())
                    .small(),
                );
            }
        });
        if !self.calendar().open {
            return;
        }
        // The grid is drawn from a cached day index, so an open calendar
        // costs a fixed 42 cells per frame however long the history is.
        let mut calendar = self.calendar();
        let _action = egui::ScrollArea::both()
            .id_salt("report_calendar_grid")
            .max_height(ui.available_height().max(0.0) * 0.45)
            .show(ui, |ui| {
                crate::paper_calendar::draw_month(
                    ui,
                    self.report_days(),
                    &mut calendar,
                    egui::vec2(CALENDAR_CELL_W_PX, CALENDAR_CELL_H_PX),
                    // Only reached when nothing on disk names a month. The
                    // calendar module is deliberately clock-free, so the host —
                    // which may read a clock — says where "no history at all"
                    // should open.
                    today(tz),
                )
            })
            .inner;
        if (calendar.month, calendar.selection, calendar.open)
            != (
                self.calendar().month,
                self.calendar().selection,
                self.calendar().open,
            )
        {
            self.update(Intent::Calendar(calendar));
        }
    }
}
