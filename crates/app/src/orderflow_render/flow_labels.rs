//! FLOW's quantity labels: placed once per drawing, laid out and painted each frame.
use super::*;

/// A placed quantity label: where its galley goes, and the label.
pub(super) type PlacedLabel = ([f32; 2], FlowQuantityLabel);

/// `plan`'s quantity labels as `tick` sizes them, each fitted to its sphere by
/// its galley and placed in `history` apart from the others.
pub(super) fn placed_labels(
    (frame, plan): (&FlowTapeFrame, &FlowPresentation),
    tick: Option<quantick_orderflow::TickBubbleStyle>,
    (painter, history): (&egui::Painter, egui::Rect),
) -> Vec<PlacedLabel> {
    let labels = plan.quantity_labels(frame, tick);
    let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
    let measure = |label: &FlowQuantityLabel| quantity_galley(painter, label).size().into();
    let area = [history.min.into(), history.max.into()];
    flow::place_quantity_labels(labels, area, fit, measure)
}

impl FlowDrawing {
    /// Labels finish the candle pass, so no candle contour crosses a quantity.
    pub fn paint_labels(&self, painter: &egui::Painter) {
        paint_quantity_labels(painter, self.history, &self.labels);
    }
}

fn paint_quantity_labels(painter: &egui::Painter, history: egui::Rect, labels: &[PlacedLabel]) {
    let clip = painter.with_clip_rect(history);
    for (pos, label) in labels {
        let pos = egui::Pos2::from(*pos);
        // Laid out each frame (egui's galley cache answers): a galley kept across
        // frames would outlive a font atlas egui recreates.
        let galley = quantity_galley(painter, label);
        let shadow = egui::Color32::from_black_alpha(super::constants::LABEL_SHADOW_ALPHA);
        let offset = super::constants::LABEL_SHADOW_OFFSET_PX;
        clip.galley_with_override_text_color(pos + offset, galley.clone(), shadow);
        clip.galley(pos, galley, crate::theme::TEXT_PRIMARY);
    }
}

fn quantity_galley(painter: &egui::Painter, label: &FlowQuantityLabel) -> Arc<egui::Galley> {
    use super::constants::{LABEL_FONT_SCALE, LABEL_MAX_FONT_PX, LABEL_MIN_FONT_PX};
    let font = (label.radius * LABEL_FONT_SCALE).clamp(LABEL_MIN_FONT_PX, LABEL_MAX_FONT_PX);
    let font = egui::FontId::proportional(font);
    painter.layout_no_wrap(label.text.clone(), font, crate::theme::TEXT_PRIMARY)
}

#[cfg(test)]
mod label_tests {
    use super::*;
    use quantick_chart::flow_execution::FlowQuantityLabel;

    #[test]
    fn tick_quantities_use_fitted_nonoverlapping_galleys() {
        let ctx = egui::Context::default();
        let history = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 180.0));
        let labels = [
            FlowQuantityLabel {
                center: [100.0, 90.0],
                radius: 30.0,
                text: "28.1".into(),
            },
            FlowQuantityLabel {
                center: [105.0, 90.0],
                radius: 20.0,
                text: "14.7".into(),
            },
            FlowQuantityLabel {
                center: [180.0, 90.0],
                radius: 15.0,
                text: "7.2".into(),
            },
            FlowQuantityLabel {
                center: [0.0, 90.0],
                radius: 15.0,
                text: "7.2".into(),
            },
        ];
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
            let measure =
                |label: &FlowQuantityLabel| quantity_galley(&painter, label).size().into();
            let area = [history.min.into(), history.max.into()];
            let placed = flow::place_quantity_labels(labels.to_vec(), area, fit, measure);
            paint_quantity_labels(&painter, history, &placed);
        });
        let bounds: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.override_text_color.is_none() => {
                    Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            bounds.len(),
            2,
            "the overlapping and clipped labels are omitted"
        );
        assert!(!bounds[0].intersects(bounds[1]));
        assert!(bounds.iter().all(|bounds| history.contains_rect(*bounds)));
    }
}
