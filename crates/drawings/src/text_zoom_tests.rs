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
fn legacy_notes_bind_once_and_invalid_chart_spacing_preserves_the_reference() {
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
    for zoom in [f32::MIN_POSITIVE, f32::MAX] {
        let size = original.format_at(zoom).font.size_px;
        assert!(size.is_finite() && size > 0.0 && size <= 512.0);
    }
}

#[test]
fn unreadable_optional_zoom_preserves_the_saved_note_style_and_words() {
    let mut saved = SavedDrawing::from_drawing(&placed().items()[0]);
    saved.text = Some("Keep these words".into());
    let style = saved.payload.as_mut().unwrap();
    style["size_px"] = toml::Value::Float(30.0);
    style["appearance"]["bold"] = toml::Value::Boolean(true);
    for bad in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        saved.payload.as_mut().unwrap()["zoom"]["px_per_bar"] = toml::Value::Float(bad);
        let mut drawings = Drawings::default();
        drawings.adopt([saved.to_drawing(DrawingId(1)).unwrap()]);
        let recovered = note(&drawings);
        assert_eq!(recovered.size_px, 30.0);
        assert!(recovered.appearance.bold);
        assert_eq!(recovered.text, "Keep these words");
        assert!(recovered.export_saved().unwrap().get("zoom").is_none());
        drawings.sync_text_zoom(10.0);
        assert_eq!(note(&drawings).format_at(5.0).font.size_px, 15.0);
    }
}

#[test]
fn loaded_legacy_note_binds_on_the_first_visible_layout_or_eye() {
    use quantick_workspace::layout_document::LayoutId;
    let (a, b) = (LayoutId(1), LayoutId(2));
    for (shared, hidden, hide_all) in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mut saved = SavedDrawing::from_drawing(&placed().items()[0]);
        saved
            .payload
            .as_mut()
            .unwrap()
            .as_table_mut()
            .unwrap()
            .remove("zoom");
        saved.points[0].time_ms = Some(1000);
        saved.shared = shared;
        saved.layout = Some(b);
        saved.hidden = hidden;
        let mut drawings = Drawings::default();
        drawings.set_live_layouts([a, b]);
        drawings.set_layout(Some(a));
        drawings.open_all_hidden(hide_all);
        drawings.adopt([saved.to_drawing(DrawingId(1)).unwrap()]);
        let revision = drawings.revision();
        drawings.sync_text_zoom(10.0);
        assert!(!drawings.is_visible(0));
        assert_eq!(drawings.revision(), revision);
        assert!(
            SavedDrawing::from_drawing(&drawings.items()[0])
                .payload
                .unwrap()
                .get("zoom")
                .is_none()
        );
        if shared {
            drawings.set_layout(Some(b));
        }
        if hidden {
            drawings.set_hidden_at(0, false);
        }
        if hide_all {
            drawings.set_all_hidden(false);
        }
        assert!(drawings.is_visible(0));
        drawings.sync_text_zoom(20.0);
        assert_eq!(note(&drawings).format_at(20.0).font.size_px, 12.0);
        let persisted = SavedDrawing::from_drawing(&drawings.items()[0]);
        if hidden || hide_all {
            assert!(drawings.undo());
            assert!(!drawings.is_visible(0));
            drawings.set_hidden_at(0, false);
            drawings.set_all_hidden(false);
            drawings.sync_text_zoom(10.0);
            assert_eq!(note(&drawings).format_at(10.0).font.size_px, 6.0);
        }
        let mut loaded = Drawings::default();
        loaded.adopt([persisted.to_drawing(DrawingId(1)).unwrap()]);
        loaded.sync_text_zoom(10.0);
        assert_eq!(note(&loaded).format_at(10.0).font.size_px, 6.0);
    }
}

#[test]
fn explicit_resize_binds_a_hidden_note_before_the_inspector_gesture_ends() {
    let mut drawings = placed();
    drawings.set_hidden_at(0, true);
    drawings.set_text_spacing(10.0);
    let before = drawings.items()[0].clone();
    let mut edited = before.clone();
    edited.tool.set_glyph_size(&mut edited, 30.0);
    drawings.replace_edited(0, edited);
    assert_eq!(note(&drawings).format_at(5.0).font.size_px, 15.0);
    drawings.set_text_spacing(5.0);
    drawings.record_edit_of(0, before);
    assert_eq!(note(&drawings).format_at(5.0).font.size_px, 15.0);
    drawings.undo();
    assert_eq!(note(&drawings).format_at(5.0).font.size_px, 3.0);
    drawings.redo();
    assert_eq!(note(&drawings).format_at(5.0).font.size_px, 15.0);
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
