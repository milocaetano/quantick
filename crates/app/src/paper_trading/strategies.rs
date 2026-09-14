//! Order strategies: the saved bracket presets and their editor.
//!
//! A strategy is a name and two offsets the ticket can arm, so the trader
//! does not retype the same stop and target every session. The rows and the
//! editor are here; what an armed strategy *does* to an order is the
//! account's arithmetic.

use eframe::egui;
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::{NEW_RUNG_TICKS, PaperTrading, STRATEGY_NONE};
use crate::paper_chrome::{caption, fmt_decimal};
use crate::theme;

impl PaperTrading {
    /// The ticket's strategy row: which named ladder the next order rests
    /// with, and the way into the editor that shapes them.
    ///
    /// Returns true when the selection changed, so the app can remember it.
    pub(super) fn draw_strategy_row(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Strategy")
                    .color(theme::TEXT_MUTED)
                    .small(),
            )
            .on_hover_text(
                "a named exit ladder: the order rests with its parts already drawn on \
                 the chart. <None> rests a bare order you bracket by hand",
            );
            let selected = self.account.selected_order_strategy().map_or_else(
                || STRATEGY_NONE.to_owned(),
                |strategy| strategy.name.clone(),
            );
            let mut choice = self.account.selected_strategy;
            egui::ComboBox::from_id_salt("paper_order_strategy")
                .width(140.0)
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, None, STRATEGY_NONE);
                    for (index, strategy) in self.account.strategies.iter().enumerate() {
                        ui.selectable_value(&mut choice, Some(index), &strategy.name);
                    }
                });
            if choice != self.account.selected_strategy {
                self.account.selected_strategy = choice;
                changed = true;
            }
            if ui
                .small_button("Edit…")
                .on_hover_text("build and change the named exit ladders")
                .clicked()
            {
                self.strategy_editor_open = true;
                self.strategy_editing =
                    self.account
                        .selected_strategy
                        .or(if self.account.strategies.is_empty() {
                            None
                        } else {
                            Some(0)
                        });
            }
        });
        // The selected ladder, said in words: a trader must be able to read
        // what the next click will place without aiming it first.
        if let Some(strategy) = self.account.selected_order_strategy() {
            ui.label(
                egui::RichText::new(summarise_strategy(strategy))
                    .color(theme::TEXT_SUPPORT)
                    .small(),
            );
            // An armed ladder that cannot resolve draws nothing on the
            // chart, and a trader holding the modifier over a silent chart
            // has no way to know why. The reason belongs here, beside the
            // thing that is armed, in the colour the rest of this surface
            // uses for "this will not do what you think".
            if let Err(error) = strategy.validate() {
                ui.label(
                    egui::RichText::new(format!("not armed - {}", error.advice()))
                        .color(theme::AMBER)
                        .small(),
                );
            }
        }
        // The editor itself is drawn from the app's own frame, not from
        // here: it is a window, and a window that lives inside a dock tab
        // disappears the moment the trader looks at another panel.
        changed
    }

    /// The strategy editor: the list on the left, the open one's rows on the
    /// right, and one line saying why it cannot be used when it cannot.
    ///
    /// A window rather than a panel, because building a ladder is a job the
    /// trader finishes and closes - it is not part of reading the chart.
    ///
    /// Returns true when anything changed, so the app can persist it.
    pub(crate) fn draw_strategy_editor(&mut self, ctx: &egui::Context) -> bool {
        if !self.strategy_editor_open {
            return false;
        }
        // Opening onto a blank right pane while the list holds strategies is
        // an editor that looks broken. Whatever route opened it - the
        // ticket's button, or the launch hook, which has no click to carry a
        // choice - it opens on the one the ticket is armed with, else the
        // first.
        if self.strategy_editing.is_none() && !self.account.strategies.is_empty() {
            self.strategy_editing = Some(self.account.selected_strategy.unwrap_or(0));
        }
        let mut changed = false;
        let mut open = true;
        egui::Window::new("Exit strategies")
            .open(&mut open)
            .resizable(true)
            // Clear of the plot's top-left corner, where the indicator
            // legend lives: a window that opens under the legend reads as a
            // broken one, and the legend is an overlay the trader cannot
            // move out of the way.
            .default_pos(egui::pos2(360.0, 140.0))
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_min_width(150.0);
                        ui.label(caption("STRATEGIES"));
                        for index in 0..self.account.strategies.len() {
                            let selected = self.strategy_editing == Some(index);
                            let name = self.account.strategies[index].name.clone();
                            if ui.selectable_label(selected, name).clicked() {
                                self.strategy_editing = Some(index);
                            }
                        }
                        ui.add_space(4.0);
                        if ui
                            .small_button("+ New")
                            .on_hover_text("start a ladder from one whole-position rung")
                            .clicked()
                        {
                            self.account
                                .strategies
                                .push(new_strategy(self.account.strategies.len()));
                            self.strategy_editing = Some(self.account.strategies.len() - 1);
                            self.strategy_dirty = true;
                        }
                        if let Some(index) = self.strategy_editing
                            && ui
                                .small_button("Delete")
                                .on_hover_text("remove this strategy")
                                .clicked()
                        {
                            // Read the selection's *name* before the list
                            // shifts: resolving the index afterwards answers
                            // with whichever strategy slid into that slot, and
                            // the ticket would silently arm the neighbour of
                            // the one that was deleted.
                            let selected = self
                                .account()
                                .selected_order_strategy()
                                .map(|strategy| strategy.name.clone());
                            let removed = self.account.strategies.remove(index).name;
                            self.account.selected_strategy =
                                selected.filter(|name| *name != removed).and_then(|name| {
                                    self.account
                                        .strategies
                                        .iter()
                                        .position(|strategy| strategy.name == name)
                                });
                            self.strategy_editing = if self.account.strategies.is_empty() {
                                None
                            } else {
                                Some(index.min(self.account.strategies.len() - 1))
                            };
                            self.strategy_dirty = true;
                        }
                    });
                    ui.separator();
                    ui.vertical(|ui| {
                        // Structural edits and typed ones alike: held until
                        // the window closes, so one word is one save.
                        self.strategy_dirty |= self.draw_strategy_rows(ui);
                    });
                });
            });
        if !open {
            self.strategy_editor_open = false;
            // Closing is the save point: whatever was typed is what the
            // trader meant, and it reaches disk once.
            changed |= std::mem::take(&mut self.strategy_dirty);
        }
        changed
    }

    /// The open strategy's name and rungs.
    fn draw_strategy_rows(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(index) = self.strategy_editing else {
            ui.label(
                egui::RichText::new("No strategy yet - New starts one.")
                    .color(theme::TEXT_SUPPORT)
                    .small(),
            );
            return false;
        };
        let Some(strategy) = self.account.strategies.get_mut(index) else {
            return false;
        };
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Name").color(theme::TEXT_MUTED).small());
            if ui
                .add(egui::TextEdit::singleline(&mut strategy.name).desired_width(200.0))
                .changed()
            {
                changed = true;
            }
        });
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Qty %")
                    .color(theme::TEXT_MUTED)
                    .small(),
            );
            ui.add_space(38.0);
            ui.label(egui::RichText::new("Gain").color(theme::TEXT_MUTED).small());
            ui.add_space(34.0);
            ui.label(egui::RichText::new("Loss").color(theme::TEXT_MUTED).small());
            ui.label(
                egui::RichText::new("ticks")
                    .color(theme::TEXT_FAINT)
                    .small(),
            );
        });
        let mut remove: Option<usize> = None;
        for (row_index, row) in strategy.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                let mut share = row.share_percent.to_f64().unwrap_or_default();
                if ui
                    .add(
                        egui::DragValue::new(&mut share)
                            .speed(1.0)
                            .range(0.0..=100.0)
                            .suffix("%"),
                    )
                    .changed()
                {
                    row.share_percent = Decimal::from_f64_retain(share)
                        .unwrap_or_default()
                        .round_dp(2);
                    changed = true;
                }
                changed |= ticks_field(ui, &mut row.gain_ticks, "how far the target sits");
                changed |= ticks_field(ui, &mut row.loss_ticks, "how far the stop sits");
                if ui
                    .small_button("−")
                    .on_hover_text("remove this row")
                    .clicked()
                {
                    remove = Some(row_index);
                }
            });
        }
        if let Some(row_index) = remove {
            strategy.rows.remove(row_index);
            changed = true;
        }
        if strategy.rows.len() < crate::order_strategies::MAX_ROWS
            && ui
                .small_button("+ Row")
                .on_hover_text("split the exit one more time")
                .clicked()
        {
            // A row added at zero makes the whole strategy invalid the
            // instant it appears - the shares no longer describe a whole
            // position - and the chart then silently stops projecting a
            // ladder the ticket still says is armed. The new row takes what
            // is left of 100%, and when nothing is left it halves the last
            // row rather than arriving broken.
            let assigned: Decimal = strategy.rows.iter().map(|row| row.share_percent).sum();
            let share = if assigned < Decimal::ONE_HUNDRED {
                Decimal::ONE_HUNDRED - assigned
            } else {
                let last = strategy
                    .rows
                    .last_mut()
                    .expect("a strategy always has a row to split");
                let half = (last.share_percent / Decimal::TWO).round_dp(2);
                last.share_percent -= half;
                half
            };
            strategy.rows.push(crate::order_strategies::StrategyRow {
                share_percent: share,
                gain_ticks: Some(NEW_RUNG_TICKS),
                loss_ticks: Some(NEW_RUNG_TICKS),
            });
            changed = true;
        }
        // The verdict, in one line, beside the fields that caused it.
        match strategy.validate() {
            Ok(()) => {
                ui.label(
                    egui::RichText::new("ready - the shares add up")
                        .color(theme::TEXT_SUPPORT)
                        .small(),
                );
            }
            Err(error) => {
                ui.label(
                    egui::RichText::new(error.advice())
                        .color(theme::AMBER)
                        .small(),
                );
            }
        }
        changed
    }
}

