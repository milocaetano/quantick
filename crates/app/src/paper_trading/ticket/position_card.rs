//! The position card: the ticket's first block, as a view that decides
//! nothing.
//!
//! [`PositionCard`] reads a snapshot of the position and the ticket's
//! offsets and paints them; every press comes back as a [`CardCommand`] the
//! ticket applies to the account once the card is drawn. The card never
//! reaches the account itself, so what it shows and what a press does are
//! two separate readings a test can hold apart.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_sim::{Command, Position};
use rust_decimal::Decimal;

use super::offset_price;
use crate::paper_chrome::{
    fmt_decimal, fmt_points, fmt_signed_points, points_color, position_word,
};
use crate::theme;

/// What a press on the card asks of the account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CardCommand {
    /// Remove every working order without trading (the flat card's button).
    CancelAllOrders,
    /// A venue command: a bracket edit, a breakeven stop, a partial close.
    Venue(Box<Command>),
    /// Exit the whole position at the next print.
    Close,
    /// Close and open the opposite side at the same size.
    Reverse,
    /// Close the position and cancel every working order.
    Flatten,
}

/// The snapshot the card paints from: the position, if any, and the few
/// account and ticket readings its rows need.
pub(super) struct PositionCard<'a> {
    pub position: Option<&'a Position>,
    pub mark: Option<Decimal>,
    /// The session's realized points, shown on the flat row.
    pub realized: Decimal,
    pub has_working_orders: bool,
    /// The ticket's stop and target offsets, when they parse.
    pub stop_offset: Option<Decimal>,
    pub profit_offset: Option<Decimal>,
}

impl PositionCard<'_> {
    /// A one-row FLAT card while flat (with the session's realized points),
    /// the full card while a position is open — identity, brackets with
    /// their P&L, the R:R read, and the actions.
    ///
    /// A bracket edit is returned before the action rows are drawn, exactly
    /// as it was applied before them when the card wrote the account itself;
    /// only one button can be clicked in a frame, so the last one wins.
    pub(super) fn show(self, ui: &mut egui::Ui) -> Option<CardCommand> {
        let Some(position) = self.position else {
            return self.flat_row(ui);
        };
        let open = self.mark.map(|mark| position.open_points(mark));
        identity_row(ui, position, open);
        let mut command = self
            .brackets(ui, position)
            .map(|c| CardCommand::Venue(Box::new(c)));
        reward_to_risk(ui, position);
        if let Some(action) = actions(ui, position, open) {
            command = Some(action);
        }
        command
    }

    fn flat_row(&self, ui: &mut egui::Ui) -> Option<CardCommand> {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("FLAT")
                    .monospace()
                    .color(theme::TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} pts", fmt_signed_points(self.realized)))
                        .monospace()
                        .strong()
                        .color(points_color(self.realized)),
                )
                .on_hover_text("this session's realized points");
            });
        });
        let cancel = self.has_working_orders
            && ui
                .button("Cancel all orders")
                .on_hover_text("remove every working order without trading (Shift+X)")
                .clicked();
        cancel.then_some(CardCommand::CancelAllOrders)
    }

    /// Brackets: the level, what it pays, ✕ to clear — or the way to set
    /// the missing leg right here, from the ticket's offset.
    fn brackets(&self, ui: &mut egui::Ui, position: &Position) -> Option<Command> {
        let mut bracket_change = None;
        egui::Grid::new("paper_position_brackets")
            .num_columns(3)
            .spacing([8.0, 4.0])
            .show(ui, |ui| {
                let legs = [
                    BracketLeg {
                        stop: true,
                        word: "SL",
                        level: position.stop_loss,
                        color: theme::SELL,
                        offset: self.stop_offset,
                        clear_hover: "remove the protective stop",
                    },
                    BracketLeg {
                        stop: false,
                        word: "TP",
                        level: position.take_profit,
                        color: theme::BUY,
                        offset: self.profit_offset,
                        clear_hover: "remove the profit target",
                    },
                ];
                for leg in legs {
                    if let Some(change) = leg.show(ui, position) {
                        bracket_change = Some(change);
                    }
                    ui.end_row();
                }
            });
        bracket_change
    }
}

/// One bracket row of the grid: the stop or the target.
struct BracketLeg {
    /// The stop leg, not the target.
    stop: bool,
    word: &'static str,
    level: Option<Decimal>,
    color: egui::Color32,
    offset: Option<Decimal>,
    clear_hover: &'static str,
}

impl BracketLeg {
    fn show(&self, ui: &mut egui::Ui, position: &Position) -> Option<Command> {
        ui.label(
            egui::RichText::new(self.word)
                .color(theme::TEXT_MUTED)
                .small(),
        );
        match (self.level, self.offset) {
            (Some(level), _) => {
                ui.label(
                    egui::RichText::new(format!(
                        "{} {} pts",
                        fmt_decimal(level),
                        fmt_signed_points(position.open_points(level)),
                    ))
                    .monospace()
                    .color(self.color),
                );
                let cleared = ui
                    .small_button("×")
                    .on_hover_text(self.clear_hover)
                    .clicked();
                cleared.then(|| self.with_level(position, None))
            }
            (None, Some(offset)) => {
                ui.label(
                    egui::RichText::new("—")
                        .monospace()
                        .color(theme::TEXT_FAINT),
                );
                let set = ui
                    .small_button(format!("Set {} pts", fmt_decimal(offset)))
                    .on_hover_text(
                        "place this leg the ticket's offset away from the \
                         average entry",
                    )
                    .clicked();
                set.then(|| {
                    let level = offset_price(position, offset, self.stop);
                    self.with_level(position, Some(level))
                })
            }
            (None, None) => {
                ui.label(
                    egui::RichText::new("drag from the entry line, or type an offset below")
                        .color(theme::TEXT_SUPPORT)
                        .small(),
                );
                ui.label("");
                None
            }
        }
    }

