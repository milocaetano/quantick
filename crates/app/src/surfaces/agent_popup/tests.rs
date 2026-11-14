use super::*;
use crate::surfaces::{HostIntent, Surfaces};

fn frame(
    registry: &mut Surfaces,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            assert_eq!(
                registry.draw_all(ctx, &SurfaceEnv::quiet(std::time::Instant::now())),
                SurfaceResponse::default()
            );
        },
    )
}

fn text_center(output: &egui::FullOutput, wanted: &str) -> Option<egui::Pos2> {
    fn find(shape: &egui::Shape, wanted: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == wanted => {
                Some(text.visual_bounding_rect().center())
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, wanted)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, wanted))
}

/// Locate the actual painted X, whose two diagonal strokes share a center.
fn close_center(output: &egui::FullOutput) -> egui::Pos2 {
    fn collect(shape: &egui::Shape, strokes: &mut Vec<[egui::Pos2; 2]>) {
        match shape {
            egui::Shape::LineSegment { points, .. } => {
                let delta = points[1] - points[0];
                if delta.x.abs() > 1.0 && (delta.x.abs() - delta.y.abs()).abs() < 0.01 {
                    strokes.push(*points);
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, strokes);
                }
            }
            _ => {}
        }
    }
    let mut strokes = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, &mut strokes);
    }
    for first in &strokes {
        for second in &strokes {
            let a = first[1] - first[0];
            let b = second[1] - second[0];
            let center = first[0].lerp(first[1], 0.5);
            if a.x * a.y * b.x * b.y < 0.0 && center.distance(second[0].lerp(second[1], 0.5)) < 0.01
            {
                return center;
            }
        }
    }
    panic!("the populated popup must paint a close X");
}

#[test]
fn painted_dismiss_and_window_close_clear_the_registered_popup() {
    for close_window in [false, true] {
        let ctx = egui::Context::default();
        let mut registry = Surfaces::default();
        assert_eq!(
            registry.update(HostIntent::AgentPopup(Box::new(Intent::Show(AgentPopup {
                title: "Assistant".to_owned(),
                message: "Inspect the latest volume".to_owned(),
                author: "popup regression".to_owned(),
            })))),
            SurfaceResponse::default()
        );
        for _ in 0..3 {
            frame(&mut registry, &ctx, vec![]);
        }
        let painted = frame(&mut registry, &ctx, vec![]);
        assert!(text_center(&painted, "Inspect the latest volume").is_some());
        assert!(text_center(&painted, "Sent by popup regression").is_some());
        let pos = if close_window {
            close_center(&painted)
        } else {
            text_center(&painted, "Dismiss").expect("the populated popup paints Dismiss")
        };
        for pressed in [true, false] {
            frame(
                &mut registry,
                &ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(registry.agent_popup.model_state.pending().is_none());
        let after = frame(&mut registry, &ctx, vec![]);
        assert!(text_center(&after, "Inspect the latest volume").is_none());
        assert!(text_center(&after, "Dismiss").is_none());
    }
}
