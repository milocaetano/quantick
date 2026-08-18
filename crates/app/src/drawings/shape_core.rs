//! The closed-outline geometry the Shapes family shares.
//!
//! Rectangle, ellipse and triangle are the same object to a trader: an
//! outline with an optional interior, selectable by its border always and by
//! its middle only while that interior is visible. That last rule is the one
//! worth sharing — get it wrong in one tool and an invisible fill starts
//! swallowing clicks the chart should have received.

use eframe::egui;
use egui_phosphor::regular as icons;

use super::{
    DrawContext, DrawingStyle, ToolFamily, distance_to_segment, drawing_fill, drawing_stroke,
};

/// The one rail family every closed shape declares.
pub(super) const SHAPES_FAMILY: ToolFamily = ToolFamily {
    id: "shapes",
    title: "Shapes",
    icon: icons::SHAPES,
    icon_strokes: &[],
    icon_dots: &[],
};

/// Segments in an ellipse outline. Fixed, not derived from the on-screen
/// size: the paint path must not change shape as the trader zooms, and 48
/// segments are already sub-pixel on a full-height ellipse.
const ELLIPSE_SEGMENTS: usize = 48;

/// The ellipse inscribed in the box spanned by two corners, as a closed
/// polygon in winding order.
pub(super) fn ellipse_outline(corners: [egui::Pos2; 2]) -> Vec<egui::Pos2> {
    let rect = egui::Rect::from_two_pos(corners[0], corners[1]);
    let centre = rect.center();
    let radius = rect.size() / 2.0;
    (0..ELLIPSE_SEGMENTS)
        .map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / ELLIPSE_SEGMENTS as f32;
            egui::pos2(
                centre.x + radius.x * angle.cos(),
                centre.y + radius.y * angle.sin(),
            )
        })
        .collect()
}

/// Paint a closed outline with the drawing's fill and stroke. The fill is a
/// separate pass so the halo (which carries `fill_alpha: 0`) paints only the
/// outline, exactly like every other tool.
pub(super) fn paint_outline(painter: &egui::Painter, style: DrawingStyle, outline: &[egui::Pos2]) {
    if outline.len() < 3 {
        return;
    }
    if style.fill_alpha > 0 {
        painter.add(egui::Shape::convex_polygon(
            outline.to_vec(),
            drawing_fill(style),
            egui::Stroke::NONE,
        ));
    }
    let stroke = drawing_stroke(style);
    let mut closed: Vec<egui::Pos2> = outline.to_vec();
    closed.push(outline[0]);
    painter.add(egui::Shape::line(closed, stroke));
}

/// Whether `position` is inside a closed polygon — the even-odd ray cast.
fn inside(outline: &[egui::Pos2], position: egui::Pos2) -> bool {
    let mut inside = false;
    let mut previous = outline[outline.len() - 1];
    for &current in outline {
        let crosses = (current.y > position.y) != (previous.y > position.y);
        if crosses {
            let span = previous.y - current.y;
            if span.abs() > f32::EPSILON {
                let x = current.x + (position.y - current.y) / span * (previous.x - current.x);
                if position.x < x {
                    inside = !inside;
                }
            }
        }
        previous = current;
    }
    inside
}

/// Hit-test a closed outline: the border always, the interior only while the
/// fill is actually visible. An outline-only shape lets clicks through its
/// middle reach the chart, which is what makes a big zone usable at all.
pub(super) fn hit_outline(
    outline: &[egui::Pos2],
    position: egui::Pos2,
    radius_px: f32,
    ctxt: &DrawContext<'_>,
) -> bool {
    if outline.len() < 3 {
        return false;
    }
    let on_border = outline
        .iter()
        .zip(outline.iter().cycle().skip(1))
        .any(|(start, end)| distance_to_segment(position, *start, *end) <= radius_px);
    on_border || (ctxt.style.fill_alpha > 0 && inside(outline, position))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::PriceScale;
    use crate::drawings::{NoPayload, ValueUnit};

    fn with_fill<R>(fill_alpha: u8, body: impl FnOnce(&DrawContext<'_>) -> R) -> R {
        let payload = NoPayload;
        let scale = PriceScale::from_range(0.0, 300.0, 0.0, 300.0);
        let style = DrawingStyle {
            fill_alpha,
            ..DrawingStyle::default()
        };
        let ctxt = DrawContext {
            payload: &payload,
            anchors: &[],
            scale: &scale,
            px_per_bar: 20.0,
            unit: ValueUnit::Price,
            primary_band: true,
            style,
            selected: false,
            halo: false,
            content_editing: false,
        };
        body(&ctxt)
    }

    #[test]
    fn an_ellipse_outline_is_inscribed_in_its_box() {
        let corners = [egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0)];
        let outline = ellipse_outline(corners);
        let box_rect = egui::Rect::from_two_pos(corners[0], corners[1]).expand(0.01);
        assert_eq!(outline.len(), ELLIPSE_SEGMENTS);
        assert!(outline.iter().all(|point| box_rect.contains(*point)));
        // It touches all four sides: an inscribed ellipse, not a smaller one.
        let widest = outline
            .iter()
            .fold(f32::MIN, |widest, point| widest.max(point.x));
        assert!((widest - 300.0).abs() < 0.01);
    }

    #[test]
    fn an_unfilled_shape_lets_clicks_through_its_middle() {
        let outline = ellipse_outline([egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0)]);
        let middle = egui::pos2(200.0, 150.0);
        assert!(!with_fill(0, |ctxt| hit_outline(
            &outline, middle, 4.0, ctxt
        )));
        assert!(with_fill(20, |ctxt| hit_outline(
            &outline, middle, 4.0, ctxt
        )));
    }

    #[test]
    fn the_border_is_always_selectable() {
        let outline = ellipse_outline([egui::pos2(100.0, 100.0), egui::pos2(300.0, 200.0)]);
        let on_border = egui::pos2(300.0, 150.0);
        assert!(with_fill(0, |ctxt| hit_outline(
            &outline, on_border, 4.0, ctxt
        )));
    }
}