    /// The bracket with this leg at `level` and the other leg untouched.
    fn with_level(&self, position: &Position, level: Option<Decimal>) -> Command {
        if self.stop {
            Command::SetBracket {
                stop_loss: level,
                take_profit: position.take_profit,
            }
        } else {
            Command::SetBracket {
                stop_loss: position.stop_loss,
                take_profit: level,
            }
        }
    }
}

/// Identity: the HUD's own chip, so the two surfaces read as one.
fn identity_row(ui: &mut egui::Ui, position: &Position, open: Option<Decimal>) {
    let color = theme::side_color(position.side);
    ui.horizontal(|ui| {
        egui::Frame::none()
            .fill(color)
            .rounding(egui::Rounding::same(2.0))
            .inner_margin(egui::Margin::symmetric(5.0, 1.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "SIM {} {}",
                        position_word(position.side),
                        fmt_decimal(position.quantity)
                    ))
                    .color(theme::CHIP_INK)
                    .strong()
                    .small(),
                );
            });
        ui.label(
            egui::RichText::new(format!("@ {}", fmt_decimal(position.avg_price)))
                .monospace()
                .color(theme::TEXT_PRIMARY),
        );
        if let Some(open) = open {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(format!("{} pts", fmt_signed_points(open)))
                        .monospace()
                        .strong()
                        .color(points_color(open)),
                )
                .on_hover_text("open profit at the last print, in points (price units × quantity)");
            });
        }
    });
}

/// The R:R read, while both legs are set and the stop is not at entry.
fn reward_to_risk(ui: &mut egui::Ui, position: &Position) {
    let (Some(stop), Some(target)) = (position.stop_loss, position.take_profit) else {
        return;
    };
    let risk = position.avg_price.saturating_sub(stop).abs();
    let reward = target.saturating_sub(position.avg_price).abs();
    if risk > Decimal::ZERO {
        ui.label(
            egui::RichText::new(format!("R:R {}", fmt_points(reward / risk)))
                .monospace()
                .color(theme::TEXT_MUTED),
        )
        .on_hover_text("reward divided by risk, in points, at the current levels");
    }
}

/// Actions, two per row at equal width; consequential ones stay text-first,
/// never a bare glyph.
fn actions(ui: &mut egui::Ui, position: &Position, open: Option<Decimal>) -> Option<CardCommand> {
    ui.add_space(4.0);
    let half = (ui.available_width() - ui.spacing().item_spacing.x) / 2.0;
    let word = position_word(position.side);
    let qty = fmt_decimal(position.quantity);
    let mut command = None;
    ui.horizontal(|ui| {
        if ui
            .add(quiet_action("× Close", half))
            .on_hover_text(format!("exit the {word} {qty} at the next print (market)"))
            .clicked()
        {
            command = Some(CardCommand::Close);
        }
        if ui
            .add(quiet_action(
                &format!("{} Reverse", icons::ARROWS_LEFT_RIGHT),
                half,
            ))
            .on_hover_text(format!(
                "close the {word} {qty} and open the opposite side at the same size \
                 (Shift+R)"
            ))
            .clicked()
        {
            command = Some(CardCommand::Reverse);
        }
    });
    ui.horizontal(|ui| {
        let in_profit = open.is_some_and(|open| open > Decimal::ZERO);
        if ui
            .add_enabled(in_profit, quiet_action("Breakeven", half))
            .on_hover_text(
                "move the stop to the average entry - with no fees simulated, \
                 break-even is the entry exactly",
            )
            .on_disabled_hover_text(
                "the stop can only move to entry while the position is in profit - \
                 below it, this would widen your risk",
            )
            .clicked()
        {
            command = Some(CardCommand::Venue(Box::new(Command::SetBracket {
                stop_loss: Some(position.avg_price),
                take_profit: position.take_profit,
            })));
        }
        if ui
            .add(quiet_action("Close 50%", half))
            .on_hover_text(
                "close half the open quantity at the next print; the rest keeps \
                 its average entry and brackets",
            )
            .clicked()
        {
            command = Some(CardCommand::Venue(Box::new(Command::ClosePartial {
                quantity: (position.quantity / Decimal::TWO).normalize(),
            })));
        }
    });
    let full = ui.available_width();
    if ui
        .add(quiet_action("Flatten all", full))
        .on_hover_text("close the position and cancel every working order (Shift+F)")
        .clicked()
    {
        command = Some(CardCommand::Flatten);
    }
    command
}

/// A quiet action button — the HUD's control grammar, sized to share a
/// row evenly.
fn quiet_action(label: &str, width: f32) -> egui::Button<'_> {
    egui::Button::new(
        egui::RichText::new(label)
            .color(theme::TEXT_PRIMARY)
            .small(),
    )
    .fill(theme::CONTROL)
    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
    .rounding(egui::Rounding::same(3.0))
    .min_size(egui::vec2(width, 22.0))
}
