//! Modular `egui` renderer for the Bookmap-style order-flow projection.
//!
//! This module deliberately owns only pixels. Book synchronization, grouping,
//! aggression clustering and the conservative association between trades and
//! book reductions live in the pure `orderflow` modules. Keeping that boundary
//! lets themes and visual effects evolve without changing market-data facts.

use eframe::egui;
use eframe::egui::epaint::{Vertex, WHITE_UV};
#[cfg(test)]
use quantick_orderbook::BookSide;
use quantick_orderflow::HeatmapTheme;

mod bubbles;
mod constants;
mod flow_cache;
mod flow_execution;
mod flow_inspection;
mod flow_perimeter;
pub(crate) use flow_cache::cached_flow;
#[cfg(test)]
pub(crate) use flow_cache::draw_cached_flow;
pub(crate) use flow_execution::{FlowDrawing, flow_caption};
pub(crate) use flow_inspection::{FlowInspection, draw_flow_inspection};
pub(crate) use flow_perimeter::draw_flow_perimeters;
mod heatmap;
mod layout;
mod legend;
mod preview;
mod tape_path;
mod tape_rebuild;

pub(crate) use bubbles::draw_aggression_bubbles;
pub(crate) use bubbles::{SphereShading, add_shaded_sector};
pub(crate) use constants::PIE_START_ANGLE;
pub(crate) use heatmap::{draw_heatmap_background, draw_liquidity_events, draw_live_lane_marks};
pub(crate) use layout::{ProjectedLayout, RenderContext, lane_divider_x};
pub(crate) use legend::draw_compact_legend;
pub(crate) use preview::draw_preview;
pub(crate) use tape_path::draw_past_tape_edge;
pub(crate) use tape_rebuild::PaneTapeRebuilds;

pub(crate) use quantick_orderflow::config::theme::{
    LEGEND_HEADER_CLEARANCE_PX, OrderflowRenderStyle, ThemeBubbleRgb, theme_bubble_rgb,
};
use quantick_orderflow::config::theme::{finite_unit, mix_rgb, resting_rgb, thermal_rgb};
type Palette = quantick_orderflow::config::theme::Palette<egui::Color32>;

fn premultiplied([r, g, b, a]: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_premultiplied(r, g, b, a)
}

fn palette_for_theme(theme: HeatmapTheme) -> Palette {
    Palette::for_theme(theme, premultiplied)
}

fn add_gradient_rect(
    mesh: &mut egui::Mesh,
    rect: egui::Rect,
    left: egui::Color32,
    right: egui::Color32,
) {
    if !rect.is_positive() {
        return;
    }
    let base = mesh.vertices.len() as u32;
    mesh.vertices.extend_from_slice(&[
        Vertex {
            pos: rect.left_top(),
            uv: WHITE_UV,
            color: left,
        },
        Vertex {
            pos: rect.right_top(),
            uv: WHITE_UV,
            color: right,
        },
        Vertex {
            pos: rect.right_bottom(),
            uv: WHITE_UV,
            color: right,
        },
        Vertex {
            pos: rect.left_bottom(),
            uv: WHITE_UV,
            color: left,
        },
    ]);
    mesh.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

#[allow(clippy::too_many_arguments)]
fn draw_dashed_vertical(
    painter: &egui::Painter,
    x: f32,
    rect: egui::Rect,
    dash: f32,
    gap: f32,
    color: egui::Color32,
    width: f32,
) {
    let dash = dash.max(0.5);
    let gap = gap.max(0.0);
    let mut y = rect.top();
    while y < rect.bottom() {
        painter.line_segment(
            [
                egui::pos2(x, y),
                egui::pos2(x, (y + dash).min(rect.bottom())),
            ],
            egui::Stroke::new(width.max(0.5), color),
        );
        y += dash + gap;
    }
}

fn rgba(rgb: [u8; 3], alpha: f32) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        rgb[0],
        rgb[1],
        rgb[2],
        (finite_unit(alpha) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests;
