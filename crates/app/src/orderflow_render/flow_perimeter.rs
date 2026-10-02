//! FLOW silhouettes inside the earned radius, above candle bodies.
use super::flow_execution::{FLOW_BUY, FLOW_SELL, FlowDisc, flow_disc};
use eframe::egui::{
    self,
    epaint::{Vertex, WHITE_UV},
};
use quantick_orderflow::{
    HeatmapConfig,
    config::dressing::inner_perimeter_quads,
    projection::flow_tape::{FlowTapeDot, FlowTapeFrame},
};

pub(crate) fn draw_flow_perimeters(
    painter: &egui::Painter,
    history: egui::Rect,
    frame: &FlowTapeFrame,
    _config: &HeatmapConfig,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) {
    let clip = painter.with_clip_rect(history);
    for dot in frame.dots.iter().filter(|dot| dot.opening_oversized) {
        let Some(disc) = center(dot)
            .and_then(|at| flow_disc(dot, at))
            .filter(|disc| disc.visible(history))
        else {
            continue;
        };
        let (buy, sell) = dot.side_shares();
        let mut mesh = egui::Mesh::default();
        let mut angle = f64::from(super::PIE_START_ANGLE);
        for (share, color) in [(buy, FLOW_BUY), (sell, FLOW_SELL)] {
            let sweep = share * std::f64::consts::TAU;
            add_perimeter(&mut mesh, disc, angle, sweep, color, true);
            angle += sweep;
        }
        clip.add(egui::Shape::mesh(mesh));
        let clipped = !history.contains_rect(egui::Rect::from_center_size(
            disc.center,
            egui::Vec2::splat(disc.radius * 2.0),
        ));
        let label = clip.layout_no_wrap(
            format!(
                "First {}{}",
                dot.mark.quantity.normalize(),
                if clipped { " (clipped)" } else { "" }
            ),
            egui::FontId::monospace(11.0),
            crate::theme::TEXT_PRIMARY,
        );
        if label.size().x <= history.width() && label.size().y <= history.height() {
            let origin =
                history.shrink2(label.size() * 0.5).clamp(disc.center) - label.size() * 0.5;
            clip.galley_with_override_text_color(
                origin + egui::vec2(1.0, 1.0),
                label.clone(),
                egui::Color32::BLACK,
            );
            clip.galley(origin, label, crate::theme::TEXT_PRIMARY);
        }
    }
}

// Explicit annular triangles keep every vertex within the volume radius;
// a centred egui stroke would inflate the outer edge and tiny marks.
fn add_perimeter(
    mesh: &mut egui::Mesh,
    disc: FlowDisc,
    angle: f64,
    sweep: f64,
    color: egui::Color32,
    dashed: bool,
) {
    for quad in inner_perimeter_quads(disc.radius, angle, sweep, dashed) {
        let base = mesh.vertices.len() as u32;
        for [x, y] in quad {
            mesh.vertices.push(Vertex {
                pos: disc.center + egui::vec2(x, y),
                uv: WHITE_UV,
                color,
            });
        }
        mesh.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silhouettes_never_expand_or_fill_the_earned_disc_even_at_subpixel_radii() {
        for radius in [0.01, 0.2, 1.0, 3.0, 12.0] {
            let disc = FlowDisc {
                center: egui::Pos2::ZERO,
                radius,
            };
            let mut mesh = egui::Mesh::default();
            add_perimeter(
                &mut mesh,
                disc,
                0.0,
                std::f64::consts::TAU,
                egui::Color32::RED,
                false,
            );
            assert!(!mesh.is_empty());
            for vertex in &mesh.vertices {
                let distance = vertex.pos.distance(disc.center);
                assert!(distance <= radius + 0.00001);
                assert!(distance >= radius * 0.75 - 0.00001);
            }
            let mut exception = egui::Mesh::default();
            add_perimeter(
                &mut exception,
                disc,
                0.0,
                std::f64::consts::TAU,
                egui::Color32::RED,
                true,
            );
            assert!(!exception.is_empty());
            assert!(
                exception
                    .vertices
                    .iter()
                    .all(|vertex| vertex.pos.distance(disc.center) >= radius * 0.75 - 0.00001)
            );
            assert_ne!(exception.vertices, mesh.vertices);
        }
    }
}
