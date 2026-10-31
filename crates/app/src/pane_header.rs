//! Paints the header line every pane carries: the rule, the mark and the bars
//! control. What the line says is `quantick_chart::pane_header`'s call.

use eframe::egui;
use egui_phosphor::regular as icons;
use quantick_chart::pane_header::PaneHeader;
use quantick_engine::bar_selection::{BarInputAvailability, BarSelection};

use crate::theme;

/// Padding inside the strip: clears the pane's edge and keeps the controls off
/// the rule below.
pub const CONTENT_PADDING: egui::Vec2 = egui::vec2(8.0, 2.0);

/// The narrowest strip that still gets the mark. The bars control comes first:
/// it is what a hand has to hit.
const MIN_MARK_STRIP_PX: f32 = 190.0;

/// The rule closing the strip against the chart below it.
pub fn paint_rule(ui: &egui::Ui, strip: egui::Rect) {
    ui.painter().line_segment(
        [
            egui::pos2(strip.left(), strip.bottom()),
            egui::pos2(strip.right(), strip.bottom()),
        ],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
}

/// The flow pane's header line.
pub fn draw_marked(
    ui: &mut egui::Ui,
    strip: egui::Rect,
    header: &PaneHeader,
    salt: u64,
    selection: &mut BarSelection,
    inputs: BarInputAvailability,
) {
    paint_rule(ui, strip);
    let mut content = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(("pane_header", salt))
            .max_rect(strip.shrink2(CONTENT_PADDING))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    content.set_clip_rect(strip.intersect(ui.clip_rect()));
    if let Some(mark) = header.mark.filter(|_| strip.width() >= MIN_MARK_STRIP_PX) {
        content.label(
            egui::RichText::new(mark)
                .monospace()
                .small()
                .color(theme::PANE_MARK),
        );
    }
    bars_control(&mut content, header, salt, selection, inputs);
}

/// The bars as words with a faint caret; a click opens this pane's bars menu.
pub fn bars_control(
    ui: &mut egui::Ui,
    header: &PaneHeader,
    salt: u64,
    selection: &mut BarSelection,
    inputs: BarInputAvailability,
) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let mut text = egui::text::LayoutJob::default();
    text.append(
        &header.bars_text,
        0.0,
        egui::TextFormat::simple(font.clone(), theme::TEXT_MUTED),
    );
    text.append(
        icons::CARET_DOWN,
        3.0,
        egui::TextFormat::simple(font, theme::TEXT_FAINT.gamma_multiply(0.6)),
    );
    let control = ui
        .add(egui::Button::new(text).frame(false))
        .on_hover_text("Change this chart's bars");
    let popup = menu_id(salt);
    #[cfg(test)]
    ui.ctx()
        .data_mut(|data| data.insert_temp(popup, control.rect));
    if control.clicked() {
        ui.memory_mut(|memory| memory.toggle_popup(popup));
    }
    egui::popup_below_widget(
        ui,
        popup,
        &control,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_min_width(control.rect.width().max(120.0));
            crate::toolbar::bar_parameters::draw_kinds(ui, selection, inputs);
            ui.separator();
            ui.horizontal(|ui| crate::toolbar::bar_parameters::draw(ui, selection, inputs));
        },
    );
}

/// The popup id of the bars menu of the pane keyed by `salt`.
pub(crate) fn menu_id(salt: u64) -> egui::Id {
    egui::Id::new(("pane_bars_menu", salt))
}