/// A ladder to start from: one rung covering the whole position, which is
/// the plain bracket a trader already knows, ready to be split.
fn new_strategy(existing: usize) -> crate::order_strategies::OrderStrategy {
    crate::order_strategies::OrderStrategy {
        name: format!("Strategy {}", existing + 1),
        rows: vec![crate::order_strategies::StrategyRow {
            share_percent: Decimal::ONE_HUNDRED,
            gain_ticks: Some(NEW_RUNG_TICKS),
            loss_ticks: Some(NEW_RUNG_TICKS),
        }],
    }
}

/// One optional tick distance, with a box that empties to "no leg here".
fn ticks_field(ui: &mut egui::Ui, ticks: &mut Option<u32>, hover: &str) -> bool {
    let mut on = ticks.is_some();
    let mut changed = false;
    if ui.checkbox(&mut on, "").on_hover_text(hover).changed() {
        *ticks = if on { Some(NEW_RUNG_TICKS) } else { None };
        changed = true;
    }
    let mut value = ticks.unwrap_or(0);
    let enabled = ticks.is_some();
    if ui
        .add_enabled(
            enabled,
            egui::DragValue::new(&mut value)
                .speed(1.0)
                .range(1..=100_000),
        )
        .changed()
        && enabled
    {
        *ticks = Some(value);
        changed = true;
    }
    changed
}

/// A strategy said in one line: the rungs, in the trader's own order.
pub(super) fn summarise_strategy(strategy: &crate::order_strategies::OrderStrategy) -> String {
    let rungs: Vec<String> = strategy
        .rows
        .iter()
        .map(|row| {
            let gain = row
                .gain_ticks
                .map_or_else(|| "runs".to_owned(), |ticks| format!("+{ticks}"));
            let loss = row
                .loss_ticks
                .map_or_else(|| "no stop".to_owned(), |ticks| format!("-{ticks}"));
            format!("{}% {gain}/{loss}", fmt_decimal(row.share_percent))
        })
        .collect();
    rungs.join(" · ")
}
