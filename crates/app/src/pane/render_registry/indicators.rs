use super::super::draw_frame::DrawFrame;
use super::{Contribution, Package};
use crate::{
    chart,
    indicator_render::{self, PlotX},
    indicators::{IndicatorView, IndicatorViews},
};
use eframe::egui;
pub(super) const PACKAGE: Package = Package {
    layers: &[],
    contributions: &[
        Contribution::Overlay(overlays),
        Contribution::IndicatorPane(pane),
    ],
};
pub(in crate::pane) struct OverlayPass<'a, 'b> {
    pub frame: &'a DrawFrame<'b>,
    pub painter: &'a egui::Painter,
    pub plot_x: &'a PlotX<'b>,
    pub indicators: &'a IndicatorViews,
}
pub(in crate::pane) struct IndicatorPanePass<'a, 'b> {
    pub painter: &'a egui::Painter,
    pub frame: &'a indicator_render::PaneFrame<'b>,
    pub view: &'a mut IndicatorView,
    pub plot_x: &'a PlotX<'b>,
    pub start: usize,
    pub end: usize,
    pub partial_slot: Option<usize>,
}
fn overlays(pass: &mut OverlayPass<'_, '_>) {
    let &DrawFrame {
        scale,
        start,
        end,
        closed_total,
        prefix,
        closed,
        partial,
        partial_visible,
        ..
    } = pass.frame;
    // Slot -> (high_y, low_y) in pixels, for above/below-bar markers.
    let bar_extents = |slot: usize| -> Option<(f32, f32)> {
        let bar = if slot < prefix.len() {
            prefix.get(slot)
        } else if slot < closed_total {
            closed.get(slot - prefix.len())
        } else if slot == closed_total {
            partial
        } else {
            None
        }?;
        Some((
            scale.y(chart::to_f64(bar.high)),
            scale.y(chart::to_f64(bar.low)),
        ))
    };
    indicator_render::draw_overlays(
        pass.painter,
        pass.indicators.visible_overlays(),
        pass.plot_x,
        &scale,
        start,
        end,
        partial_visible.map(|_| closed_total),
        &bar_extents,
    );
    // Draw objects (lines/boxes/labels) share the overlays' paint slot:
    // after candles, before aggression bubbles.
    for view in pass.indicators.visible_overlays() {
        indicator_render::draw_objects(
            pass.painter,
            view.render_objects(),
            pass.plot_x,
            |v| scale.y(v),
            start,
            end,
        );
    }
}
fn pane(pass: &mut IndicatorPanePass<'_, '_>) {
    let auto = indicator_render::pane_auto_range(pass.view, pass.start, pass.end);
    pass.view.last_auto = auto;
    indicator_render::draw_pane(
        pass.painter,
        pass.frame,
        pass.view,
        pass.plot_x,
        auto.map(|auto| pass.view.scale.resolve(auto)),
        pass.start,
        pass.end,
        pass.partial_slot,
    );
}
