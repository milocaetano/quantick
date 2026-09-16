use super::{Contribution, Package};
use crate::{chart::PriceScale, orderflow_view::OrderflowView, viewport::Viewport};
use eframe::egui;
use quantick_orderflow::engine::VisibleOrderflow;
pub(super) const PACKAGE: Package = Package {
    layers: &[
        quantick_layers::ChartLayer::TapeChart,
        quantick_layers::ChartLayer::TapeHeatmap,
        quantick_layers::ChartLayer::TapeBubbles,
        quantick_layers::ChartLayer::Heatmap,
        quantick_layers::ChartLayer::Bubbles,
        quantick_layers::ChartLayer::LiveStrip,
        quantick_layers::ChartLayer::LaneMarks,
        quantick_layers::ChartLayer::FlowLegend,
        quantick_layers::ChartLayer::BookStatus,
        quantick_layers::ChartLayer::DepthGaps,
    ],
    contributions: &[
        Contribution::Heatmap(background),
        Contribution::Aggressions(aggressions),
        Contribution::Legend(legend),
        Contribution::Strip(strip),
        Contribution::Status(status),
    ],
};
pub(in crate::pane) struct FlowPass<'a> {
    pub owner: &'a OrderflowView,
    pub painter: &'a egui::Painter,
    pub rect: egui::Rect,
    pub viewport: &'a Viewport,
    pub total: usize,
    pub projection: &'a VisibleOrderflow,
    pub background: egui::Color32,
    pub lane_width: f32,
    pub inverted: bool,
}
pub(in crate::pane) struct LegendPass<'a> {
    pub owner: &'a OrderflowView,
    pub painter: &'a egui::Painter,
    pub rect: egui::Rect,
    pub viewport: &'a Viewport,
    pub total: usize,
    pub projection: &'a VisibleOrderflow,
    pub background: egui::Color32,
    pub lane_width: f32,
    pub legend_inset: f32,
    pub bounds: &'a mut Option<egui::Rect>,
}
pub(in crate::pane) struct StripPass<'a> {
    pub owner: &'a mut OrderflowView,
    pub painter: &'a egui::Painter,
    pub rect: egui::Rect,
    pub scale: &'a PriceScale,
    pub background: egui::Color32,
    pub partial_time: Option<i64>,
}
pub(in crate::pane) struct StatusPass<'a> {
    pub owner: &'a OrderflowView,
    pub painter: &'a egui::Painter,
    pub rect: egui::Rect,
}
fn status(p: &mut StatusPass<'_>) {
    p.owner.draw_status_badge(
        p.painter,
        p.rect,
        super::super::tape_switch::TAPE_SWITCH_RESERVED_PX,
    );
}
fn background(p: &mut FlowPass<'_>) {
    p.owner.draw_background(
        p.painter,
        p.rect,
        p.viewport,
        p.total,
        p.projection,
        p.background,
        p.lane_width,
        p.inverted,
    );
}
fn aggressions(p: &mut FlowPass<'_>) {
    p.owner.draw_aggressions(
        p.painter,
        p.rect,
        p.viewport,
        p.total,
        p.projection,
        p.background,
        p.lane_width,
        p.inverted,
    );
}
fn legend(p: &mut LegendPass<'_>) {
    *p.bounds = p.owner.draw_legend(
        p.painter,
        p.rect,
        p.viewport,
        p.total,
        p.projection,
        p.background,
        p.lane_width,
        p.legend_inset,
    );
}
fn strip(p: &mut StripPass<'_>) {
    p.owner
        .draw_live_strip(p.painter, p.rect, p.scale, p.background, p.partial_time);
}
