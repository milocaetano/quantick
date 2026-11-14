//! FLOW's quantity labels: placed once per drawing, laid out and painted each frame.
use super::*;

/// A placed quantity label: where its galley goes, and the label.
pub(super) type PlacedLabel = ([f32; 2], FlowQuantityLabel);

/// `plan`'s quantity labels, fitted to their spheres and placed in `history` apart.
pub(super) fn placed_labels(
    (frame, plan): (&FlowTapeFrame, &FlowPresentation),
    tick_look: Option<quantick_orderflow::TickLook>,
    (painter, history): (&egui::Painter, egui::Rect),
    appearance: &crate::theme::Palette,
) -> Vec<PlacedLabel> {
    let labels = plan.quantity_labels(frame, tick_look.map(|(tick, _)| tick));
    let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
    let measure =
        |label: &FlowQuantityLabel| quantity_galley(painter, label, appearance).size().into();
    let area = [history.min.into(), history.max.into()];
    let peaks = frame.view.region.divides_at_peaks();
    flow::place_quantity_labels_until_miss(labels, area, fit, measure, peaks)
}

impl FlowDrawing {
    /// Labels finish the candle pass, so no candle contour crosses a quantity.
    pub fn paint_labels(&self, painter: &egui::Painter) {
        paint_quantity_labels(painter, self.history, &self.labels, &self.appearance);
    }
}

fn paint_quantity_labels(
    painter: &egui::Painter,
    history: egui::Rect,
    labels: &[PlacedLabel],
    appearance: &crate::theme::Palette,
) {
    let clip = painter.with_clip_rect(history);
    for (pos, label) in labels {
        let pos = egui::Pos2::from(*pos);
        // Laid out each frame (egui's galley cache answers): a galley kept across
        // frames would outlive a font atlas egui recreates.
        let galley = quantity_galley(painter, label, appearance);
        let shadow = appearance.quantity_shadow();
        let offset = super::constants::LABEL_SHADOW_OFFSET_PX;
        clip.galley_with_override_text_color(pos + offset, galley.clone(), shadow);
        clip.galley(pos, galley, appearance.text_primary());
    }
}

fn quantity_galley(
    painter: &egui::Painter,
    label: &FlowQuantityLabel,
    appearance: &crate::theme::Palette,
) -> Arc<egui::Galley> {
    use super::constants::{LABEL_FONT_SCALE, LABEL_MAX_FONT_PX, LABEL_MIN_FONT_PX};
    let font = (label.radius * LABEL_FONT_SCALE).clamp(LABEL_MIN_FONT_PX, LABEL_MAX_FONT_PX);
    let font = appearance.label_font(font);
    painter.layout_no_wrap(label.text.clone(), font, appearance.text_primary())
}

#[cfg(test)]
mod label_tests;
