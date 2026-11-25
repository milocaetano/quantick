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
use crate::paper_calendar::{DaySelection, today};
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
        let mut open = true;
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
        crate::popup::centred(egui::Window::new("Trading performance"), id, ctx)
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
                if self.draw_report_filters(ui) {
                    self.update(Intent::ReloadReport);
                }
                self.draw_report_calendar(ui, tz);
                if let Some(scope) = self.report_scope_label() {
                    let reset = ui
                        .horizontal(|ui| {
                            ui.label(scope);
                            ui.button("Back to ordinary report").clicked()
                        })
                        .inner;
                    if reset {
                        self.update(Intent::ClearFilters(env.symbol.to_owned()));
                    }
                }
                self.ensure_report_view(tz, env.pnl);
                if let Some(view) = self.report_view() {
                    ui.label(
                        egui::RichText::new(view.scope_line())
                            .color(theme::get(ui.ctx()).text_support()),
                    );
                }
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
                    _ => self.draw_report_empty(ui, env),
                }
                self.draw_report_footer(ui, env);
            });
        if toggle_list {
            self.update(Intent::ListOpen(!self.report_list_open()));
        }
        if !open {
            self.update(Intent::Close);
        }
        self.update(Intent::DrainToast);
    }

    /// Why nothing is listed and the way back.
    fn draw_report_empty(&mut self, ui: &mut egui::Ui, env: &ReportEnv<'_>) {
        let palette = theme::get(ui.ctx());
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("No saved trades for this filter.").color(palette.text_primary()),
        );
        ui.label(egui::RichText::new(self.empty_hint()).color(palette.text_support()));
        if self.filters_narrowed(env.symbol)
            && ui
                .button("Clear filters")
                .on_hover_text("back to this symbol, all time, no dates")
                .clicked()
        {
            self.update(Intent::ClearFilters(env.symbol.to_owned()));
        }
    }

    /// The unreadable-file count, the history scan totals and the P&L footnote.
    fn draw_report_footer(&mut self, ui: &mut egui::Ui, env: &ReportEnv<'_>) {
        let palette = theme::get(ui.ctx());
        ui.add_space(6.0);
        if let Some(history) = self.report() {
            super::draw_history_warning(ui, history);
            ui.label(
                egui::RichText::new(history.scan_summary())
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
                let on = self.report_source().contains(source);
                if pill_toggle(ui, source.label(), on, source.hover()).clicked() {
                    self.update(Intent::Source(self.report_source().toggled(source)));
                }
            }
            ui.separator();
            ui.label(
                egui::RichText::new("Period")
                    .color(palette.text_muted())
                    .small(),
            );
            let ranged = self.applied_dates().is_some();
            for period in ReportPeriod::PILLS {
                let on = !ranged && self.report_period() == period;
                if pill_toggle(
                    ui,
                    period.label(),
                    on,
                    "Apply a period from the newest saved close in scope",
                )
                .clicked()
                {
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
        reload
    }

    fn draw_report_calendar(&mut self, ui: &mut egui::Ui, tz: TzOffset) {
        ui.strong("Report dates");
        let mut draft = self.date_draft().clone();
        ui.horizontal_wrapped(|ui| {
            for (label, text) in [
                ("Start date", &mut draft.start),
                ("End date", &mut draft.end),
            ] {
                ui.vertical(|ui| {
                    let id = egui::Id::new(("report date", label));
                    let label = ui.label(label);
                    ui.add(
                        egui::TextEdit::singleline(text)
                            .id(id)
                            .hint_text("YYYY-MM-DD")
                            .char_limit(10)
                            .desired_width(theme::get(ui.ctx()).layout(100.0)),
                    )
                    .labelled_by(label.id);
                });
            }
            if &draft != self.date_draft() {
                self.update(Intent::Draft(draft));
            }
            let valid = self.date_draft().range().is_ok();
            let mut calendar = self.calendar();
            if ui
                .add_enabled(valid, egui::Button::new("Generate report"))
                .clicked()
            {
                self.update(Intent::GenerateDates);
            }
            if ui.toggle_value(&mut calendar.open, "Calendar").changed() {
                self.update(Intent::Calendar(calendar));
            }
            if ui
                .button("Clear dates")
                .on_hover_text(format!("Restore {}", self.report_period().phrase()))
                .clicked()
            {
                self.update(Intent::Dates(DaySelection::None));
            }
        });
        if let Some(feedback) = self.date_feedback() {
            ui.label(feedback);
        }
        let mut calendar = self.calendar();
        if calendar.open {
            let day = egui::ScrollArea::horizontal()
                .id_salt("report_calendar_grid")
                .show(ui, |ui| {
                    crate::paper_calendar::draw_month(
                        ui,
                        self.report_days(),
                        &mut calendar,
                        egui::vec2(CALENDAR_CELL_W_PX, CALENDAR_CELL_H_PX),
                        today(tz),
                    )
                })
                .inner;
            if calendar.month != self.calendar().month {
                self.update(Intent::Calendar(calendar));
            }
            if let Some(day) = day {
                self.update(Intent::PickDate(day));
            }
        }
    }
}
