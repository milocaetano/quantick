//! The canvas's own pan, zoom and double click, anywhere on it. Beside the
//! native tape the draggable divider splits them — which is why the wheel
//! over that lane now zooms the lane, where a hairline once could not say
//! which chart a roll meant: over the candles they move the candles, over the
//! tape its window, its time and the shared price axis, never the other side.
//! Tape only is the tape across the whole canvas.
use eframe::egui;
use quantick_orderflow::projection::TapeHorizontalGeometry;
use quantick_orderflow::tape_view::TapeEnd;

use super::{ChartPane, LANE_HANDLE_HALF_WIDTH_PX, SCROLL_ZOOM_PX};
use crate::plot_area;

/// How far a drag travels from its press before it is judged. One that sets
/// off more sideways than up or down pans the tape's time and the price
/// axis; one that sets off up or down pans the axis alone, to its release. A
/// hand dragging the axis drifts a few pixels sideways, and each rightward
/// one took the tape into the past, and the book off it, while leftward ones
/// only clamped back at live: the book blinked with the wobble.
const DRAG_JUDGED_AFTER_PX: f32 = 6.0;

/// Whether the drag pressed at `pressed_at`, `travel` off its press now, may
/// move the tape's time: judged once, and kept under the canvas's `id` until
/// the next press.
fn drag_moves_tape_time(
    ctx: &egui::Context,
    id: egui::Id,
    pressed_at: Option<f64>,
    travel: egui::Vec2,
) -> bool {
    let Some(press) = pressed_at.map(f64::to_bits) else {
        return false;
    };
    let key = id.with("drag-moves-tape-time");
    ctx.data_mut(|data| {
        if let Some((judged, sideways)) = data.get_temp::<(u64, bool)>(key)
            && judged == press
        {
            return sideways;
        }
        if travel.length() < DRAG_JUDGED_AFTER_PX {
            return false;
        }
        let sideways = travel.x.abs() > travel.y.abs();
        data.insert_temp(key, (press, sideways));
        sideways
    })
}

/// What the canvas gestures read from this frame's input pass.
pub(super) struct CanvasInput<'r> {
    pub(super) chart: &'r egui::Response,
    pub(super) area: egui::Rect,
    pub(super) total: usize,
    /// No armed tool, drawing or paper line holds the primary button.
    pub(super) primary_free: bool,
    /// The paper ruler spent this frame's wheel: one roll, one meaning.
    pub(super) scroll_taken: bool,
}

impl ChartPane {
    pub(super) fn handle_canvas_gestures(&mut self, ui: &egui::Ui, input: CanvasInput<'_>) {
        let CanvasInput {
            chart,
            area,
            total,
            primary_free,
            scroll_taken,
        } = input;
        let (tape_only, native_tape) = self.tape_modes();
        // Tape only has no divider to grab: its left edge is the canvas's.
        let divider = self.frame.lane_divider_x.filter(|_| !tape_only);
        let on_divider = |position: egui::Pos2| {
            plot_area::gesture_hits_lane_divider(divider, position.x, LANE_HANDLE_HALF_WIDTH_PX)
        };
        let over_tape = |position: egui::Pos2| {
            native_tape && (tape_only || divider.is_some_and(|x| position.x > x))
        };
        let (press, pressed_at, travel, middle_down, delta, scroll) = ui.input(|i| {
            let p = &i.pointer;
            let travel = p.latest_pos().zip(p.press_origin());
            (
                p.press_origin(),
                p.press_start_time(),
                travel.map_or(egui::Vec2::ZERO, |(now, from)| now - from),
                p.middle_down(),
                p.delta(),
                i.raw_scroll_delta.y,
            )
        });
        // Over the tape the sideways part of a drag is its time, and only a
        // drag that set off sideways may move it.
        let side = |delta: egui::Vec2, tape: bool| {
            if !tape || drag_moves_tape_time(ui.ctx(), chart.id, pressed_at, travel) {
                delta
            } else {
                egui::vec2(0.0, delta.y)
            }
        };
        // Primary only: the secondary drag is the quick range's
        // (`pane/quick_range.rs`), and the middle button pans below.
        if total > 0
            && primary_free
            && chart.dragged_by(egui::PointerButton::Primary)
            && !chart.interact_pointer_pos().is_some_and(on_divider)
        {
            let tape = press.is_some_and(over_tape);
            self.pan_canvas(side(chart.drag_delta(), tape), total, tape);
        }
        // The middle button pans always, mid-placement included: a trader who
        // drops one end of a trend line must be able to go find the other.
        // Like the primary drag, it pans the side it was pressed on.
        let hover = chart
            .hover_pos()
            .filter(|position| area.contains(*position) && !on_divider(*position));
        if total > 0
            && middle_down
            && let Some(position) = hover
        {
            let tape = over_tape(press.unwrap_or(position));
            self.pan_canvas(side(delta, tape), total, tape);
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        // Not with a tool armed: two placement clicks are two anchors. Over
        // the tape a double click returns it to live and to automatic Y; on an
        // overlay's own line it opens that line; elsewhere the candles snap
        // back to the live edge.
        if chart.double_clicked() && primary_free {
            let at = chart.interact_pointer_pos();
            match at.and_then(|position| self.hit_test().overlay_plot_at(position)) {
                _ if at.is_some_and(over_tape) => {
                    if let Some(tape) = self.orderflow.as_mut() {
                        tape.set_tape_end(TapeEnd::Live);
                    }
                    self.price_view.reset();
                }
                Some(slot) => self.pending_settings = Some(slot),
                None => {
                    self.viewport.snap_to_live();
                    self.price_view.reset();
                }
            }
        }
        if chart.hovered() && !scroll_taken && scroll.abs() > 0.0 {
            // Scroll up (positive) zooms in.
            let factor = 2.0_f32.powf(scroll / SCROLL_ZOOM_PX);
            let tape = tape_only || chart.hover_pos().is_some_and(over_tape);
            match self.orderflow.as_mut().filter(|_| tape) {
                Some(orderflow) => orderflow.zoom_live_lane(factor),
                None => self.viewport.zoom(factor),
            }
        }
    }

    /// Up and down pans the shared price axis wherever the drag is; sideways
    /// it pans the candles, or over the tape the tape's own time: rightward
    /// reveals older prints.
    fn pan_canvas(&mut self, delta: egui::Vec2, total: usize, tape: bool) {
        let height = self.frame.chart_height;
        if !tape {
            self.viewport.pan_pixels(delta.x, total);
        } else if let (Some(chart), Some(divider), Some(orderflow)) = (
            self.frame.chart_area,
            self.frame.lane_divider_x,
            self.orderflow.as_mut(),
        ) {
            let bubbles = &orderflow.cached_config().bubbles;
            let span = TapeHorizontalGeometry::resolve(chart.right() - divider, height, bubbles);
            orderflow.pan_tape(delta.x, span.span_px);
        }
        if let Some(auto) = self.frame.auto_range
            && delta.y != 0.0
            && height > 1.0
        {
            let (lo, hi) = self.price_view.resolve(auto);
            let price_per_px = (hi - lo) / f64::from(height);
            self.price_view
                .pan_screen(f64::from(delta.y), price_per_px, auto);
        }
    }
}
