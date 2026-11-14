use crate::text::*;
use crate::{ChartPoint, DrawingPayload, DrawingTool, Drawings, FontSpec, TextPayload, UNMEASURED};
use crate::{Px, TextMetrics, pos2, repeat_opening, vec2};

struct Measured;
impl TextMetrics for Measured {
    fn size(&self, _text: &str, _font: FontSpec) -> Px {
        vec2(123.0, 34.0)
    }
}

fn styled() -> TextPayload {
    TextPayload {
        text: "independent note".into(),
        size_px: 27.0,
        family: FontFamily::Monospace,
        appearance: TextAppearance {
            bold: true,
            italic: true,
            background: Some([22, 44, 66, 150]),
            border: Some([200, 210, 220, 255]),
            wrap_width_px: Some(150.0),
        },
        ..Default::default()
    }
}

#[test]
fn measured_bounds_follow_anchor_without_edge_pinning_and_feed_headless_picks() {
    let payload = styled();
    let first = payload.bounds(pos2(200.0, 180.0), &Measured);
    assert_eq!(payload.bounds(pos2(200.0, 180.0), &UNMEASURED), first);
    for anchor in [pos2(-150.0, -20.0), pos2(1500.0, 1200.0)] {
        let moved = payload.bounds(anchor, &UNMEASURED);
        assert_eq!(moved.min - first.min, anchor - pos2(200.0, 180.0));
        assert_eq!(anchor_from_bounds(moved, payload.appearance), anchor);
    }
}

#[test]
fn tool_bounds_dispatch_uses_note_measurement_and_keeps_other_tool_geometry() {
    let payload = styled();
    let anchor = pos2(200.0, 180.0);
    let anchors = crate::PxRect::from_min_max(anchor, anchor);
    let chart = crate::PxRect::from_min_size(Px::ZERO, vec2(800.0, 600.0));
    let measured = payload.bounds(anchor, &Measured);
    let tool = DrawingTool::by_id("text").unwrap();
    assert_eq!(tool.content_bounds(&payload, anchors, chart), measured);
    for tool in crate::DRAWING_TOOLS {
        if tool.id() != "text" {
            assert_eq!(
                tool.content_bounds(tool.default_payload().as_ref(), anchors, chart),
                tool.painted_bounds(anchors, chart),
                "{} retains its painted geometry",
                tool.id(),
            );
        }
    }
}

#[test]
fn measurement_is_derived_and_invalidates_after_text_font_or_wrap_changes() {
    let mut payload = styled();
    let original = payload.clone();
    let measured = payload.bounds(Px::ZERO, &Measured);
    assert_eq!(payload, original, "measurement is not an undoable edit");
    payload.text = "x".into();
    assert_ne!(payload.bounds(Px::ZERO, &UNMEASURED), measured);
    payload.bounds(Px::ZERO, &Measured);
    payload.size_px = 50.0;
    assert_ne!(payload.bounds(Px::ZERO, &UNMEASURED), measured);
    payload.bounds(Px::ZERO, &Measured);
    payload.appearance.wrap_width_px = Some(50.0);
    assert_ne!(payload.bounds(Px::ZERO, &UNMEASURED), measured);
}

#[test]
fn text_style_round_trips_without_words_and_legacy_defaults_stay_plain() {
    let payload = styled();
    let exported = payload.export_preset().unwrap();
    let mut restored = TextPayload::default();
    assert!(restored.import_preset(&exported));
    assert_eq!(restored.appearance, payload.appearance);
    assert_eq!(restored.size_px, payload.size_px);
    assert_eq!(restored.family, payload.family);
    assert!(restored.text.is_empty());
    let old: toml::Value = toml::from_str("size_px = 12.0").unwrap();
    assert!(restored.import_preset(&old));
    assert_eq!(restored, TextPayload::default());
}

#[test]
fn rich_text_survives_a_saved_drawing_document_round_trip() {
    use crate::SavedDrawingExt;
    use quantick_workspace::layout_document::SavedDrawing;

    let mut drawings = Drawings::default();
    drawings.place(
        DrawingTool::by_id("text").unwrap(),
        ChartPoint::at_time(3.5, 123.45, Some(1700000000000)),
    );
    let drawing = drawings.selected_mut().unwrap();
    drawing.payload = Box::new(styled());
    drawing.style.color = crate::rgba_unmultiplied(100, 150, 200, 180);
    let saved = SavedDrawing::from_drawing(drawing);
    let document = toml::to_string(&saved).unwrap();
    let loaded: SavedDrawing = toml::from_str(&document).unwrap();
    let restored = loaded.to_drawing(drawing.id).unwrap();
    assert_eq!(restored.points, drawing.points);
    assert_eq!(restored.style, drawing.style);
    assert_eq!(
        restored.payload.as_any().downcast_ref::<TextPayload>(),
        Some(&styled())
    );
    assert!(!document.contains("extent"), "measurement is derived state");
}

#[test]
fn invalid_appearance_is_rejected_atomically() {
    let mut payload = styled();
    let before = payload.clone();
    for width in [0.0, 39.0, 1201.0, f32::INFINITY, f32::NAN] {
        let mut malformed = before.clone();
        malformed.appearance.wrap_width_px = Some(width);
        let value = malformed.export_preset().unwrap();
        assert!(!payload.import_preset(&value));
        assert_eq!(payload, before);
    }
}

#[test]
fn repeat_opening_preserves_style_not_content_and_undo_restores_styling() {
    let tool = DrawingTool::by_id("text").unwrap();
    let mut drawings = Drawings::default();
    drawings.place(tool, ChartPoint::at(3.0, 10.0));
    let before = drawings.items()[0].clone();
    drawings.selected_mut().unwrap().payload = Box::new(styled());
    drawings.record_edit_of(0, before.clone());
    let opening = repeat_opening(&drawings.items()[0]);
    let copied = opening
        .payload
        .as_any()
        .downcast_ref::<TextPayload>()
        .unwrap();
    assert!(copied.text.is_empty());
    assert_eq!(copied.appearance, styled().appearance);
    drawings.undo();
    assert_eq!(drawings.items()[0], before);
    drawings.redo();
    assert_eq!(
        drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<TextPayload>()
            .unwrap(),
        &styled()
    );
}
