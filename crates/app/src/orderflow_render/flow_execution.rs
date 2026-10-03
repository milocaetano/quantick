//! Compact history execution groups on the FLOW candle coordinate system.
use super::bubbles::{PIE_START_ANGLE, SphereShading, add_sector};
use eframe::egui;
#[cfg(test)]
pub(super) use quantick_chart::flow_execution::FlowDisc;
use quantick_chart::flow_execution::{FlowPresentation, FlowRegionRole};
#[cfg(test)]
use quantick_orderflow::HeatmapConfig;
#[cfg(test)]
use quantick_orderflow::projection::flow_tape::FlowTapeDot;
use quantick_orderflow::projection::flow_tape::{FlowProgress, FlowTapeFrame};
#[cfg(test)]
use rust_decimal::Decimal;

// Translucent regional area remains distinct while candle contours stay in front.
#[cfg(test)]
const FLOW_FILL_OPACITY: f32 = quantick_orderflow::config::dressing::flow::PEAK_OPACITY;
// FLOW volume uses its own palette, distinct from candle direction and native Tape.
pub(super) const FLOW_BUY: egui::Color32 = egui::Color32::from_rgb(112, 185, 244);
pub(super) const FLOW_SELL: egui::Color32 = egui::Color32::from_rgb(232, 175, 99);
#[cfg(test)]
const FLOW_OFFSET: egui::Vec2 = egui::vec2(
    quantick_chart::flow_execution::FLOW_EXECUTION_OFFSET[0],
    quantick_chart::flow_execution::FLOW_EXECUTION_OFFSET[1],
);

/// One factual gross-area disc shared by paint and passive inspection.
#[cfg(test)]
pub(super) fn flow_disc(dot: &FlowTapeDot, center: egui::Pos2) -> Option<FlowDisc> {
    FlowDisc::new(dot, center.into())
}

#[cfg(test)]
fn draw_flow_executions(
    painter: &egui::Painter,
    rect: egui::Rect,
    lane_width: f32,
    frame: &FlowTapeFrame,
    _config: &HeatmapConfig,
    backing: Option<egui::Color32>,
    center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) {
    let history = rect.with_max_x(rect.right() - lane_width);
    painter
        .with_clip_rect(history)
        .add(egui::Shape::mesh(flow_mesh(
            history, frame, backing, center,
        )));
}

#[cfg(test)]
pub(super) fn flow_mesh(
    history: egui::Rect,
    frame: &FlowTapeFrame,
    backing: Option<egui::Color32>,
    mut center: impl FnMut(&FlowTapeDot) -> Option<egui::Pos2>,
) -> egui::Mesh {
    let plan = FlowPresentation::new(frame, [history.min.into(), history.max.into()], |dot| {
        center(dot).map(Into::into)
    });
    let drawing = FlowDrawing::new(
        frame,
        plan,
        backing.unwrap_or(crate::theme::CANVAS),
        backing,
    );
    let mut mesh = drawing.meshes[0].clone();
    mesh.append(drawing.meshes[1].clone());
    mesh
}

pub(crate) struct FlowDrawing {
    pub plan: FlowPresentation,
    pub(super) meshes: [egui::Mesh; 2],
}
impl FlowDrawing {
    pub fn new(
        frame: &FlowTapeFrame,
        plan: FlowPresentation,
        background: egui::Color32,
        backing: Option<egui::Color32>,
    ) -> Self {
        use quantick_orderflow::config::dressing::flow::colors;
        let mut meshes = [egui::Mesh::default(), egui::Mesh::default()];
        let colors = [
            colors(None, true, false),
            colors(Some(background.to_array()), false, true),
            colors(backing.map(|color| color.to_array()), false, false),
        ]
        .map(|palette| palette.map(super::premultiplied));
        for region in &plan.regions {
            let dot = &frame.dots[region.dot_index];
            let disc = region.disc;
            let mesh = &mut meshes[usize::from(region.role == FlowRegionRole::Peak)];
            let [buy_color, sell_color] = colors[region.role as usize];
            let (buy, sell) = dot.side_shares();
            let mut angle = f64::from(PIE_START_ANGLE);
            for (share, color) in [(buy, buy_color), (sell, sell_color)] {
                if share <= 0.0 {
                    continue;
                }
                // Exact sides reach geometry directly; no cached share or readability cutoff.
                let sweep = share * std::f64::consts::TAU;
                add_sector(
                    mesh,
                    disc.center.into(),
                    disc.radius,
                    angle as f32,
                    sweep as f32,
                    SphereShading::flat(color),
                    0.0,
                );
                angle += sweep;
            }
        }
        Self { plan, meshes }
    }

    pub fn paint(&self, painter: &egui::Painter, history: egui::Rect, foreground: bool) {
        let mesh = &self.meshes[usize::from(foreground)];
        if !mesh.is_empty() {
            painter
                .with_clip_rect(history)
                .add(egui::Shape::mesh(mesh.clone()));
        }
    }
}

/// Compose neutral isolation into the earned sectors, without extra circle geometry.
/// Glow blends premultiplied sRGBA in gamma space; linear Rgba would change the tone.
#[cfg(test)]
pub(super) fn flow_colors(backing: Option<egui::Color32>, oversized: bool) -> [egui::Color32; 2] {
    quantick_orderflow::config::dressing::flow::colors(
        backing.map(|color| color.to_array()),
        oversized,
        false,
    )
    .map(|[r, g, b, a]| egui::Color32::from_rgba_premultiplied(r, g, b, a))
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
    let detail =
        quantick_orderflow::projection::flow_tape::caption_text(frame, progress, legend_visible);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = (history.width() - 16.0).max(1.0);
    let mut append = |text: &str, color| {
        job.append(
            text,
            0.0,
            egui::TextFormat::simple(egui::FontId::proportional(10.0), color),
        )
    };
    append("FLOW", crate::theme::TEXT_MUTED);
    if legend_visible {
        append(" \u{00b7} ", crate::theme::TEXT_MUTED);
        append("Buy", FLOW_BUY);
        append(" / ", crate::theme::TEXT_MUTED);
        append("Sell", FLOW_SELL);
    }
    append(" \u{00b7} offset \u{2196}", crate::theme::TEXT_MUTED);
    if let Some(detail) = detail {
        job.append(
            &format!(" | {detail}"),
            0.0,
            egui::TextFormat::simple(egui::FontId::proportional(10.0), crate::theme::TEXT_MUTED),
        );
    }
    let galley = painter.layout_job(job);
    let rect = egui::Rect::from_min_size(egui::pos2(history.left() + 8.0, top), galley.size());
    painter
        .with_clip_rect(history)
        .galley(rect.min, galley, crate::theme::TEXT_MUTED);
    Some(rect)
}
#[cfg(test)]
#[path = "tests/flow_execution.rs"]
mod tests;
