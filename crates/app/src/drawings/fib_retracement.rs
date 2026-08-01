use eframe::egui;
use egui_phosphor::regular as icons;

use super::{
    DrawingStyle, DrawingToolImpl, FibGeometry, FibLevel, drawing_stroke, hit_fib_levels,
    paint_fib_levels,
};

pub(super) static TOOL: FibRetracement = FibRetracement;
const LEVELS: [FibLevel; 7] = [
    FibLevel::new(0.0, "0.000"),
    FibLevel::new(0.236, "0.236"),
    FibLevel::new(0.382, "0.382"),
    FibLevel::new(0.5, "0.500"),
    FibLevel::new(0.618, "0.618"),
    FibLevel::new(0.786, "0.786"),
    FibLevel::new(1.0, "1.000"),
];

pub(super) struct FibRetracement;

impl DrawingToolImpl for FibRetracement {
    fn id(&self) -> &'static str {
        "fib-retracement"
    }
    fn settings_title(&self) -> &'static str {
        "Fib retracement settings"
    }
    fn icon(&self) -> &'static str {
        icons::ROWS
    }
    fn hover_text(&self) -> &'static str {
        "Fib retracement - click two points or drag"
    }
    fn required_points(&self) -> usize {
        2
    }
    fn paint(
        &self,
        painter: &egui::Painter,
        _chart_rect: egui::Rect,
        style: DrawingStyle,
        points: &[egui::Pos2],
    ) {
        if points.len() == 2 {
            paint_fib_levels(
                painter,
                FibGeometry {
                    left: points[0].x.min(points[1].x),
                    right: points[0].x.max(points[1].x),
                    first_y: points[0].y,
                    second_y: points[1].y,
                    origin_y: points[0].y,
                },
                &LEVELS,
                drawing_stroke(style),
                style.color,
            );
        }
    }
    fn hit_test(
        &self,
        _chart_rect: egui::Rect,
        points: &[egui::Pos2],
        position: egui::Pos2,
        radius_px: f32,
    ) -> bool {
        points.len() == 2
            && hit_fib_levels(position, points, &LEVELS, points[0].y, radius_px)
    }

    #[cfg(test)]
    fn test_geometry(&self) -> (Vec<egui::Pos2>, egui::Pos2) {
        (
            vec![egui::pos2(100.0, 100.0), egui::pos2(250.0, 200.0)],
            egui::pos2(175.0, 150.0),
        )
    }
}
