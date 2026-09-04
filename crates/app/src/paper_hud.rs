//! The persistent position HUD (`docs/ux/ux-audit-2026-08.md` §2): the
//! chart's own answer to "am I in a trade, and which one?".
//!
//! Pinned to the top-left of the pane that owns order entry, visible exactly
//! while a position is open: side, size, average entry, open points, and the
//! two exit affordances (close, reverse). When the entry price sits outside
//! the visible price window a directional caret says which way it left —
//! panning away from the position must never hide that it exists.
//!
//! The pane caches where the HUD anchors (`ChartPane::paper_hud_anchor`);
//! the draw itself happens in `tab.rs`, where the paper host is mutably
//! reachable outside the pane chrome's shared borrow.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_engine::Side;
use rust_decimal::prelude::ToPrimitive;

use crate::chart::PriceScale;
use crate::paper_chrome::{
    PositionSummary, fmt_decimal, fmt_signed_points, points_color, position_word,
};
use crate::paper_trading::PaperTrading;
use crate::theme;

/// Gap between the pane's corner and the HUD card, in pixels.
const HUD_MARGIN_PX: f32 = 8.0;

/// Where the entry price sits relative to the visible price window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryDrift {
    OnScreen,
    /// The entry price is above the window's top edge.
    Above,
    /// The entry price is below the window's bottom edge.
    Below,
}

/// Classify the entry line's screen row against the pane's edges.
fn entry_drift(entry_y: f32, top: f32, bottom: f32) -> EntryDrift {
    if entry_y < top {
        EntryDrift::Above
    } else if entry_y > bottom {
        EntryDrift::Below
    } else {
        EntryDrift::OnScreen
    }
}

/// The caret row's text, `None` while the entry line is on screen.
fn caret_text(summary: &PositionSummary, drift: EntryDrift) -> Option<String> {
    let (glyph, word) = match drift {
        EntryDrift::OnScreen => return None,
        EntryDrift::Above => (icons::CARET_UP, "above"),
        EntryDrift::Below => (icons::CARET_DOWN, "below"),
    };
    Some(format!(
        "{glyph} entry {} {word} the view",
        fmt_decimal(summary.avg_price),
    ))
}

