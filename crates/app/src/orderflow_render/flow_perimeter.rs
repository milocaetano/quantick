//! FLOW silhouettes inside the earned radius, above candle bodies.
use super::{
    bubbles::sphere_segments,
    flow_execution::{FLOW_BUY, FLOW_SELL, FlowDisc, flow_disc},
};
use eframe::egui::{
    self,
    epaint::{Vertex, WHITE_UV},
};
use quantick_orderflow::{
    HeatmapConfig,
    projection::flow_tape::{FlowTapeDot, FlowTapeFrame},
};

// The circumference is opaque for a stable silhouette, but occupies at most
// one quarter of a tiny radius and never changes the outer volume boundary.
const PERIMETER_WIDTH_PX: f32 = 0.75;
const PERIMETER_RADIUS_FRACTION: f32 = 0.25;
// Twelve visible dashes alternate with twelve gaps for capped exceptions.
const DASH_PHASES: usize = 24;

pub(crate) fn draw_flow_perimeters(
    painter: &egui::Painter,
    history: egui::Rect,
    frame: &FlowTapeFrame,
    _config: &HeatmapConfig,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) {
    let clip = painter.with_clip_rect(history);
    for dot in frame.dots.iter().filter(|dot| dot.opening_capped) {
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
            add_perimeter(&mut mesh, disc, angle, sweep, color, dot.opening_capped);
            angle += sweep;
        }
        clip.add(egui::Shape::mesh(mesh));
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
    if sweep <= 0.0 {
        return;
    }
    let full = if dashed {
        sphere_segments(disc.radius).next_multiple_of(DASH_PHASES)
    } else {
        sphere_segments(disc.radius)
    };
    let steps = (sweep / std::f64::consts::TAU * full as f64).ceil() as usize;
    let inner = disc.radius - PERIMETER_WIDTH_PX.min(disc.radius * PERIMETER_RADIUS_FRACTION);
    for step in 0..steps {
        let start = angle + sweep * step as f64 / steps as f64;
        let end = angle + sweep * (step + 1) as f64 / steps as f64;
        // Twelve evenly spaced gaps identify a capped exception, even for one side.
        let phase =
            ((start + end) * 0.5 - f64::from(super::PIE_START_ANGLE)) / std::f64::consts::TAU;
        if dashed && (phase * DASH_PHASES as f64).floor() as usize % 2 == 1 {
            continue;
        }
        let base = mesh.vertices.len() as u32;
        for (angle, radius) in [
            (start, inner),
            (start, disc.radius),
            (end, disc.radius),
            (end, inner),
        ] {
            mesh.vertices.push(Vertex {
                pos: disc.center + egui::vec2(angle.cos() as f32, angle.sin() as f32) * radius,
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
