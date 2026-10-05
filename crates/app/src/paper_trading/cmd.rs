//! The cmd-trading block of the ticket: the enable pill, the two key
//! bindings, the entry kind and the ruler's step.
//!
//! Widgets only. What a held key aims, which kind may rest where and how
//! far a notch walks are `quantick_paper::desk`'s; this draws the settings
//! that steer them and says, beside them, what they will do.

use eframe::egui;
use quantick_paper::PaperAccount as Core;
use quantick_paper::desk::Ruler;
use rust_decimal::Decimal;

use super::{CmdEntryKind, CmdModifier, CmdTradingSettings};
use crate::paper_chrome::{caption, fmt_decimal, pill_toggle};
use crate::theme;

/// The cmd-trading block of the ticket. Returns whether anything changed,
/// so the host can persist and fan out.
pub(super) fn draw_cmd_trading_settings(
    ui: &mut egui::Ui,
    settings: &mut CmdTradingSettings,
    ruler: &mut Ruler,
    account: &Core,
) -> bool {
    let mut changed = false;
    ui.add_space(4.0);
    ui.label(caption("CMD TRADING"));
    ui.horizontal(|ui| {
        if pill_toggle(
            ui,
            "Enabled",
            settings.enabled,
            "hold a key over the chart: a dashed line shows exactly where the order \
             will rest, and the click places it",
        )
        .clicked()
        {
            settings.enabled = !settings.enabled;
            changed = true;
        }
        for (word, slot) in [("Buy", true), ("Sell", false)] {
            ui.label(egui::RichText::new(word).color(theme::TEXT_MUTED).small());
            let current = if slot { settings.buy } else { settings.sell };
            egui::ComboBox::from_id_salt(("cmd_trading_modifier", word))
                .width(64.0)
                .selected_text(current.label())
                .show_ui(ui, |ui| {
                    for modifier in CmdModifier::ALL {
                        if ui
                            .selectable_label(current == modifier, modifier.label())
                            .clicked()
                            && current != modifier
                        {
                            if slot {
                                settings.buy = modifier;
                            } else {
                                settings.sell = modifier;
                            }
                            changed = true;
                        }
                    }
                });
        }
    });
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Place")
                .color(theme::TEXT_MUTED)
                .small(),
        );
        let current = settings.kind;
        egui::ComboBox::from_id_salt("cmd_trading_entry_kind")
            .width(64.0)
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for kind in CmdEntryKind::ALL {
                    if ui.selectable_label(current == kind, kind.label()).clicked()
                        && current != kind
                    {
                        settings.kind = kind;
                        changed = true;
                    }
                }
            })
            .response
            .on_hover_text(
                "which order the aim places - auto takes whichever kind can rest at \
                 the price, limit and stop place only that one and show nothing where \
                 it cannot rest",
            );
        ui.label(egui::RichText::new("Step").color(theme::TEXT_MUTED).small());
        // The empty field shows this instrument's own default as a hint,
        // so blank reads as "follows the instrument" rather than as
        // nothing. A tick is what the ladder speaks, and saying what one
        // is worth here is the only place the ticket ever does.
        let derived = account.derived_ruler_step();
        let tick = account.tick();
        let response = ui.add(
            egui::TextEdit::singleline(&mut ruler.step_text)
                .desired_width(52.0)
                .hint_text(fmt_decimal(derived)),
        );
        if response.changed() {
            let typed = ruler.step_text.trim();
            let step = if typed.is_empty() {
                None
            } else {
                typed.parse::<Decimal>().ok()
            };
            ruler.set_step(account.symbol(), step);
            changed = true;
        }
        response.on_hover_text(format!(
            "how far one wheel notch walks the aim's stop and target, in points of \
             this instrument. One tick here is {}, so this instrument defaults to \
             {} a notch. Empty follows that default; saved per symbol.",
            fmt_decimal(tick),
            fmt_decimal(derived),
        ));
        ui.label(egui::RichText::new("pts").color(theme::TEXT_FAINT).small());
    });
    if settings.enabled && settings.kind != CmdEntryKind::Auto {
        // A stated kind is valid on one side of the market only, so the
        // aim is silent on the other half of the chart. Said here, or a
        // trader spends a minute wondering why the gesture died.
        ui.label(
            egui::RichText::new(format!(
                "the aim shows only where a {} can rest: {} the market",
                settings.kind.label(),
                if settings.kind == CmdEntryKind::Limit {
                    "below it to buy, above it to sell"
                } else {
                    "above it to buy, below it to sell"
                },
            ))
            .color(theme::TEXT_SUPPORT)
            .small(),
        );
    }
    if settings.enabled && settings.buy == settings.sell {
        // A shared key is ambiguous, so the gesture shows nothing —
        // said here rather than discovered over the chart.
        ui.label(
            egui::RichText::new(
                "buy and sell share a key - the gesture stays hidden until they differ",
            )
            .color(theme::AMBER)
            .small(),
        );
    } else if settings.enabled {
        // The gesture is invisible until a key is held; its one line
        // of instructions lives where the toggle does, not in a
        // tooltip a newcomer never hovers.
        ui.label(
            egui::RichText::new(format!(
                "hold {} over the chart to buy, {} to sell - the dashed line shows \
                 where, and the click places it. Roll the wheel while holding to walk \
                 a stop and target out from the aim; roll back to zero, or press the \
                 wheel, to leave the aim as it was",
                settings.buy.label(),
                settings.sell.label(),
            ))
            .color(theme::TEXT_SUPPORT)
            .small(),
        );
    }
    changed
}
