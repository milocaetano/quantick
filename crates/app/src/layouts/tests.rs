use super::*;
use crate::drawings::{DrawingId, DrawingScope, StyleColor32};
fn level(price: f64) -> SavedDrawing {
    SavedDrawing {
        tool: "horizontal-line".to_owned(),
        name: Some("max".to_owned()),
        author: None,
        points: vec![SavedPoint {
            time_ms: Some(1_000),
            price,
            bar: 3.0,
        }],
        band: SavedBand::Price,
        color: [255, 200, 0, 255],
        width_px: 1.5,
        fill_alpha: 0,
        locked: false,
        hidden: false,
        magnet: false,
        shared: false,
        payload: None,
        text: None,
        id: None,
        durable_identity: None,
        origin: None,
        bars: None,
    }
}

#[test]
fn a_text_note_keeps_its_words() {
    let mut saved = level(1.0);
    saved.tool = "text".to_owned();
    saved.text = Some("congestion 108k".to_owned());
    let drawing = saved.to_drawing(DrawingId(1)).expect("the text tool");
    assert_eq!(
        drawing.tool.inline_text(drawing.payload.as_ref()),
        Some("congestion 108k")
    );
    assert_eq!(
        SavedDrawing::from_drawing(&drawing).text.as_deref(),
        Some("congestion 108k")
    );
}

#[test]
fn a_drawing_survives_the_file_form_and_back() {
    let saved = level(101.5);
    let drawing = saved.to_drawing(DrawingId(7)).expect("a known tool");
    assert_eq!(drawing.id, DrawingId(7));
    let mut with_id = saved.clone();
    with_id.id = Some(7);
    assert_eq!(drawing.points[0].price, 101.5);
    assert_eq!(drawing.points[0].time_ms, Some(1_000));
    assert_eq!(drawing.name.as_deref(), Some("max"));
    assert_eq!(drawing.scope, DrawingScope::ThisChart);
    assert_eq!(
        SavedDrawing::from_drawing(&drawing),
        with_id,
        "the id travels"
    );

    let mut unknown = level(1.0);
    unknown.tool = "a-tool-from-the-future".to_owned();
    assert!(unknown.to_drawing(DrawingId(1)).is_none());
}

#[test]
fn a_drawings_own_magnet_survives_the_file_form_and_back() {
    let off = level(1.0).to_drawing(DrawingId(1)).unwrap();
    assert!(!off.magnet, "a file without the flag opens with it off");
    let mut saved = level(1.0);
    saved.magnet = true;
    let drawing = saved.to_drawing(DrawingId(1)).unwrap();
    assert!(drawing.magnet);
    assert!(SavedDrawing::from_drawing(&drawing).magnet);
}

#[test]
fn a_shared_drawing_keeps_its_scope() {
    let mut saved = level(1.0);
    saved.shared = true;
    let drawing = saved.to_drawing(DrawingId(1)).unwrap();
    assert_eq!(drawing.scope, DrawingScope::AllCharts);
    assert!(SavedDrawing::from_drawing(&drawing).shared);
}

/// A migrated drawing still says where it came from after a pass through
/// the live object, and one drawn since says nothing.
#[test]
fn a_drawings_origin_survives_the_file_form_and_back() {
    assert!(
        level(1.0)
            .to_drawing(DrawingId(1))
            .unwrap()
            .origin
            .is_none()
    );
    let mut saved = level(1.0);
    saved.origin = Some(SavedOrigin {
        layout: "Fluxo".to_owned(),
        feed: "metatrader-b3".to_owned(),
        symbol: "WINV26".to_owned(),
        pane: 2,
        id: Some(7),
        fingerprint: 0,
    });
    let drawing = saved.to_drawing(DrawingId(1)).unwrap();
    assert_eq!(
        drawing.origin.as_ref().unwrap().label(),
        "from Fluxo · WINV26"
    );
    assert_eq!(SavedDrawing::from_drawing(&drawing).origin, saved.origin);
}

/// Both historical files still load with their original painted colours.
/// Further saves must not repeat their old double-premultiplication bug.
#[test]
fn legacy_drawings_keep_their_loaded_colour_across_further_saves() {
    const SAVED: &str = include_str!("../../fixtures/layouts/drawings.toml");
    const RESAVED: &str = include_str!("../../fixtures/layouts/drawings-resaved.toml");
    let dir = crate::scratch::ScratchDir::new("layouts-colour");
    let path = dir.join(LAYOUTS_FILE);

    for fixture in [SAVED, RESAVED] {
        let mut book = parse(fixture).unwrap();
        save(&path, &book);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), fixture);

        let saved = book.take_legacy_drawings().remove(0).items;
        assert_eq!(saved.len(), 3, "an opaque and two translucent colours");
        for entry in saved {
            let drawing = entry.to_drawing(DrawingId(entry.id.unwrap())).unwrap();
            let [r, g, b, a] = entry.color;
            assert_eq!(
                drawing.style.color32(),
                eframe::egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                "the painted colour is the one egui made from the historical file"
            );
            let resaved = SavedDrawing::from_drawing(&drawing);
            let reloaded = resaved.to_drawing(drawing.id).unwrap();
            assert_eq!(reloaded, drawing, "saving preserves the loaded drawing");
            assert_eq!(SavedDrawing::from_drawing(&reloaded), resaved);
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
