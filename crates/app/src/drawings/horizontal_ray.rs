use eframe::egui;
use egui_phosphor::regular as icons;

use super::line_core::{LINES_FAMILY, single_level};
use super::{AxisLevels, DrawContext, DrawingStyle, DrawingToolImpl, ToolFamily, drawing_stroke};

pub(super) static TOOL: HorizontalRay = HorizontalRay;

pub(super) struct HorizontalRay;

/// The painted span: from the anchor bar to the right edge. A level only
/// becomes a level once it has been touched, so a horizontal ray says
/// "from here on" — which is exactly what a horizontal line cannot say.
fn span(chart_rect: egui::Rect, point: egui::Pos2) -> (egui::Pos2, egui::Pos2) {
    (
        egui::pos2(point.x.min(chart_rect.right()), point.y),
        egui::pos2(chart_rect.right(), point.y),
    )
}

impl DrawingToolImpl for HorizontalRay {
    /// From its anchor to the right edge: the drawing runs forward to the
    /// live edge. See [`DrawingToolImpl::painted_bounds`].
    fn painted_bounds(&self, anchors: egui::Rect, chart: egui::Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            anchors.min,
            egui::pos2(chart.right(), anchors.bottom()),
        )
    }
    fn id(&self) -> &'static str {
        "horizontal-ray"
    }
    fn name(&self) -> &'static str {
        "Horizontal ray"
    }
    fn settings_title(&self) -> &'static str {
        "Horizontal ray settings"
    }
    fn icon(&self) -> &'static str {
        icons::ARROW_LINE_RIGHT
    }
    fn hover_text(&self) -> &'static str {
        "Horizontal ray - a level that starts at the bar you click"
    }
    fn required_points(&self) -> usize {
        1
    }
    fn family(&self) -> Option<ToolFamily> {
        Some(LINES_FAMILY)
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
        _ctxt: &DrawContext<'_>,
    ) {
        if let Some(point) = points.first() {
            let (from, to) = span(chart_rect, *point);
            painter.line_segment([from, to], drawing_stroke(style));
        }
    }
    /// The price it names, tagged on the axis like the horizontal line's —
    /// the two say the same kind of thing and differ only in where they start.
    ///
    /// And that difference is why this one asks about the rect: a ray runs
    /// from its anchor to the right edge, so an anchor that has been panned
    /// past that edge leaves [`span`] a zero-length stroke and nothing on the
    /// canvas. A chip on the gutter then would be the axis marking a level
    /// whose line is gone, which is exactly what the tags must not do.
    fn axis_levels(&self, chart_rect: egui::Rect, points: &[egui::Pos2]) -> AxisLevels {
        match points.first() {
            Some(point) if point.x < chart_rect.right() => single_level(points),
            _ => AxisLevels::new(),
        }
    }
    fn hit_test(
        &self,
        chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
        _ctxt: &DrawContext<'_>,
    ) -> bool {
        points.first().is_some_and(|point| {
            let (from, to) = span(chart_rect, *point);
            position.x >= from.x - radius_px
                && position.x <= to.x
                && (position.y - point.y).abs() <= radius_px
        })
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (vec![egui::pos2(200.0, 150.0)], egui::pos2(400.0, 152.0))
    }
}
