//! The tape lane divider's pointer, and the rail a closed side leaves; the
//! decisions are [`quantick_orderflow::LaneDividerDrag`]'s.
use eframe::egui;
use quantick_chart_interaction::{pane::Intent, pane_axis::AxisGesture};
use quantick_orderflow::{FoldRail, LaneFold, LaneSide, LiveLaneStyle};

use super::{ChartPane, LANE_HANDLE_HALF_WIDTH_PX};
use crate::canvas_layout as canvas;
use crate::plot_area::split_time_strip;

// One feel for both folds: the lane's numbers are the canvas pane's.
const _: () = assert!(quantick_orderflow::LANE_FOLD_RAIL_PX == canvas::COLLAPSED_PANE_WIDTH_PX);
const _: () = assert!(quantick_orderflow::LANE_FOLD_AT_PX == canvas::COLLAPSE_AT_PX);
const _: () = assert!(quantick_orderflow::LANE_FOLD_HIT_PX == canvas::COLLAPSED_HIT_PX);

fn span(chart: egui::Rect, (from, to): (f32, f32)) -> egui::Rect {
    egui::Rect::from_x_y_ranges(chart.left() + from..=chart.left() + to, chart.y_range())
}

fn lane(pane: &ChartPane) -> Option<&LiveLaneStyle> {
    Some(&pane.orderflow.as_ref()?.cached_config().live_lane)
}

fn rail(pane: &ChartPane, chart: egui::Rect) -> Option<FoldRail> {
    lane(pane)?.fold_rail(chart.width())
}

/// The rail this pane painted, for the scene: the side it closes and its rectangle.
pub(crate) fn painted_rail(pane: &ChartPane) -> Option<(LaneSide, egui::Rect)> {
    let chart = pane.frame.chart_rect?;
    let rail = rail(pane, chart)?;
    Some((
        rail.side,
        span(chart, (rail.paint_from_px, rail.paint_to_px)),
    ))
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
    pane.frame.candles_right?;
    let hit = lane(pane)?.divider_hit(chart.width(), LANE_HANDLE_HALF_WIDTH_PX)?;
    Some(span(chart, hit))
}

/// Drag the divider to move it or close a side; click a rail to reopen at the
/// default width, drag it to reopen under the hand. Registered after the chart
/// body so it takes the drag that would pan.
pub(super) fn handle_lane_divider(pane: &mut ChartPane, ui: &egui::Ui, chart: egui::Rect) {
    let Some(rect) = lane_handle(pane, chart) else {
        return;
    };
    let closed = rail(pane, chart).map(|rail| rail.side);
    let sense = closed.map_or(egui::Sense::drag(), |_| egui::Sense::click_and_drag());
    let mut handle = ui.interact(rect, pane.interaction_id("lane_divider"), sense);
    if let Some(closed) = closed {
        handle = handle.on_hover_text(quantick_chart::pane_header::rail_hint(closed));
    }
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let Some(tape) = pane.orderflow.as_mut() else {
        return;
    };
    if handle.drag_started() || !handle.dragged() {
        tape.release_lane_drag();
    }
    if handle.clicked() {
        tape.set_lane_fold(LaneFold::Open);
    }
    if handle.dragged() {
        // Drag left → a wider tape, at the expense of the candles.
        let (delta, width) = (handle.drag_delta().x, chart.width());
        let resize = Intent::Axis(AxisGesture::ResizeTape { delta, width });
        super::canvas_gestures::navigate(pane, resize);
    }
}

/// A closed side's rail over what the side drew, as the canvas rail looks.
pub(super) fn paint_lane_rail(pane: &ChartPane, painter: &egui::Painter, chart: egui::Rect) {
    let Some(rail) = rail(pane, chart) else {
        return;
    };
    let paint = span(chart, (rail.paint_from_px, rail.paint_to_px));
    let hit = span(chart, (rail.hit_from_px, rail.hit_to_px));
    let hover = painter.ctx().pointer_hover_pos();
    let hot = hover.is_some_and(|at| hit.contains(at));
    crate::tab::paint_rail(painter, paint, chart.left() + rail.edge_px(), hot);
}
