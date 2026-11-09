//! The vocabulary every paper-trading surface shares: how a simulated
//! number, label, colour and journal folder are spelled.
//!
//! This module exists because two modules above it needed the same eleven
//! items and reached for each other to get them. `paper_report` — the
//! reading half — imported them from `paper_trading` — the writing half —
//! while `paper_trading` re-exported the ledger's own types back, and the
//! two became a cycle: neither could be read, moved or tested without the
//! other. The same shape `plot_area` was carved out of `pane` to fix.
//!
//! So the rule here is the one `paper_calendar` and `plot_area` already
//! follow, and it is the whole reason the module is worth its own file:
//!
//! - **It never reaches up.** Nothing here names `PaperTrading`,
//!   `ReportState` or any surface that draws. The imports below are the
//!   proof, and they are meant to stay that short.
//! - **It never reaches sideways either.** `theme` is the only module of
//!   this crate it imports. `paper_calendar` sits *above* it, painting its
//!   day cells with `points_color` and `fmt_signed_points` — which is
//!   exactly why `fmt_offset_minute` and `today` live there rather than
//!   here. Both need `CivilDate`, and hosting them here would have pointed
//!   this module back at the calendar that imports it: one cycle traded for
//!   another, in the module built to prevent it.
//! - **Presentation only.** Nothing here holds session state, opens a
//!   file picker, places an order or reads a clock. [`PositionSummary`] is
//!   the one type, and it is a read-only snapshot the host hands out.
//!
//! `crates/guards/src/cycle.rs` now fails the build if a future edit
//! points one of these modules back at another, which is what stops the
//! third cycle from being found by hand like the first two.

use eframe::egui::{self, Color32};
use quantick_appearance::{Token, paper};
use rust_decimal::Decimal;

use crate::theme;

// The spelling the account itself writes into its acknowledgements and its
// journal moved into `quantick_paper::format`; re-exported so this module stays
// the one vocabulary every paper surface imports.
pub(crate) use quantick_paper::format::{
    PositionSummary, fmt_decimal, fmt_duration_ms, fmt_exact, fmt_points, fmt_signed_points,
    has_signed_figure, position_word, signed_figure,
};
pub(crate) use quantick_paper::report::list_symbol_folders;

/// Uppercase section caption — the ledger's group labels and column header.
pub(crate) fn caption(palette: &crate::theme::Palette, text: &str) -> egui::RichText {
    egui::RichText::new(text)
        .font(palette.body_font(10.0))
        .color(palette.text_faint())
}

/// A pill-shaped toggle chip: accent fill while on, quiet control while off.
pub(crate) fn pill_toggle(ui: &mut egui::Ui, label: &str, on: bool, hover: &str) -> egui::Response {
    let palette = theme::get(ui.ctx());
    let (fill, ink, stroke) = if on {
        (palette.accent(), palette.chip_ink(), egui::Stroke::NONE)
    } else {
        (
            palette.control(),
            palette.text_muted(),
            egui::Stroke::new(1.0_f32, palette.border()),
        )
    };
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(ink).small())
            .fill(fill)
            .stroke(stroke)
            .rounding(egui::Rounding::same(9.0)),
    )
    .on_hover_text(hover)
}

/// Green gains, red losses, muted zero on a panel — shared with the HUD
/// (`paper::pnl_ink`).
pub(crate) fn points_color(palette: &theme::Palette, points: Decimal) -> Color32 {
    points_color_on(palette, Token::Inset, points)
}

/// The same verdict for a figure written straight `on` a surface.
pub(crate) fn points_color_on(palette: &theme::Palette, on: Token, points: Decimal) -> Color32 {
    let sign = points.cmp(&Decimal::ZERO);
    *palette.get(paper::pnl_ink(&palette.resolved, on, sign))
}

/// Financial text keeps its original words; only signed numbers carry outcome
/// ink in Light. In particular a quantity, unit or price is not a gain or loss.
pub(crate) fn financial_text(
    palette: &theme::Palette,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) -> egui::text::LayoutJob {
    if palette.resolved.dark {
        return signed_text(text, font, color, |_| color);
    }
    signed_text(text, font, palette.text_primary(), |value| {
        points_color(palette, value)
    })
}

