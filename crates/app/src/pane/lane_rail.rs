//! The tape lane divider's pointer; what a drag decides is
//! [`quantick_orderflow::LaneDividerDrag`]'s.
use eframe::egui;
use quantick_chart_interaction::{pane::Intent, pane_axis::AxisGesture};
use quantick_orderflow::LiveLaneStyle;

use super::{ChartPane, LANE_HANDLE_HALF_WIDTH_PX};
use crate::canvas_layout as canvas;
use crate::plot_area::split_time_strip;

// One feel for both edges: the lane's threshold is the canvas pane's.
const _: () = assert!(quantick_orderflow::LANE_FOLD_AT_PX == canvas::COLLAPSE_AT_PX);

fn span(chart: egui::Rect, (from, to): (f32, f32)) -> egui::Rect {
    egui::Rect::from_x_y_ranges(chart.left() + from..=chart.left() + to, chart.y_range())
}

fn lane(pane: &ChartPane) -> Option<&LiveLaneStyle> {
    Some(&pane.orderflow.as_ref()?.cached_config().live_lane)
}

/// The jump-to-live chip at the candles' end of the time strip: `None`
/// while following live.
pub(super) fn live_chip(pane: &ChartPane, strip: egui::Rect) -> Option<egui::Rect> {
    let away = !pane.model.viewport.follows_live();
    let (history_strip, _) = split_time_strip(strip, pane.frame.lane_divider_x);
    away.then(|| super::live_chip_rect(history_strip))
}

/// Where the divider takes the pointer; `None` without a divider to move.
pub(super) fn lane_handle(pane: &ChartPane, chart: egui::Rect) -> Option<egui::Rect> {
    pane.frame.lane_divider_x?;
    let hit = lane(pane)?.divider_hit(chart.width(), LANE_HANDLE_HALF_WIDTH_PX)?;
    Some(span(chart, hit))
}

/// Drag the divider to move it; let it go near an edge to leave that side's
/// view. Registered after the chart body so it takes the drag that would pan.
pub(super) fn handle_lane_divider(pane: &mut ChartPane, ui: &egui::Ui, chart: egui::Rect) {
    let Some(rect) = lane_handle(pane, chart) else {
        return;
    };
    let handle = ui.interact(
        rect,
        pane.interaction_id("lane_divider"),
        egui::Sense::drag(),
    );
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let Some(tape) = pane.orderflow.as_mut() else {
        return;
    };
    if handle.drag_started() {
        tape.forget_lane_drag();
    }
    if handle.dragged() {
        // Drag left → a wider tape, at the expense of the candles.
        let (delta, width) = (handle.drag_delta().x, chart.width());
        let resize = Intent::Axis(AxisGesture::ResizeTape { delta, width });
        super::canvas_gestures::navigate(pane, resize);
    }
    if handle.drag_stopped()
        && let Some(tape) = pane.orderflow.as_mut()
    {
        tape.finish_lane_drag();
    }
}