/// Draw the HUD over `chart_rect`. A no-op while flat.
pub fn draw(
    ctx: &egui::Context,
    chart_rect: egui::Rect,
    paper: &mut PaperTrading,
    scale: &PriceScale,
) {
    let Some(summary) = paper.position_summary() else {
        return;
    };
    let side_color = match summary.side {
        Side::Buy => theme::BUY,
        Side::Sell => theme::SELL,
    };
    let entry_y = scale.y(summary.avg_price.to_f64().unwrap_or_default());
    let drift = entry_drift(entry_y, chart_rect.top(), chart_rect.bottom());
    let word = position_word(summary.side);
    let opposite = position_word(match summary.side {
        Side::Buy => Side::Sell,
        Side::Sell => Side::Buy,
    });
    let qty = fmt_decimal(summary.quantity);
    egui::Area::new(egui::Id::new("paper_position_hud"))
        .fixed_pos(chart_rect.left_top() + egui::vec2(HUD_MARGIN_PX, HUD_MARGIN_PX))
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            // A sunken card with a hairline border; the position's side is
            // said once, loudly, by the rail on the left edge and the solid
            // side tag — the chip language of the price gutter.
            let card = egui::Frame::none()
                .fill(theme::INSET)
                .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                .rounding(egui::Rounding::same(4.0))
                .inner_margin(egui::Margin {
                    left: 12.0,
                    right: 10.0,
                    top: 7.0,
                    bottom: 7.0,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 5.0;
                    ui.horizontal(|ui| {
                        egui::Frame::none()
                            .fill(side_color)
                            .rounding(egui::Rounding::same(2.0))
                            .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(format!("SIM {word} {qty}"))
                                        .color(theme::CHIP_INK)
                                        .strong()
                                        .small(),
                                );
                            });
                        ui.label(
                            egui::RichText::new(format!("@ {}", fmt_decimal(summary.avg_price)))
                                .monospace()
                                .color(theme::TEXT_PRIMARY),
                        );
                        if let Some(open) = summary.open_points {
                            ui.label(
                                egui::RichText::new(format!("{} pts", fmt_signed_points(open)))
                                    .monospace()
                                    .strong()
                                    .color(points_color(open)),
                            )
                            .on_hover_text(
                                "open profit at the last print, in points \
                                 (price units × quantity)",
                            );
                        }
                    });
                    if let Some(caret) = caret_text(&summary, drift) {
                        ui.label(
                            egui::RichText::new(caret)
                                .small()
                                .color(theme::TEXT_SUPPORT),
                        );
                    }
                    ui.horizontal(|ui| {
                        let quiet = |label: &str| {
                            egui::Button::new(
                                egui::RichText::new(label)
                                    .color(theme::TEXT_PRIMARY)
                                    .small(),
                            )
                            .fill(theme::CONTROL)
                            .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                            .rounding(egui::Rounding::same(3.0))
                        };
                        if ui
                            .add(quiet("× Close"))
                            .on_hover_text(format!(
                                "exit the {word} {qty} at the next print (market)"
                            ))
                            .clicked()
                        {
                            paper.close_position();
                        }
                        if ui
                            .add(quiet(&format!("{} Reverse", icons::ARROWS_LEFT_RIGHT)))
                            .on_hover_text(format!(
                                "close the {word} {qty} and open {opposite} {qty} \
                                 at the next print"
                            ))
                            .clicked()
                        {
                            paper.reverse_position();
                        }
                    });
                });
            // The side rail: 3 px of the position's colour down the card's
            // left edge, inside the border, rounded with the card.
            let rect = card.response.rect;
            ui.painter().rect_filled(
                egui::Rect::from_min_max(
                    rect.min + egui::vec2(1.0, 1.0),
                    egui::pos2(rect.min.x + 4.0, rect.max.y - 1.0),
                ),
                egui::Rounding {
                    nw: 3.0,
                    sw: 3.0,
                    ne: 0.0,
                    se: 0.0,
                },
                side_color,
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Trade;
    use rust_decimal::Decimal;

    #[test]
    fn drift_reads_the_entry_row_against_the_pane_edges() {
        assert_eq!(entry_drift(50.0, 0.0, 400.0), EntryDrift::OnScreen);
        assert_eq!(entry_drift(-1.0, 0.0, 400.0), EntryDrift::Above);
        assert_eq!(entry_drift(401.0, 0.0, 400.0), EntryDrift::Below);
    }

    #[test]
    fn the_caret_names_the_price_and_the_direction() {
        let summary = PositionSummary {
            side: Side::Buy,
            quantity: Decimal::ONE,
            avg_price: Decimal::new(1055, 1),
            open_points: None,
        };
        assert_eq!(caret_text(&summary, EntryDrift::OnScreen), None);
        let above = caret_text(&summary, EntryDrift::Above).expect("off-screen carets");
        assert!(
            above.contains("105.5") && above.contains("above"),
            "{above}"
        );
        let below = caret_text(&summary, EntryDrift::Below).expect("off-screen carets");
        assert!(below.contains("below"), "{below}");
    }

    fn print(agg_id: u64, price: i64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: i64::try_from(agg_id).expect("small test ids") * 1000,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    /// The HUD reaches real pixels while a position is open, and paints
    /// nothing at all while flat.
    #[test]
    fn the_hud_paints_the_position_and_only_the_position() {
        let ctx = egui::Context::default();
        let chart = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(800.0, 400.0));
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 400.0);
        let mut paper = PaperTrading::new();

        let painted = |paper: &mut PaperTrading| {
            let mut text = String::new();
            for _ in 0..2 {
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    draw(ctx, chart, paper, &scale);
                });
                text.clear();
                for shape in output.shapes {
                    if let egui::epaint::Shape::Text(galley) = shape.shape {
                        text.push_str(galley.galley.text());
                        text.push(' ');
                    }
                }
            }
            text
        };

        assert!(painted(&mut paper).is_empty(), "flat paints no HUD");
        paper.seed(&print(0, 100));
        paper.market(Side::Buy);
        paper.on_trade(&print(1, 100));
        let text = painted(&mut paper);
        assert!(text.contains("SIM LONG 1 @ 100"), "painted: {text}");
        assert!(text.contains("Close"), "painted: {text}");
        assert!(text.contains("Reverse"), "painted: {text}");
    }
}
