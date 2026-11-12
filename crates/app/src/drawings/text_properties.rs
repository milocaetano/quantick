//! Text properties use the same layout as the on-chart note.
use super::{Drawing, StyleColor32};
use eframe::egui;
use quantick_drawings::{
    FontFamily, MAX_TEXT_PX, MIN_TEXT_PX, TextPayload,
    text::{DEFAULT_WRAP_PX, MAX_WRAP_PX, MIN_WRAP_PX},
};

pub(super) fn draw(ui: &mut egui::Ui, drawing: &mut Drawing) -> bool {
    let mut color = drawing.style.color32();
    let Some(payload) = drawing.payload.as_any_mut().downcast_mut::<TextPayload>() else {
        return false;
    };
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui.color_edit_button_srgba(&mut color).changed();
        if ui
            .selectable_label(payload.appearance.bold, egui::RichText::new("B").strong())
            .on_hover_text("Bold")
            .clicked()
        {
            payload.appearance.bold = !payload.appearance.bold;
            changed = true;
        }
        if ui
            .selectable_label(
                payload.appearance.italic,
                egui::RichText::new("I").italics(),
            )
            .on_hover_text("Italic")
            .clicked()
        {
            payload.appearance.italic = !payload.appearance.italic;
            changed = true;
        }
    });
    drawing.style.set_color32(color);
    egui::ComboBox::from_label("Font")
        .selected_text(match payload.family {
            FontFamily::Proportional => "Sans serif",
            FontFamily::Monospace => "Monospace",
        })
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(&mut payload.family, FontFamily::Proportional, "Sans serif")
                .changed();
            changed |= ui
                .selectable_value(&mut payload.family, FontFamily::Monospace, "Monospace")
                .changed();
        });
    changed |= ui
        .add(
            egui::Slider::new(&mut payload.size_px, MIN_TEXT_PX..=MAX_TEXT_PX)
                .text("Size")
                .suffix(" px"),
        )
        .changed();
    let format = payload.format();
    let mut layouter =
        |ui: &egui::Ui, text: &str, _width: f32| super::text::layout(ui.ctx(), text, format, color);
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut payload.text)
                .font(super::text::font_id(format))
                .layouter(&mut layouter)
                .desired_rows(3)
                .hint_text("Add text")
                .desired_width(f32::INFINITY),
        )
        .changed();
    changed |= optional_color(
        ui,
        "Background",
        &mut payload.appearance.background,
        [80, 100, 120, 160],
    );
    changed |= optional_color(
        ui,
        "Border",
        &mut payload.appearance.border,
        [110, 130, 150, 255],
    );
    let mut wrap = payload.appearance.wrap_width_px.is_some();
    if ui.checkbox(&mut wrap, "Text wrap").changed() {
        payload.appearance.wrap_width_px = wrap.then_some(DEFAULT_WRAP_PX);
        changed = true;
    }
    if let Some(width) = &mut payload.appearance.wrap_width_px {
        changed |= ui
            .add(
                egui::Slider::new(width, MIN_WRAP_PX..=MAX_WRAP_PX)
                    .text("Width")
                    .suffix(" px"),
            )
            .changed();
    }
    changed
}

fn optional_color(
    ui: &mut egui::Ui,
    label: &str,
    rgba: &mut Option<[u8; 4]>,
    initial: [u8; 4],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let mut enabled = rgba.is_some();
        if ui.checkbox(&mut enabled, label).changed() {
            *rgba = enabled.then_some(initial);
            changed = true;
        }
        if let Some(value) = rgba {
            changed |= ui.color_edit_button_srgba_unmultiplied(value).changed();
        }
    });
    changed
}
