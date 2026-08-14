//! The geometry every straight-line tool shares.
//!
//! Trend line, ray, extended line, horizontal ray, vertical line and arrow
//! are one shape with two questions attached: how far does it run past its
//! anchors, and does it end in a head. Keeping that in one place means the
//! seven tools in the Lines family are seven declarations, not seven
//! copies of the same segment maths — and a fix to the hit-test fixes all
//! of them at once.

use eframe::egui;
use egui_phosphor::regular as icons;

use super::{Constrain, DrawingStyle, ToolFamily, distance_to_segment, drawing_stroke, level_with};

/// The far end of a two-point line while the trader holds Shift: level with
/// the end it started from, and still free to slide along the tape.
///
/// Shared here rather than written out per tool, because "a line held level"
/// is one rule and every tool built on this core wants the same one. A tool
/// adopts it by forwarding its shaping port to this and nothing else, which
/// is what keeps the gesture meaning the same thing everywhere it exists.
pub(super) fn levelled_far_end(
    placed: &[egui::Pos2],
    cursor: egui::Pos2,
    constrain: Constrain,
) -> egui::Pos2 {
    match (placed, constrain) {
        ([start], Constrain::Level) => level_with(*start, cursor),
        _ => cursor,
    }
}

/// The one rail family every straight-line tool declares. Shared here so
/// the members cannot drift apart on the title or the pre-arm icon.
pub(super) const LINES_FAMILY: ToolFamily = ToolFamily {
    id: "lines",
    title: "Lines",
    icon: icons::LINE_SEGMENT,
    icon_strokes: &[],
};

/// How far a line runs past the anchors that define it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Extend {
    /// A segment: it stops where the trader put it.
    None,
    /// A ray: past the second anchor to the chart edge.
    Forward,
    /// A ray the other way: past the first anchor to the chart edge. Not a
    /// tool of its own — the channel's "extend left" is expressed with it.
    Backward,
    /// An infinite line: past both anchors to the chart edge.
    Both,
}

/// Arrowhead length as a multiple of the stroke width, floored so a hairline
/// still gets a head you can see.
const ARROWHEAD_WIDTH_FACTOR: f32 = 6.0;
const ARROWHEAD_MIN_PX: f32 = 8.0;
/// Half-angle of the arrowhead, in radians (~24°).
const ARROWHEAD_HALF_ANGLE: f32 = 0.42;

/// The largest step from `origin` along `dir` that stays inside `rect`.
///
/// `dir` need not be normalised. A direction with no component on an axis
/// simply never bounds `t` on it; a line that cannot reach any edge (a zero
/// direction) stays at its origin rather than running to infinity.
fn edge_step(origin: egui::Pos2, dir: egui::Vec2, rect: egui::Rect) -> f32 {
    const EPSILON: f32 = 1e-4;
    let mut step = f32::INFINITY;
    if dir.x > EPSILON {
        step = step.min((rect.right() - origin.x) / dir.x);
    } else if dir.x < -EPSILON {
        step = step.min((rect.left() - origin.x) / dir.x);
    }
    if dir.y > EPSILON {
        step = step.min((rect.bottom() - origin.y) / dir.y);
    } else if dir.y < -EPSILON {
        step = step.min((rect.top() - origin.y) / dir.y);
    }
    if step.is_finite() { step.max(0.0) } else { 0.0 }
}

/// The two screen points a line with this extension actually runs between.
/// `None` while the draft still has fewer than two anchors, or when both
/// anchors landed on the same pixel (nothing to draw a direction from).
pub(super) fn line_ends(
    chart_rect: egui::Rect,
    points: &[egui::Pos2],
    extend: Extend,
) -> Option<(egui::Pos2, egui::Pos2)> {
    let (&start, &end) = match points {
        [start, end, ..] => (start, end),
        _ => return None,
    };
    let direction = end - start;
    if direction.length_sq() <= f32::EPSILON {
        return Some((start, end));
    }
    let head = match extend {
        Extend::None | Extend::Backward => end,
        Extend::Forward | Extend::Both => end + direction * edge_step(end, direction, chart_rect),
    };
    let tail = match extend {
        Extend::None | Extend::Forward => start,
        Extend::Backward | Extend::Both => {
            start - direction * edge_step(start, -direction, chart_rect)
        }
    };
    Some((tail, head))
}

/// Paint a straight line, optionally ending in an arrowhead at `points[1]`.
///
/// The head is skipped on the halo pass: a halo is a widened copy of the
/// stroke, and painting the head twice would blunt it into a blob.
pub(super) fn paint_line(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    style: DrawingStyle,
    points: &[egui::Pos2],
    extend: Extend,
    arrowhead: bool,
) {
    let Some((tail, head)) = line_ends(chart_rect, points, extend) else {
        return;
    };
    let stroke = drawing_stroke(style);
    painter.line_segment([tail, head], stroke);
    if !arrowhead {
        return;
    }
    let direction = head - tail;
    if direction.length_sq() <= f32::EPSILON {
        return;
    }
    let back =
        -direction.normalized() * (style.width_px * ARROWHEAD_WIDTH_FACTOR).max(ARROWHEAD_MIN_PX);
    let (sin, cos) = ARROWHEAD_HALF_ANGLE.sin_cos();
    for wing in [
        egui::vec2(back.x * cos - back.y * sin, back.x * sin + back.y * cos),
        egui::vec2(back.x * cos + back.y * sin, -back.x * sin + back.y * cos),
    ] {
        painter.line_segment([head, head + wing], stroke);
    }
}

