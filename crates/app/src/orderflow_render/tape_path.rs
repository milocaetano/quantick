//! The quiet price path behind tape-only volume dots.

use eframe::egui;

/// A muted hairline keeps execution order readable behind the volume discs.
const PATH_WIDTH_PX: f32 = 0.8;
const PATH_RGBA: [u8; 4] = [142, 166, 177, 95];

/// Connect factual centres, including offscreen vertices, behind the discs.
/// The painter clips each segment; culling vertices would invent a shortcut.
pub(super) fn draw(painter: &egui::Painter, points: impl Iterator<Item = egui::Pos2>) {
    let [r, g, b, a] = PATH_RGBA;
    let stroke = egui::Stroke::new(
        PATH_WIDTH_PX,
        egui::Color32::from_rgba_unmultiplied(r, g, b, a),
    );
    let mut points: Vec<_> = points.collect();
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then_with(|| a.y.total_cmp(&b.y)));
    let mut previous = None;
    for point in points {
        if let Some(from) = previous {
            painter.line_segment([from, point], stroke);
        }
        previous = Some(point);
    }
}
