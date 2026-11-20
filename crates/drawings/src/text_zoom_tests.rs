use crate::*;
use quantick_workspace::layout_document::SavedDrawing;

fn note(drawings: &Drawings<DrawingTool>) -> &TextPayload {
    drawings.items()[0].payload.as_any().downcast_ref().unwrap()
}

fn placed() -> Drawings<DrawingTool> {
    let mut drawings = Drawings::default();
    drawings.sync_text_zoom(20.0);
    drawings.place(
        DrawingTool::by_id("text").unwrap(),
        ChartPoint::at(3.5, 100.0),
    );
    drawings
}

#[test]
fn zoom_is_relative_to_size_selection_and_undo_restores_the_reference() {
    let mut drawings = placed();
    let original = drawings.items()[0].clone();
    let revision = drawings.revision();
    let depth = drawings.undo_depth();
    drawings.sync_text_zoom(10.0);
    assert_eq!(note(&drawings).format_at(10.0).font.size_px, 6.0);
    assert_eq!(note(&drawings).format_at(20.0).font.size_px, 12.0);
    assert_eq!(drawings.revision(), revision);
    assert_eq!(drawings.undo_depth(), depth);
    let tool = drawings.items()[0].tool;
    tool.set_glyph_size(&mut drawings.items_mut()[0], 30.0);
    drawings.record_edit_of(0, original.clone());
    assert_eq!(note(&drawings).format_at(10.0).font.size_px, 30.0);
    assert_eq!(note(&drawings).format_at(5.0).font.size_px, 15.0);
    drawings.undo();
    assert_eq!(drawings.items()[0], original);
    assert_eq!(note(&drawings).format_at(10.0).font.size_px, 6.0);
    drawings.redo();
    assert_eq!(note(&drawings).format_at(10.0).font.size_px, 30.0);
    assert_eq!(drawings.items()[0].points, original.points);
}

#[test]
fn persisted_reference_survives_reload_but_does_not_travel_with_presets() {
    let drawings = placed();
    let saved = SavedDrawing::from_drawing(&drawings.items()[0]);
    let serialized = toml::to_string(&saved).unwrap();
    let saved: SavedDrawing = toml::from_str(&serialized).unwrap();
    let restored = saved.to_drawing(DrawingId(99)).unwrap();
    let payload = restored
        .payload
        .as_any()
        .downcast_ref::<TextPayload>()
        .unwrap();
    assert_eq!(payload.format_at(10.0).font.size_px, 6.0);
    let mut preset = TextPayload::default();
    assert!(preset.import_preset(&payload.export_preset().unwrap()));
    assert_eq!(preset.format_at(10.0).font.size_px, 12.0);
    assert_eq!(restored.points, drawings.items()[0].points);
}

#[test]
fn legacy_notes_bind_once_and_invalid_zoom_never_corrupts_saved_text() {
    let mut drawings = Drawings::default();
    drawings.place(
        DrawingTool::by_id("text").unwrap(),
        ChartPoint::at(2.0, 10.0),
    );
    let revision = drawings.revision();
    for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        drawings.sync_text_zoom(zoom);
        assert_eq!(note(&drawings).format_at(zoom).font.size_px, 12.0);
    }
    assert_eq!(drawings.revision(), revision);
    drawings.sync_text_zoom(8.0);
    assert!(drawings.revision() > revision);
    assert_eq!(note(&drawings).format_at(8.0).font.size_px, 12.0);
    assert_eq!(note(&drawings).format_at(4.0).font.size_px, 6.0);
    let original = note(&drawings).clone();
    for bad in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        let mut payload = original.clone();
        let mut saved = payload.export_saved().unwrap();
        saved["zoom"]["px_per_bar"] = toml::Value::Float(bad);
        assert!(!payload.import_saved(&saved));
        assert_eq!(payload, original);
    }
    for zoom in [f32::MIN_POSITIVE, f32::MAX] {
        let size = original.format_at(zoom).font.size_px;
        assert!(size.is_finite() && size > 0.0 && size <= 512.0);
    }
}

struct Metrics;
impl TextMetrics for Metrics {
    fn size(&self, text: &str, font: FontSpec) -> Px {
        vec2(text.len() as f32 * font.size_px, font.size_px)
    }
}

#[test]
fn paint_editor_bounds_and_hit_testing_share_zoomed_layout() {
    let mut drawings = placed();
    let drawing = &mut drawings.items_mut()[0];
    let payload = drawing
        .payload
        .as_any_mut()
        .downcast_mut::<TextPayload>()
        .unwrap();
    payload.text = "Daily high".into();
    let tool = drawing.tool;
    let scale = quantick_chart::geometry::PriceScale::from_range(0.0, 200.0, 0.0, 400.0);
    let anchor = pos2(200.0, 200.0);
    let chart = PxRect::from_min_size(Px::ZERO, vec2(800.0, 400.0));
    for rich in [false, true] {
        let payload = drawing
            .payload
            .as_any_mut()
            .downcast_mut::<TextPayload>()
            .unwrap();
        payload.appearance.background = rich.then_some([20, 30, 40, 255]);
        payload.appearance.wrap_width_px = rich.then_some(240.0);
        for zoom in [20.0, 10.0, 20.0] {
            let context = DrawContext {
                appearance: quantick_appearance::ResolvedTheme::bundled_dark(),
                payload,
                anchors: &drawing.points,
                scale: &scale,
                px_per_bar: zoom,
                unit: ValueUnit::Price,
                primary_band: true,
                style: drawing.style,
                selected: false,
                halo: false,
                content_editing: false,
                text: &Metrics,
            };
            let mut scene = MarkScene::default();
            tool.paint(&mut scene, chart, drawing.style, &[anchor], &context, false);
            let painted = scene
                .shapes
                .iter()
                .find_map(|shape| match shape {
                    MarkShape::Text { font, .. } => Some(font.size_px),
                    MarkShape::NoteText { format, .. } => Some(format.font.size_px),
                    _ => None,
                })
                .unwrap();
            assert_eq!(painted, 12.0 * zoom / 20.0);
            let bounds = payload.bounds_at(anchor, &Metrics, zoom);
            assert_eq!(
                tool.content_bounds(payload, PxRect::from_min_max(anchor, anchor), chart, zoom),
                bounds
            );
            assert_eq!(text::anchor_from_bounds(bounds, payload.appearance), anchor);
            assert!(tool.hit_test(chart, &[anchor], bounds.center(), 0.0, &context));
            assert!(!tool.hit_test(
                chart,
                &[anchor],
                bounds.max + vec2(10.0, 10.0),
                0.0,
                &context
            ));
        }
    }
}
