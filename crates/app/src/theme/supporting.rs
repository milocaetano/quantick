//! Local treatments for enabled supporting text and editable preset names.
use eframe::egui;

/// Preserve Dark's authored weak ink; Light's enabled hints need opaque ink.
pub fn supporting_text(ui: &egui::Ui, text: impl Into<String>) -> egui::RichText {
    let text = egui::RichText::new(text);
    let palette = super::get(ui.ctx());
    if palette.resolved.dark {
        text.weak()
    } else {
        text.color(palette.text_support())
    }
}

/// Consequential prose uses the body role and wraps even inside horizontal rows.
pub fn supporting_label(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    let palette = super::get(ui.ctx());
    let ink = if palette.resolved.dark {
        palette.text_muted()
    } else {
        palette.text_support()
    };
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .color(ink)
                .text_style(egui::TextStyle::Body),
        )
        .wrap(),
    )
}

/// A local border advertises editing without changing any disabled-state opacity.
pub fn preset_input(ui: &mut egui::Ui, edit: egui::TextEdit<'_>) -> egui::Response {
    let palette = super::get(ui.ctx());
    ui.scope(|ui| {
        if !palette.resolved.dark {
            let border = egui::Stroke::new(1.0_f32, palette.text_support());
            ui.visuals_mut().widgets.inactive.bg_stroke = border;
            ui.visuals_mut().widgets.hovered.bg_stroke = border;
        }
        ui.add(edit)
    })
    .inner
}

#[cfg(test)]
mod tests;
