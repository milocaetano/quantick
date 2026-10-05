//! Order strategies: the saved bracket presets and their editor.
//!
//! A strategy is a name and two offsets the ticket can arm, so the trader
//! does not retype the same stop and target every session. The widgets are
//! here; what a new strategy or a new row is, and what an armed strategy
//! *does* to an order, is `quantick_paper`'s - the editor's window state is
//! the desk's [`StrategyEditor`], and the list is the account's.

use eframe::egui;
use quantick_paper::PaperAccount as Core;
use quantick_paper::desk::StrategyEditor;
use quantick_paper::order_strategies::{NEW_RUNG_TICKS, OrderStrategy};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::STRATEGY_NONE;
use crate::paper_chrome::caption;
use crate::theme;

/// The ticket's strategy row: which named ladder the next order rests
/// with, and the way into the editor that shapes them.
///
/// Returns true when the selection changed, so the app can remember it.
pub(super) fn draw_strategy_row(
    ui: &mut egui::Ui,
    account: &mut Core,
    editor: &mut StrategyEditor,
) -> bool {
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
        let selected = account.selected_order_strategy().map_or_else(
            || STRATEGY_NONE.to_owned(),
            |strategy| strategy.name.clone(),
        );
        let mut choice = account.selected_strategy();
        egui::ComboBox::from_id_salt("paper_order_strategy")
            .width(140.0)
            .selected_text(selected)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut choice, None, STRATEGY_NONE);
                for (index, strategy) in account.order_strategies().iter().enumerate() {
                    ui.selectable_value(&mut choice, Some(index), &strategy.name);
                }
            });
        if choice != account.selected_strategy() {
            account.select_strategy(choice);
            changed = true;
        }
        if ui
            .small_button("Edit…")
            .on_hover_text("build and change the named exit ladders")
            .clicked()
        {
            editor.open_on(
                account.selected_strategy(),
                account.order_strategies().len(),
            );
        }
    });
    // The selected ladder, said in words: a trader must be able to read
    // what the next click will place without aiming it first.
    if let Some(strategy) = account.selected_order_strategy() {
        ui.label(
            egui::RichText::new(strategy.summary())
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
pub(super) fn draw_strategy_editor(
    ctx: &egui::Context,
    account: &mut Core,
    editor: &mut StrategyEditor,
) -> bool {
    if !editor.open {
        return false;
    }
    editor.settle_editing(
        account.selected_strategy(),
        account.order_strategies().len(),
    );
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
                    for index in 0..account.order_strategies().len() {
                        let selected = editor.editing == Some(index);
                        let name = account.order_strategies()[index].name.clone();
                        if ui.selectable_label(selected, name).clicked() {
                            editor.editing = Some(index);
                        }
                    }
                    ui.add_space(4.0);
                    if ui
                        .small_button("+ New")
                        .on_hover_text("start a ladder from one whole-position rung")
                        .clicked()
                    {
                        let fresh = OrderStrategy::starter(account.order_strategies().len());
                        account.add_order_strategy(fresh);
                        editor.added(account.order_strategies().len());
                    }
                    if let Some(index) = editor.editing
                        && ui
                            .small_button("Delete")
                            .on_hover_text("remove this strategy")
                            .clicked()
                    {
                        // The account keeps the selection on the strategy
                        // it named, not on the slot that shifts under it.
                        account.remove_order_strategy(index);
                        editor.removed(index, account.order_strategies().len());
                    }
                });
                ui.separator();
                ui.vertical(|ui| {
                    // Structural edits and typed ones alike: held until
                    // the window closes, so one word is one save.
                    editor.dirty |= draw_strategy_rows(ui, account, editor.editing);
                });
            });
        });
    if !open {
        // Closing is the save point: whatever was typed is what the
        // trader meant, and it reaches disk once.
        changed |= editor.close();
    }
    changed
}

/// The open strategy's name and rungs.
fn draw_strategy_rows(ui: &mut egui::Ui, account: &mut Core, editing: Option<usize>) -> bool {
    let Some(index) = editing else {
        ui.label(
            egui::RichText::new("No strategy yet - New starts one.")
                .color(theme::TEXT_SUPPORT)
                .small(),
        );
        return false;
    };
    let Some(strategy) = account.order_strategy_mut(index) else {
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
        // The new row's share is decided beside the ladder it joins.
        strategy.push_row();
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
