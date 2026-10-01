//! Compact history execution groups on the FLOW candle coordinate system.
use super::bubbles::{BubbleColors, PIE_START_ANGLE, SphereShading, add_sector};
use eframe::egui;
use quantick_orderflow::{
    HeatmapConfig,
    projection::flow_tape::{FlowProgress, FlowTapeDot, FlowTapeFrame},
};
use rust_decimal::Decimal;

// Translucent regional area remains distinct while candle contours stay in front.
const FLOW_FILL_OPACITY: f32 = 0.5;

#[derive(Clone, Copy)]
pub(super) struct FlowDisc {
    pub center: egui::Pos2,
    pub radius: f32,
}
impl FlowDisc {
    pub fn visible(self, history: egui::Rect) -> bool {
        history.distance_sq_to_pos(self.center) < self.radius.powi(2)
    }
    pub fn hit_distance(self, history: egui::Rect, pointer: egui::Pos2) -> Option<f32> {
        let distance = self.center.distance_sq(pointer);
        (history.contains(pointer)
            && self.visible(history)
            && distance <= self.radius.max(6.0).powi(2))
        .then_some(distance)
    }
}

/// One factual gross-area disc shared by paint and passive inspection.
pub(super) fn flow_disc(dot: &FlowTapeDot, center: egui::Pos2) -> Option<FlowDisc> {
    (center.is_finite()
        && dot.radius.is_finite()
        && dot.radius > 0.0
        && dot.mark.quantity > Decimal::ZERO)
        .then_some(FlowDisc {
            center,
            radius: dot.radius,
        })
}

pub(crate) fn draw_flow_executions(
    painter: &egui::Painter,
    rect: egui::Rect,
    lane_width: f32,
    frame: &FlowTapeFrame,
    config: &HeatmapConfig,
    backing: Option<egui::Color32>,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) {
    let history = rect.with_max_x(rect.right() - lane_width);
    let clip = painter.with_clip_rect(history);
    let palette = super::palette_for_theme(config.theme);
    let colors = BubbleColors::resolve(&palette, &config.bubbles);
    for dot in &frame.dots {
        let Some(disc) = center(dot)
            .and_then(|at| flow_disc(dot, at))
            .filter(|disc| disc.visible(history))
        else {
            continue;
        };
        // A visible footprint must not change sector colours. The opaque neutral
        // backing occupies exactly the earned disc, with no rim or minimum floor.
        if let Some(color) = backing {
            clip.circle_filled(disc.center, disc.radius, color);
        }
        let (buy, sell) = dot.side_shares();
        let mut mesh = egui::Mesh::default();
        let mut angle = f64::from(PIE_START_ANGLE);
        for (share, color) in [(buy, colors.buy), (sell, colors.sell)] {
            if share <= 0.0 {
                continue;
            }
            // Exact sides reach geometry directly; no cached share or readability cutoff.
            let sweep = share * std::f64::consts::TAU;
            add_sector(
                &mut mesh,
                disc.center,
                disc.radius,
                angle as f32,
                sweep as f32,
                SphereShading::flat(color.gamma_multiply(FLOW_FILL_OPACITY)),
                0.0,
            );
            angle += sweep;
        }
        clip.add(egui::Shape::mesh(mesh));
    }
}

/// Regional chrome is docked after the native legend, including incomplete first frames.
pub(crate) fn flow_caption(
    painter: &egui::Painter,
    history: egui::Rect,
    top: f32,
    frame: Option<&FlowTapeFrame>,
    progress: FlowProgress,
    legend_visible: bool,
) -> Option<egui::Rect> {
    let text =
        quantick_orderflow::projection::flow_tape::caption_text(frame, progress, legend_visible)?;
    let galley = painter.layout(
        text,
        egui::FontId::proportional(10.0),
        crate::theme::TEXT_MUTED,
        (history.width() - 16.0).max(1.0),
    );
    let rect = egui::Rect::from_min_size(egui::pos2(history.left() + 8.0, top), galley.size());
    painter
        .with_clip_rect(history)
        .galley(rect.min, galley, crate::theme::TEXT_MUTED);
    Some(rect)
}
#[cfg(test)]
#[path = "tests/flow_execution.rs"]
mod tests;
