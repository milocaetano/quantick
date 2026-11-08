//! The entry pair: the two buttons where the trader commits.
//!
//! A view over the ticket that returns what was pressed as an
//! [`EntryPress`]; the ticket fires, arms or disarms after the row is drawn.

use eframe::egui;
use quantick_engine::Side;
use quantick_sim::EntryKind;

use super::super::{PaperTrading, kind_word, side_word_upper};
use crate::paper_chrome::{fmt_decimal, points_color, trade_action_ink};
use crate::theme;
use rust_decimal::Decimal;

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
    let palette = theme::get(ui.ctx());
    let color = palette.side_color(side);
    let armed = ticket.desk.armed;
    let armed_here = armed.is_some_and(|armed| armed.side == side);
    let armed_other = armed.is_some_and(|armed| armed.side != side);
    let enabled = ready && !armed_other;
    let label = match (ticket.desk.ticket.order_type, armed_here) {
        (_, true) => "Click a price…".to_owned(),
        (EntryKind::Market, _) => ticket.entry_label(side),
        (kind, _) => format!(
            "{} {} {}",
            side_word_upper(side),
            kind_word(kind).to_uppercase(),
            ticket
                .desk
                .ticket
                .quantity_preview()
                .map_or_else(String::new, fmt_decimal),
        ),
    };
    let button = if armed_here {
        egui::Button::new(
            egui::RichText::new(&label)
                .color(if palette.resolved.dark {
                    color
                } else {
                    points_color(
                        &palette,
                        if side == Side::Buy {
                            Decimal::ONE
                        } else {
                            -Decimal::ONE
                        },
                    )
                })
                .strong(),
        )
        .fill(palette.control())
        .stroke(egui::Stroke::new(1.5_f32, color))
    } else {
        egui::Button::new(
            egui::RichText::new(&label)
                .color(if !enabled && !palette.resolved.dark {
                    palette.text_primary()
                } else {
                    trade_action_ink(&palette, color)
                })
                .strong(),
        )
        .fill(if !enabled && !palette.resolved.dark {
            palette.control()
        } else {
            color
        })
        .stroke(egui::Stroke::NONE)
    }
    .wrap_mode(egui::TextWrapMode::Wrap)
    .rounding(egui::Rounding::same(3.0))
    .min_size(egui::vec2(width, 34.0));
    let response = ui
        .add_enabled_ui(enabled, |ui| ui.add_sized([width, 34.0], button))
        .inner
        .on_hover_text(ticket.entry_hover(side))
        .on_disabled_hover_text(if armed_other {
            "cancel the armed order first (Esc)"
        } else {
            "waiting for the first print - there is no market yet"
        });
    if !enabled && !palette.resolved.dark {
        crate::paper_chrome::disabled_action_label(ui, &response, &label, egui::TextWrapMode::Wrap);
    }
    #[cfg(test)]
    if side == Side::Buy {
        ui.data_mut(|data| data.insert_temp(egui::Id::new("ticket_buy_response"), response.id));
    }
    if !response.clicked() {
        return None;
    }
    Some(match (ticket.desk.ticket.order_type, armed_here) {
        (_, true) => EntryPress::Disarm,
        (EntryKind::Market, _) => EntryPress::Fire(side),
        (EntryKind::Limit | EntryKind::Stop, _) => EntryPress::Arm(side),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trade_actions_keep_readable_labels() {
        let ctx = egui::Context::default();
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(quantick_appearance::LIGHT).unwrap();
        theme::sync(&ctx, &appearance);
        for ready in [false, true] {
            for armed in [false, true] {
                let mut ticket = PaperTrading::new();
                ticket.desk.ticket.order_type = EntryKind::Limit;
                if armed {
                    ticket.desk.armed = Some(super::super::super::ArmedPlacement {
                        side: Side::Buy,
                        kind: EntryKind::Limit,
                    });
                }
                for state in 0..3 {
                    let hovered = state == 1;
                    ctx.memory_mut(|memory| {
                        if let Some(id) = memory.focused() {
                            memory.surrender_focus(id);
                        }
                    });
                    let mut press = None;
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(320.0, 200.0),
                            )),
                            events: if hovered {
                                vec![egui::Event::PointerMoved(egui::pos2(40.0, 20.0))]
                            } else if state == 2 {
                                vec![
                                    egui::Event::PointerGone,
                                    egui::Event::Key {
                                        key: egui::Key::Tab,
                                        physical_key: None,
                                        pressed: true,
                                        repeat: false,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ]
                            } else {
                                vec![egui::Event::PointerGone]
                            },
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                press = entry_pair(ui, &ticket, ready);
                            });
                        },
                    );
                    assert_eq!(press, None, "painting or hovering never submits an order");
                    if state == 2 {
                        let id = ctx
                            .data(|data| {
                                data.get_temp::<egui::Id>(egui::Id::new("ticket_buy_response"))
                            })
                            .unwrap();
                        assert_eq!(
                            ctx.memory(|memory| memory.has_focus(id)),
                            ready,
                            "BUY focus follows enabled state"
                        );
                    }

                    let shape = output
                        .shapes
                        .iter()
                        .rev()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Text(text)
                                if text.galley.text().starts_with(if armed {
                                    "Click a price"
                                } else {
                                    "BUY"
                                }) =>
                            {
                                Some(text)
                            }
                            _ => None,
                        })
                        .expect("the complete BUY label is painted");
                    let ink = shape
                        .override_text_color
                        .unwrap_or_else(|| shape.galley.rows[0].visuals.mesh.vertices[0].color);
                    let fill = output
                        .shapes
                        .iter()
                        .rev()
                        .find_map(|paint| match &paint.shape {
                            egui::Shape::Rect(rect)
                                if rect.rect.contains(shape.visual_bounding_rect().center())
                                    && rect.fill.a() == 255 =>
                            {
                                Some(rect.fill)
                            }
                            _ => None,
                        })
                        .expect("painted button surface");
                    let a = theme::relative_luminance(ink);
                    let b = theme::relative_luminance(fill);
                    assert!(
                        (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                        "ready={ready} armed={armed} hovered={hovered}: {ink:?}"
                    );
                    assert!(shape.visual_bounding_rect().right() <= 164.0);
                }
                let mut clicked = None;
                for pressed in [true, false] {
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(320.0, 200.0),
                            )),
                            events: vec![
                                egui::Event::PointerMoved(egui::pos2(40.0, 20.0)),
                                egui::Event::PointerButton {
                                    pos: egui::pos2(40.0, 20.0),
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                clicked = entry_pair(ui, &ticket, ready);
                            });
                        },
                    );
                }
                assert_eq!(
                    clicked,
                    if !ready {
                        None
                    } else if armed {
                        Some(EntryPress::Disarm)
                    } else {
                        Some(EntryPress::Arm(Side::Buy))
                    }
                );
            }
        }
    }
}
