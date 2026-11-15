use super::FakePane;
use crate::drawing_text::*;
use quantick_control::wire::WireU64;
use quantick_drawings::{ChartPoint, DrawingTool, FontFamily, TextPayload};

fn note() -> (FakePane, TextTarget) {
    let mut pane = FakePane::new(70);
    pane.drawings.place_with(
        DrawingTool::by_id("text").unwrap(),
        &quantick_drawings::DrawingBand::Price,
        ChartPoint::at_time(0.5, 10.0, Some(1000)),
        |tool| quantick_drawings::NewDrawing {
            style: tool.default_style(),
            payload: tool.default_payload(),
        },
    );
    let id = pane.drawings.items()[0].id.0;
    (
        pane,
        TextTarget {
            tab_id: WireU64::new(7),
            pane_id: WireU64::new(70),
            drawing_id: WireU64::new(id),
        },
    )
}

fn properties() -> TextProperties {
    TextProperties {
        size_px: quantick_control::wire::CanonicalDecimal::new("31.5").unwrap(),
        family: Family::Monospace,
        bold: true,
        italic: true,
        color: [100, 150, 200, 255],
        background: Some([20, 30, 40, 180]),
        border: Some([80, 90, 100, 255]),
        wrap_width_px: Some(quantick_control::wire::CanonicalDecimal::new("140.25").unwrap()),
    }
}

#[test]
fn text_set_validates_atomically_and_inspects_without_exposing_words() {
    let (mut pane, target) = note();
    let input = SetInput {
        target: target.clone(),
        properties: Some(properties()),
        text: Some("private note".into()),
        anchor: Some(TextAnchor {
            time_unix_ms: 2000,
            price: quantick_control::wire::CanonicalDecimal::new("21.5").unwrap(),
        }),
    };
    let result = set(&mut pane, &input).unwrap();
    assert!(result.changed);
    assert_eq!(result.properties, properties());
    assert_eq!(
        pane.drawings.items()[0].points[0],
        ChartPoint::at_time(1.5, 21.5, Some(2000))
    );
    assert!(
        !serde_json::to_string(&result)
            .unwrap()
            .contains("private note")
    );
    assert!(!set(&mut pane, &input).unwrap().changed);
    let before = pane.drawings.items()[0].clone();
    let mut invalid = input.clone();
    invalid.properties.as_mut().unwrap().size_px =
        quantick_control::wire::CanonicalDecimal::new("1000").unwrap();
    assert!(set(&mut pane, &invalid).is_err());
    assert_eq!(pane.drawings.items()[0], before);
    pane.drawings.undo();
    assert_eq!(
        pane.drawings.items()[0]
            .payload
            .as_any()
            .downcast_ref::<TextPayload>()
            .unwrap(),
        &TextPayload::default()
    );
    pane.drawings.redo();
    assert_eq!(pane.drawings.items()[0], before);
    pane.drawings.selected_mut().unwrap().locked = true;
    assert!(set(&mut pane, &input).is_err());
    assert!(
        !set(
            &mut pane,
            &SetInput {
                target,
                properties: None,
                text: None,
                anchor: None
            }
        )
        .unwrap()
        .changed
    );
}

#[test]
fn text_repeat_copies_style_without_words_and_is_one_undo_step() {
    let (mut pane, source) = note();
    set(
        &mut pane,
        &SetInput {
            target: source.clone(),
            properties: Some(properties()),
            text: Some("do not copy".into()),
            anchor: None,
        },
    )
    .unwrap();
    let original = pane.drawings.items()[0].clone();
    let input = RepeatInput {
        source,
        anchor: TextAnchor {
            time_unix_ms: 3000,
            price: quantick_control::wire::CanonicalDecimal::new("25").unwrap(),
        },
        text: None,
    };
    let result = repeat(&mut pane, &input, None).unwrap();
    assert_ne!(result.target.drawing_id.get(), original.id.0);
    assert_eq!(pane.drawings.items()[0], original);
    let payload = pane.drawings.items()[1]
        .payload
        .as_any()
        .downcast_ref::<TextPayload>()
        .unwrap();
    assert!(payload.text.is_empty());
    assert_eq!(payload.family, FontFamily::Monospace);
    assert_eq!(result.properties, properties());
    pane.drawings.undo();
    assert_eq!(pane.drawings.items(), &[original]);
    pane.drawings.redo();
    assert_eq!(pane.drawings.items().len(), 2);
    assert!(
        repeat(
            &mut pane,
            &RepeatInput {
                anchor: TextAnchor {
                    time_unix_ms: 0,
                    price: quantick_control::wire::CanonicalDecimal::new("20").unwrap()
                },
                ..input
            },
            None
        )
        .is_err()
    );
    assert_eq!(pane.drawings.items().len(), 2);
}
