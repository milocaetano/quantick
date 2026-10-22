//! The tape lane divider's pointer, and the rail a folded side leaves; the
//! fold itself is [`quantick_orderflow::LaneDividerDrag`]'s.
use eframe::egui;
use quantick_chart_interaction::{pane::Intent, pane_axis::AxisGesture};
use quantick_orderflow::{LaneFold, LiveLaneStyle};

use super::{ChartPane, LANE_HANDLE_HALF_WIDTH_PX};
use crate::canvas_layout as canvas;

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

/// Where the divider takes the pointer; `None` without a divider to move.
pub(super) fn lane_handle(pane: &ChartPane, chart: egui::Rect) -> Option<egui::Rect> {
    pane.frame.lane_divider_x?;
    let hit = lane(pane)?.divider_hit(chart.width(), LANE_HANDLE_HALF_WIDTH_PX)?;
    Some(span(chart, hit))
}

/// Drag the divider to move it or fold a side; click a rail to unfold.
/// Registered after the chart body so it takes the drag that would pan.
pub(super) fn handle_lane_divider(pane: &mut ChartPane, ui: &egui::Ui, chart: egui::Rect) {
    let Some(rect) = lane_handle(pane, chart) else {
        return;
    };
    let fold = lane(pane).map_or(LaneFold::Open, |lane| lane.fold);
    let sense = [egui::Sense::click_and_drag(), egui::Sense::drag()][usize::from(fold.is_open())];
    let mut handle = ui.interact(rect, pane.interaction_id("lane_divider"), sense);
    handle = match fold {
        LaneFold::Open => handle,
        LaneFold::Candles => handle.on_hover_text("click to show the candles again, or drag right"),
        LaneFold::Tape => handle.on_hover_text("click to show the tape again, or drag left"),
    };
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let Some(tape) = pane.orderflow.as_mut() else {
        return;
    };
    if handle.drag_started() || handle.drag_stopped() {
        tape.release_lane_drag();
    }
    if handle.clicked() {
        tape.edit_config(|config| config.live_lane.fold = LaneFold::Open);
    }
    if handle.dragged() {
        // Drag left → a wider tape, at the expense of the candles.
        let (delta, width) = (handle.drag_delta().x, chart.width());
        let resize = Intent::Axis(AxisGesture::ResizeTape { delta, width });
        super::canvas_gestures::navigate(pane, resize);
    }
}

/// A folded side's rail over what the side drew, as the canvas rail looks.
pub(super) fn paint_lane_rail(pane: &ChartPane, painter: &egui::Painter, chart: egui::Rect) {
    let Some(rail) = lane(pane).and_then(|lane| lane.fold_rail(chart.width())) else {
        return;
    };
    let paint = span(chart, (rail.paint_from_px, rail.paint_to_px));
    let hit = span(chart, (rail.hit_from_px, rail.hit_to_px));
    let hover = painter.ctx().pointer_hover_pos();
    let hot = hover.is_some_and(|at| hit.contains(at));
    crate::tab::paint_rail(painter, paint, chart.left() + rail.edge_px(), hot);
}
