//! The entry pair: the two buttons where the trader commits.
//!
//! A view over the ticket that returns what was pressed as an
//! [`EntryPress`]; the ticket fires, arms or disarms after the row is drawn.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::EntryKind;

use super::super::{PaperTrading, kind_word, side_word_upper};
use crate::paper_chrome::fmt_decimal;
use crate::theme;

/// What a press on the entry pair asks of the ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryPress {
    /// A market entry on this side, now.
    Fire(Side),
    /// Arm a resting entry on this side: the next chart click places it.
    Arm(Side),
    /// Cancel the armed entry.
    Disarm,
}

/// The entry pair, taller than the toolbar's buttons. An armed side
/// inverts — a mode you are in must be visible on the control that put you
/// there. `ready` is false before the first print and while the risk lock
/// blocks entry.
pub(super) fn entry_pair(
    ui: &mut egui::Ui,
    ticket: &PaperTrading,
    ready: bool,
) -> Option<EntryPress> {
    let half = (ui.available_width() - 6.0) / 2.0;
    let mut press = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for side in [Side::Buy, Side::Sell] {
            if let Some(pressed) = entry_button(ui, ticket, side, ready, half) {
                press = Some(pressed);
            }
        }
    });
    press
}

fn entry_button(
    ui: &mut egui::Ui,
    ticket: &PaperTrading,
    side: Side,
    ready: bool,
    width: f32,
) -> Option<EntryPress> {
    let color = theme::side_color(side);
    let armed = ticket.account.armed;
    let armed_here = armed.is_some_and(|armed| armed.side == side);
    let armed_other = armed.is_some_and(|armed| armed.side != side);
    let label = match (ticket.order_type, armed_here) {
        (_, true) => "Click a price…".to_owned(),
        (EntryKind::Market, _) => ticket.entry_label(side),
        (kind, _) => format!(
            "{} {} {}",
            side_word_upper(side),
            kind_word(kind).to_uppercase(),
            ticket
                .quantity_preview()
                .map_or_else(String::new, fmt_decimal),
        ),
    };
    let button = if armed_here {
        egui::Button::new(egui::RichText::new(label).color(color).strong())
            .fill(theme::CONTROL)
            .stroke(egui::Stroke::new(1.5_f32, color))
    } else {
        egui::Button::new(egui::RichText::new(label).color(theme::CHIP_INK).strong())
            .fill(color)
            .stroke(egui::Stroke::NONE)
    }
    .rounding(egui::Rounding::same(3.0))
    .min_size(egui::vec2(width, 34.0));
    let response = ui
        .add_enabled(ready && !armed_other, button)
        .on_hover_text(ticket.entry_hover(side))
        .on_disabled_hover_text(if armed_other {
            "cancel the armed order first (Esc)"
        } else {
            "waiting for the first print - there is no market yet"
        });
    if !response.clicked() {
        return None;
    }
    Some(match (ticket.order_type, armed_here) {
        (_, true) => EntryPress::Disarm,
        (EntryKind::Market, _) => EntryPress::Fire(side),
        (EntryKind::Limit | EntryKind::Stop, _) => EntryPress::Arm(side),
    })
}
