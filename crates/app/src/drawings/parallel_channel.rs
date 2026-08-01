use eframe::egui;
use egui_phosphor::regular as icons;

use super::{DrawingStyle, DrawingToolImpl, distance_to_segment, drawing_fill, drawing_stroke};

pub(super) static TOOL: ParallelChannel = ParallelChannel;

pub(super) struct ParallelChannel;

fn channel_offset(points: &[egui::Pos2]) -> egui::Vec2 {
    let baseline = points[1] - points[0];
    let to_width_anchor = points[2] - points[0];
    let baseline_length_sq = baseline.length_sq();
    if baseline_length_sq <= f32::EPSILON {
        return to_width_anchor;
    }
    to_width_anchor - baseline * (to_width_anchor.dot(baseline) / baseline_length_sq)
}

impl DrawingToolImpl for ParallelChannel {
    fn id(&self) -> &'static str {
        "parallel-channel"
    }
    fn name(&self) -> &'static str {
        "Parallel channel"
    }
    fn settings_title(&self) -> &'static str {
        "Parallel channel settings"
    }
    fn icon(&self) -> &'static str {
        icons::PARALLELOGRAM
    }
    fn hover_text(&self) -> &'static str {
        "Parallel channel - set the baseline, then click the channel width"
    }
    fn required_points(&self) -> usize {
        3
    }
    fn supports_fill(&self) -> bool {
        true
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        _chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
    ) {
        let stroke = drawing_stroke(style);
        if points.len() >= 2 {
            painter.line_segment([points[0], points[1]], stroke);
        }
        if points.len() == 3 {
            let offset = channel_offset(points);
            if style.fill_alpha > 0 {
                painter.add(egui::Shape::convex_polygon(
                    vec![points[0], points[1], points[1] + offset, points[0] + offset],
                    drawing_fill(style),
                    egui::Stroke::NONE,
                ));
            }
            painter.line_segment([points[0] + offset, points[1] + offset], stroke);
            painter.line_segment([points[0], points[0] + offset], stroke);
            painter.line_segment([points[1], points[1] + offset], stroke);
        }
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
    ) -> bool {
        if points.len() != 3 {
            return false;
        }
        let offset = channel_offset(points);
        [
            (points[0], points[1]),
            (points[0] + offset, points[1] + offset),
            (points[0], points[0] + offset),
            (points[1], points[1] + offset),
        ]
        .into_iter()
        .any(|(start, end)| distance_to_segment(position, start, end) <= radius_px)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![
                egui::pos2(100.0, 100.0),
                egui::pos2(200.0, 120.0),
                egui::pos2(100.0, 160.0),
            ],
            egui::pos2(150.0, 170.0),
        )
    }
}
