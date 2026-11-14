use super::*;
use quantick_chart::flow_execution::FlowQuantityLabel;

#[test]
fn configured_quantity_font_drives_measurement_and_painting_from_one_snapshot() {
    let ctx = egui::Context::default();
    let mut state = quantick_appearance::Appearance::default();
    state.apply("version = 1\nname = 'Quantities'\n[typography.chart_label]\nsize = 22\nfamily = 'monospace'\n[colors]\ntext_primary = '#123456'").unwrap();
    crate::theme::sync(&ctx, &state);
    let appearance = crate::theme::get(&ctx);
    let label = FlowQuantityLabel {
        center: [100.0, 90.0],
        radius: 60.0,
        text: "123.4".into(),
    };
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let galley = quantity_galley(&painter, &label, &appearance);
        let font = &galley.job.sections[0].format.font_id;
        assert_eq!(font.family, egui::FontFamily::Monospace);
        assert_eq!(font.size, 2.0 * super::super::constants::LABEL_MAX_FONT_PX);
        assert_eq!(
            galley.job.sections[0].format.color,
            appearance.text_primary()
        );
        let placed = flow::place_quantity_labels(
            vec![label.clone()],
            [[0.0, 0.0], [240.0, 180.0]],
            [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE],
            |label| quantity_galley(&painter, label, &appearance).size().into(),
        );
        assert_eq!(placed.len(), 1);
        let pos = egui::Pos2::from(placed[0].0);
        assert_eq!(pos + galley.size() / 2.0, egui::Pos2::from(label.center));
        paint_quantity_labels(&painter, painter.clip_rect(), &placed, &appearance);
    });
}

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
        let appearance = crate::theme::get(ctx);
        let fit = [LABEL_MAX_WIDTH_SCALE, LABEL_MAX_HEIGHT_SCALE];
        let measure =
            |label: &FlowQuantityLabel| quantity_galley(&painter, label, &appearance).size().into();
        let area = [history.min.into(), history.max.into()];
        let placed = flow::place_quantity_labels(labels.to_vec(), area, fit, measure);
        paint_quantity_labels(&painter, history, &placed, &appearance);
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