/// `text` on a dark plate: words in `ink`, signed figures in the plate's gain
/// and loss inks.
pub(crate) fn plate_text(
    palette: &theme::Palette,
    text: &str,
    font: egui::FontId,
    ink: egui::Color32,
) -> egui::text::LayoutJob {
    signed_text(text, font, ink, |value| match value.cmp(&Decimal::ZERO) {
        std::cmp::Ordering::Greater => palette.plate_gain(),
        std::cmp::Ordering::Less => palette.plate_loss(),
        std::cmp::Ordering::Equal => ink,
    })
}

/// `text` in `ink`, each signed number in `signed`'s ink for its value: the
/// spelling of a figure on a surface whose inks are its own, such as a plate.
pub(crate) fn signed_text(
    text: &str,
    font: egui::FontId,
    ink: egui::Color32,
    signed: impl Fn(Decimal) -> egui::Color32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for word in text.split_inclusive(char::is_whitespace) {
        let ink = signed_figure(word).map_or(ink, &signed);
        job.append(
            word,
            0.0,
            egui::TextFormat {
                font_id: font.clone(),
                color: ink,
                ..Default::default()
            },
        );
    }
    job
}

/// BUY and SELL resolve their own ink without changing candles or other chips.
pub(crate) fn trade_action_ink(palette: &theme::Palette, fill: egui::Color32) -> egui::Color32 {
    if !palette.resolved.dark && fill == palette.buy() {
        palette.text_primary()
    } else {
        palette.chip_ink()
    }
}

/// Keep a disabled action legible while its ordinary response still blocks input.
pub(crate) fn disabled_action_label(
    ui: &egui::Ui,
    response: &egui::Response,
    label: &str,
    wrap: egui::TextWrapMode,
) {
    let palette = theme::get(ui.ctx());
    let galley = egui::WidgetText::from(
        egui::RichText::new(label)
            .color(palette.text_primary())
            .strong(),
    )
    .into_galley(
        ui,
        Some(wrap),
        response.rect.width() - 2.0 * ui.spacing().button_padding.x,
        egui::TextStyle::Button,
    );
    ui.painter().galley(
        response.rect.center() - galley.size() / 2.0,
        galley,
        palette.text_primary(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_pnl_ink_preserves_dark_palette() {
        let ctx = egui::Context::default();
        let dark = theme::get(&ctx);
        assert_eq!(points_color(&dark, Decimal::ONE), dark.buy());
        assert_eq!(points_color(&dark, -Decimal::ONE), dark.sell());
        let mut appearance = quantick_appearance::Appearance::default();
        appearance.apply(quantick_appearance::LIGHT).unwrap();
        theme::sync(&ctx, &appearance);
        let light = theme::get(&ctx);
        let candles = (light.buy(), light.sell());
        for light in [light.clone(), theme::bookmap(&ctx)] {
            for side in [quantick_engine::Side::Buy, quantick_engine::Side::Sell] {
                for value in [Decimal::ONE, -Decimal::ONE, Decimal::ZERO] {
                    let ink = points_color(&light, value);
                    // The border is a control's edge, never a surface a
                    // figure is written on; a hovered row lifts onto the tag
                    // grey instead (`hover_fill`).
                    for background in [
                        light.chrome(),
                        light.inset(),
                        light.control(),
                        light.tag_bg(),
                        light.canvas(),
                    ] {
                        let a = theme::relative_luminance(ink);
                        let b = theme::relative_luminance(background);
                        assert!(
                            (a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5,
                            "{side:?} {value}: {ink:?} on {background:?}"
                        );
                    }
                }
            }
        }
        let text = "+24.77 USDT move × 2 · -49.54 USDT · +0 pts @ 75916.41";
        assert_eq!(points_color(&light, Decimal::ONE), light.buy_ink());
        let job = financial_text(&light, text, light.mono_font(12.0), light.buy());
        assert_eq!(
            job.text, text,
            "the calculation remains byte-for-byte complete"
        );
        for section in job.sections {
            let word = job.text[section.byte_range].trim();
            let expected = match word {
                "+24.77" => points_color(&light, Decimal::ONE),
                "-49.54" => points_color(&light, -Decimal::ONE),
                "+0" => light.text_primary(),
                _ => light.text_primary(),
            };
            assert_eq!(section.format.color, expected, "{word}");
        }
        assert_eq!((light.buy(), light.sell()), candles);
        let dark_job = financial_text(&dark, text, dark.mono_font(12.0), dark.sell());
        assert!(
            dark_job
                .sections
                .iter()
                .all(|section| section.format.color == dark.sell())
        );
    }
}