/// Hit-test a straight line with this extension.
pub(super) fn hit_line(
    chart_rect: egui::Rect,
    points: &[egui::Pos2],
    position: egui::Pos2,
    radius_px: f32,
    extend: Extend,
) -> bool {
    line_ends(chart_rect, points, extend)
        .is_some_and(|(tail, head)| distance_to_segment(position, tail, head) <= radius_px)
}

/// A single-anchor line running across the whole chart on one axis.
/// Horizontal line and vertical line differ only in which axis they pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Axis {
    Horizontal,
    Vertical,
}

pub(super) fn axis_ends(
    chart_rect: egui::Rect,
    point: egui::Pos2,
    axis: Axis,
) -> (egui::Pos2, egui::Pos2) {
    match axis {
        Axis::Horizontal => (
            egui::pos2(chart_rect.left(), point.y),
            egui::pos2(chart_rect.right(), point.y),
        ),
        Axis::Vertical => (
            egui::pos2(point.x, chart_rect.top()),
            egui::pos2(point.x, chart_rect.bottom()),
        ),
    }
}

/// Shared paint for the single-anchor axis lines.
pub(super) fn paint_axis(
    painter: &egui::Painter,
    chart_rect: egui::Rect,
    style: DrawingStyle,
    points: &[egui::Pos2],
    axis: Axis,
) {
    if let Some(point) = points.first() {
        let (from, to) = axis_ends(chart_rect, *point, axis);
        painter.line_segment([from, to], drawing_stroke(style));
    }
}

/// Shared hit-test for the single-anchor axis lines. The cross-axis bound
/// keeps a line selectable only where it is actually painted.
pub(super) fn hit_axis(
    chart_rect: egui::Rect,
    points: &[egui::Pos2],
    position: egui::Pos2,
    radius_px: f32,
    axis: Axis,
) -> bool {
    points.first().is_some_and(|point| match axis {
        Axis::Horizontal => {
            chart_rect.x_range().contains(position.x) && (position.y - point.y).abs() <= radius_px
        }
        Axis::Vertical => {
            chart_rect.y_range().contains(position.y) && (position.x - point.x).abs() <= radius_px
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0))
    }

    #[test]
    fn a_segment_stops_at_its_anchors() {
        let points = [egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)];
        assert_eq!(
            line_ends(chart(), &points, Extend::None),
            Some((points[0], points[1]))
        );
    }

    #[test]
    fn a_ray_reaches_the_chart_edge_forward_only() {
        let points = [egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)];
        let (tail, head) = line_ends(chart(), &points, Extend::Forward).expect("two anchors");
        assert_eq!(tail, points[0], "a ray keeps its origin anchor");
        // Slope 1 from (200,200): the bottom edge at y = 600 comes before the
        // right edge at x = 800.
        assert!(
            (head.y - 600.0).abs() < 0.01,
            "head stops on the bottom edge"
        );
        assert!((head.x - 600.0).abs() < 0.01);
    }

    #[test]
    fn an_extended_line_reaches_both_edges() {
        let points = [egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)];
        let (tail, head) = line_ends(chart(), &points, Extend::Both).expect("two anchors");
        assert!((tail.x - 0.0).abs() < 0.01 && (tail.y - 0.0).abs() < 0.01);
        assert!((head.x - 600.0).abs() < 0.01 && (head.y - 600.0).abs() < 0.01);
    }

    /// A pan can drag both anchors onto the same pixel; the tool must not
    /// divide by zero or run the line to infinity.
    #[test]
    fn coincident_anchors_do_not_produce_an_infinite_line() {
        let points = [egui::pos2(100.0, 100.0), egui::pos2(100.0, 100.0)];
        for extend in [
            Extend::None,
            Extend::Forward,
            Extend::Backward,
            Extend::Both,
        ] {
            assert_eq!(
                line_ends(chart(), &points, extend),
                Some((points[0], points[1])),
                "{extend:?} must degenerate to the anchors themselves"
            );
        }
    }

    /// Extending is a *view* affordance: the anchors never move, so undo,
    /// coordinates and the shared-anchor projection all keep working on the
    /// two points the trader actually placed.
    #[test]
    fn extending_never_moves_the_anchors() {
        let points = [egui::pos2(300.0, 400.0), egui::pos2(500.0, 100.0)];
        for extend in [Extend::Forward, Extend::Backward, Extend::Both] {
            let before = points;
            let _ = line_ends(chart(), &points, extend);
            assert_eq!(points, before);
        }
    }

    #[test]
    fn a_ray_is_selectable_past_its_second_anchor() {
        let points = [egui::pos2(100.0, 100.0), egui::pos2(200.0, 200.0)];
        let past = egui::pos2(400.0, 400.0);
        assert!(!hit_line(chart(), &points, past, 4.0, Extend::None));
        assert!(hit_line(chart(), &points, past, 4.0, Extend::Forward));
    }

    #[test]
    fn a_vertical_line_is_only_selectable_inside_the_chart() {
        let points = [egui::pos2(400.0, 300.0)];
        assert!(hit_axis(
            chart(),
            &points,
            egui::pos2(402.0, 10.0),
            4.0,
            Axis::Vertical
        ));
        assert!(
            !hit_axis(
                chart(),
                &points,
                egui::pos2(402.0, 900.0),
                4.0,
                Axis::Vertical
            ),
            "below the chart there is no line to click"
        );
    }
}
