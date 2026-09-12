//! The gestures on the axes and on the indicator panes, and the free
//! functions they share.
//!
//! Every gesture here follows one grammar: an axis zooms its axis, a pane's
//! body pans the scale its gutter scales, and the later egui claim wins the
//! overlap — which is why the arms are registered in the order they are and
//! after the chart body. Two arms of [`ChartPane::handle_navigation`] plus the
//! `pane_*_gesture` and `axis_zoom_gesture` helpers only they call, cut out of
//! `pane.rs` as a pure move at the same indentation.

use eframe::egui;

use crate::chart_layers::ChartLayer;
use crate::indicator_render;
use crate::indicator_worker::SlotId;
use crate::indicators::{MIN_PANE_HEIGHT_PX, PaneSizing};
use crate::plot_area::{PlotAreas, split_time_strip};
use crate::price_view::PriceView;

use super::{
    ChartPane, LANE_HANDLE_HALF_WIDTH_PX, LANE_ZOOM_DRAG_PX, PaneChrome, SCROLL_ZOOM_PX,
    live_chip_rect,
};

/// Half-height of the grab band over a pane's top edge, in pixels.
///
/// The rule itself stays a hairline — a thick bar between a chart and its
/// indicator reads as a wall in the data. The handle around it is what makes
/// it catchable, and the resize cursor is the only thing that announces it:
/// the same bargain the live lane's divider and the canvas split already
/// strike.
pub(super) const PANE_DIVIDER_HANDLE_PX: f32 = 4.0;

/// Pixels of drag on a vertical axis that change its span by a factor of `e`.
///
/// One number for the price gutter and for every indicator pane's gutter: the
/// axes stretch at the same rate, so the gesture feels the same wherever the
/// numbers being dragged happen to live.
const AXIS_ZOOM_DRAG_PX: f32 = 150.0;
/// The same, for a scroll over an axis rather than a drag. One wheel notch
/// reports far more units than a pointer travels in a frame, so each unit has
/// to count for less — a larger divisor, not a smaller one.
const AXIS_ZOOM_SCROLL_PX: f32 = 200.0;

/// The divider along a pane's top edge, as a resize handle.
///
/// The band it opens is the pane *below* it, which is what a top edge means:
/// drag it up and that pane grows into the chart, drag it down and it gives
/// the room back. Double click hands the pane to the automatic layout again —
/// the same escape the price axis and every pane's own scale offer.
///
/// Returns the sizing the pane should now have, or `None` when the divider was
/// not touched this frame.
fn pane_divider_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    slot: &crate::indicators::PaneSlot,
    plot: egui::Rect,
) -> Option<PaneSizing> {
    let edge = slot.rect.top();
    let handle = ui.interact(
        egui::Rect::from_min_max(
            egui::pos2(plot.left(), edge - PANE_DIVIDER_HANDLE_PX),
            egui::pos2(plot.right(), edge + PANE_DIVIDER_HANDLE_PX),
        ),
        id,
        egui::Sense::click_and_drag(),
    );
    if handle.hovered() || handle.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if handle.double_clicked() {
        return Some(PaneSizing::Auto);
    }
    if handle.dragged() {
        let delta = handle.drag_delta().y;
        if delta != 0.0 {
            // Dragging the top edge upwards makes the band below it taller.
            // The floor is not applied here: `PaneSizing::desired` owns it, so
            // a drag and the automatic layout cannot disagree about how short
            // a pane may be.
            return Some(PaneSizing::Manual(slot.rect.height() - delta));
        }
    }
    None
}

/// The gesture that pans a pane's own scale: press inside the pane and drag it
/// up or down, exactly as a press on the candles drags price.
///
/// Separate from [`axis_zoom_gesture`] because they are different verbs on the
/// same axis — the gutter *scales* it, the body *moves* it — and the candles
/// already split them that way. A pane whose body did nothing left the axis
/// reachable only from the gutter at the far side of the chart, which is a
/// long way to travel to move a curve out of its own way.
///
/// `auto` is the range the last frame fitted; without one there is nothing to
/// take manual control *from*, and only the reset stays available.
/// `primary_free` is false while the primary button belongs to something else
/// — a drawing tool placing an object, or a drawing being dragged. The wheel
/// and the axis still answer then: an armed tool takes the *button*, not the
/// pane (audit S2).
fn pane_pan_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    body: egui::Rect,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
    primary_free: bool,
) -> PaneGesture {
    let response = ui.interact(body, id, egui::Sense::click_and_drag());
    if response.double_clicked() && primary_free {
        view.reset();
    }
    if !primary_free {
        let mut gesture = PaneGesture::default();
        if response.hovered() {
            gesture.scroll_y = ui.input(|input| input.raw_scroll_delta.y);
        }
        return gesture;
    }
    if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }

    // The pane's own axis moves here; time is the candles' to move, and the
    // caller applies it once for however many panes are stacked — panning
    // three panes' worth of x would move the chart three times per drag.
    let mut gesture = PaneGesture::default();
    if response.dragged() {
        gesture.pan_x = response.drag_delta().x;
        if let Some(auto) = auto {
            let height = body.height();
            let delta = response.drag_delta().y;
            if delta != 0.0 && height > 1.0 {
                let (lo, hi) = view.resolve(auto);
                let per_px = (hi - lo) / f64::from(height);
                view.pan_screen(f64::from(delta), per_px, auto);
            }
        }
    }
    if response.hovered() {
        gesture.scroll_y = ui.input(|input| input.raw_scroll_delta.y);
    }
    gesture
}

/// What a drag or scroll over a pane body owes the *chart* — the pane's own
/// axis has already been moved by the time this is returned.
///
/// A pane is a band of the same time axis the candles draw, so the gestures
/// that mean "time" there mean time here too: drag sideways to pan it, scroll
/// to zoom it. Collected rather than applied on the spot because the viewport
/// belongs to the pane's owner, and because every stacked pane would otherwise
/// apply its own copy of the same drag.
#[derive(Default)]
struct PaneGesture {
    /// Horizontal drag, in pixels.
    pan_x: f32,
    /// Wheel travel while hovering, in egui's scroll units.
    scroll_y: f32,
}

/// The gesture that scales a vertical axis, wherever its numbers live: drag up
/// to compress the span, down to expand, scroll to zoom, double-click to hand
/// the axis back to auto-fit.
///
/// One implementation for the price gutter and for every indicator pane's, so
/// a third band that wants an axis — a volume profile, the tape — registers it
/// rather than copying it, and no two axes can drift apart in feel. Lives here
/// with the panes that use it: every axis in the window belongs to one.
///
/// `auto` is the range the last frame fitted; `None` means nothing has been
/// computed to scale yet, and only the reset stays available.
///
/// `flips` is the price gutter's privilege: an expanding drag past the flip
/// threshold turns the chart upside down ([`PriceView::drag_zoom`]), and the
/// drag's sense mirrors with it — the same downward drag that flattened the
/// chart grows it again past the flip. Indicator gutters pass `false`: a
/// pane's values have no upside down. The wheel never flips on either.
///
/// Returns the band's response, so the price gutter can hang its context menu
/// off the very region the gesture owns.
fn axis_zoom_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    band: egui::Rect,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
    flips: bool,
) -> egui::Response {
    let response = ui.interact(band, id, egui::Sense::click_and_drag());
    // The cursor is the affordance (audit F5): nothing else on the band says
    // it scales, mirroring the lane divider's own rule that the pointer's
    // shape is what announces a gesture.
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    if response.double_clicked() {
        view.reset();
    }
    let Some(auto) = auto else {
        return response;
    };
    // Primary only: the band now also answers a right-click with its context
    // menu, and egui's `dragged()` counts any button — an unfiltered check
    // would let a slipped right-press zoom (or, on the price gutter, flip)
    // the chart while swallowing the menu it was aiming for.
    if response.dragged_by(egui::PointerButton::Primary) {
        // Drag up → compress the span (a taller trace); down → expand it —
        // mirrored once the chart is upside down.
        let sense = if flips && view.is_inverted() {
            -1.0
        } else {
            1.0
        };
        let factor = f64::from(sense * response.drag_delta().y / AXIS_ZOOM_DRAG_PX).exp();
        if flips {
            view.drag_zoom(factor, auto);
        } else {
            view.zoom(factor, auto);
        }
    }
    if response.hovered() {
        let scroll = ui.input(|input| input.raw_scroll_delta.y);
        if scroll.abs() > 0.0 {
            view.zoom(f64::from(-scroll / AXIS_ZOOM_SCROLL_PX).exp(), auto);
        }
    }
    response
}

impl ChartPane {
    /// The gestures on the frame around the candles: the lane divider, the
    /// time strip and its jump-to-live chip, the lane's own strip, and the
    /// price gutter with its menu.
    ///
    /// One arm of [`ChartPane::handle_navigation`], registered after the chart
    /// body so each handle takes the drag that would otherwise pan behind it.
    pub(super) fn handle_axis_gestures(
        &mut self,
        ui: &egui::Ui,
        areas: &PlotAreas,
        chrome: &mut PaneChrome<'_>,
        auto: Option<(f64, f64)>,
    ) {
        // The lane's divider, as a resize handle. Registered after the chart
        // body so it takes the drag that would otherwise pan the candles
        // behind it, and it is the only place the pointer changes shape: the
        // line stays a hairline, the cursor is what says it can be moved.
        let divider = self.frame.lane_divider_x.map(|x| {
            ui.interact(
                egui::Rect::from_min_max(
                    egui::pos2(x - LANE_HANDLE_HALF_WIDTH_PX, areas.chart.top()),
                    egui::pos2(x + LANE_HANDLE_HALF_WIDTH_PX, areas.chart.bottom()),
                ),
                self.interaction_id("lane_divider"),
                egui::Sense::drag(),
            )
        });
        if let Some(divider) = &divider {
            if divider.hovered() || divider.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if divider.dragged()
                && let Some(orderflow) = self.orderflow.as_mut()
            {
                // Drag left → a wider tape, at the expense of the candles.
                orderflow.resize_live_lane(divider.drag_delta().x, areas.chart.width());
            }
        }

        // Bottom time strip: drag or scroll to zoom. The segment under the
        // lane zooms the lane's window, the rest zooms the candle spacing —
        // each pane's own time axis, under the pane it belongs to.
        let (history_strip, lane_strip) =
            split_time_strip(areas.time_strip, self.frame.lane_divider_x);
        let time = ui.interact(
            history_strip,
            self.interaction_id("time_nav"),
            egui::Sense::click_and_drag(),
        );
        if time.dragged() {
            // Drag right → wider candles (zoom in); left → narrower (zoom out).
            self.viewport
                .zoom((time.drag_delta().x / LANE_ZOOM_DRAG_PX).exp());
        }
        if time.hovered() || time.dragged() {
            // Same rule as the vertical axes (audit F5): the cursor is what
            // announces the zoom gesture.
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if time.hovered() {
            let scroll = ui.input(|i| i.raw_scroll_delta.y);
            if scroll.abs() > 0.0 {
                self.viewport.zoom(2.0_f32.powf(scroll / SCROLL_ZOOM_PX));
            }
        }
        // The time axis's own menu, the price gutter's twin: what an axis
        // writes is switched from that axis. This one had no menu at all until
        // the compass gave it something to say.
        time.context_menu(|ui| {
            #[cfg(test)]
            self.layer_menu_rects.clear();
            let _ = self.layer_checkbox(ui, ChartLayer::PointerTime, chrome);
        });
        // Jump-to-live (audit F6): panned into history, the way back is one
        // click at the axis' live end. Registered after the strip gesture so
        // the click is the chip's, not a zoom-drag's — the lane divider's
        // own registration rule.
        if !self.viewport.follows_live() {
            let chip = ui.interact(
                live_chip_rect(history_strip),
                self.interaction_id("jump_to_live"),
                egui::Sense::click(),
            );
            if chip.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if chip.clicked() {
                self.viewport.snap_to_live();
            }
        }
        // A lane strip exists only where a lane does, so this is flow-pane
        // only by construction — the tape is what draws the segment.
        if let Some(lane_strip) = lane_strip
            && let Some(orderflow) = self.orderflow.as_mut()
        {
            let lane_time = ui.interact(
                lane_strip,
                egui::Id::new(("lane_time_nav", self.id)),
                egui::Sense::click_and_drag(),
            );
            if lane_time.dragged() {
                // Drag right → less market time in the band (zoom in), so
                // prints run across it faster and further apart.
                orderflow.zoom_live_lane((lane_time.drag_delta().x / LANE_ZOOM_DRAG_PX).exp());
            }
            if lane_time.hovered() {
                let scroll = ui.input(|i| i.raw_scroll_delta.y);
                if scroll.abs() > 0.0 {
                    orderflow.zoom_live_lane(2.0_f32.powf(scroll / SCROLL_ZOOM_PX));
                }
            }
        }

        // Right price gutter: the candles' own axis gesture. It spans their
        // height only — the bands below belong to the panes.
        let price_gutter = axis_zoom_gesture(
            ui,
            self.interaction_id("price_nav"),
            areas.price_gutter,
            &mut self.price_view,
            auto,
            true,
        );
        // The axis's own menu: about the scale and about what is written on
        // it, not about the canvas — the layer menu stays the canvas's
        // right-click. The compass's price half is offered here rather than
        // only in the layer menu because this is where a trader looks for
        // something about *this* axis, and where the mark it switches
        // actually appears.
        price_gutter.context_menu(|ui| {
            #[cfg(test)]
            self.layer_menu_rects.clear();
            let mut inverted = self.price_view.is_inverted();
            if ui
                .checkbox(&mut inverted, "Inverted chart")
                .on_hover_text(
                    "flip the chart upside down — low prices at the top. \
                     Also reached by dragging the axis down until the bars \
                     flatten and turn over",
                )
                .clicked()
            {
                self.price_view.set_inverted(inverted);
                ui.close_menu();
            }
            ui.separator();
            let _ = self.layer_checkbox(ui, ChartLayer::PointerPrice, chrome);
        });
    }

    /// The indicator panes' own handles: the gutter zoom, the body pan, the
    /// disclosure, the header, and the dividers between them.
    ///
    /// One arm of [`ChartPane::handle_navigation`], registered last so the
    /// later claim wins every overlap with the pan that covers the band.
    pub(super) fn handle_indicator_pane_gestures(
        &mut self,
        ui: &egui::Ui,
        areas: &PlotAreas,
        chrome: &mut PaneChrome<'_>,
        primary_free: bool,
        total: usize,
    ) {
        // The same gesture, once per pane, over the gutter band beside it.
        // Keyed by slot *and* pane id: slots are allocated per pane, so a
        // split's two charts can hold the same slot number and a slot-only id
        // would make one pane's axis answer for the other's.
        let pane_id = self.id;
        let mut pane_time_gesture = PaneGesture::default();
        // Collected here and parked on the pane below: the loop holds a mutable
        // borrow of `self.indicators`, and the dialog belongs to the app.
        let mut settings_request: Option<SlotId> = None;
        // Which pane, if any, was opened by a click on its own collapsed strip
        // on the last frame that had one — and what this frame decides to hand
        // to the next. See the disclosure block below.
        let strip_expanded = self.strip_expanded;
        let mut opened_from_strip = strip_expanded;
        for ((view, gutter), body) in self
            .indicators
            .visible_panes_mut()
            .zip(&areas.pane_gutters)
            .zip(&areas.indicator_panes)
        {
            axis_zoom_gesture(
                ui,
                egui::Id::new(("pane_price_nav", pane_id, view.slot)),
                *gutter,
                &mut view.scale,
                view.last_auto,
                false,
            );
            if !body.collapsed {
                // The body moves the scale the gutter scales. Registered after
                // the gutter so the two never fight over the same pixel: the
                // gutter is a band beside the pane, and egui gives an overlap
                // to the later claim.
                let gesture = pane_pan_gesture(
                    ui,
                    egui::Id::new(("pane_pan", pane_id, view.slot)),
                    body.rect,
                    &mut view.scale,
                    view.last_auto,
                    primary_free,
                );
                pane_time_gesture.pan_x += gesture.pan_x;
                pane_time_gesture.scroll_y += gesture.scroll_y;
            }
            // The disclosure, in both directions: a control that only opens is
            // half a control, so the square that brings a pane back is what
            // puts it away. Registered *last* for the same reason the body is
            // registered after the gutter — the later claim wins the overlap,
            // and this corner has to beat the pan that covers the whole band.
            let disclosure = ui.interact(
                indicator_render::pane_disclosure_rect(body.rect, body.collapsed),
                egui::Id::new(("pane_disclosure", pane_id, view.slot)),
                egui::Sense::click(),
            );
            if disclosure.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            // The pane's own handle into its settings when it is open: the
            // header row. Registered after the pan so it takes the double click
            // the body would otherwise spend resetting the scale; the pan keeps
            // every other pixel of the band.
            if !body.collapsed {
                let header = ui.interact(
                    indicator_render::pane_header_rect(body.rect, body.collapsed),
                    egui::Id::new(("pane_header", pane_id, view.slot)),
                    egui::Sense::click(),
                );
                if header.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if header.double_clicked() {
                    settings_request = Some(view.slot);
                }
            }
            // A collapsed strip is the fourth handle, and it needs the two
            // clicks of a double click read *across the frames between them*:
            // the first one expands the pane, so by the time the second arrives
            // the strip is gone, the pointer sits somewhere in the body of a
            // pane that is now a hundred pixels tall, and `body.collapsed` is
            // already false.
            //
            // Before this, that second click simply collapsed the pane again —
            // so double-clicking a collapsed strip expanded it and put it back,
            // and did nothing at all. It opens the settings instead, which is
            // the only reading that leaves the gesture worth making. What
            // carries it across the frames is `strip_expanded`: the slot whose
            // strip opened this pane, set by the click that opened it and spent
            // by the one that follows.
            if disclosure.double_clicked() && strip_expanded == Some(view.slot) {
                settings_request = Some(view.slot);
                opened_from_strip = None;
            } else if disclosure.clicked() {
                // A plain click, which egui reports only when it is *not* half
                // of a double one — so reaching here always means the previous
                // gesture is over and any flag it left is spent.
                opened_from_strip = None;
                if body.collapsed {
                    // Manual, not Auto: the automatic rule is what collapsed
                    // it, so handing it back would undo the click on the very
                    // next frame. An explicit height is served before the
                    // automatic ones and therefore always fits.
                    view.sizing = PaneSizing::Manual(MIN_PANE_HEIGHT_PX);
                    opened_from_strip = Some(view.slot);
                } else {
                    view.sizing = PaneSizing::Collapsed;
                }
            }
        }
        self.strip_expanded = opened_from_strip;
        if let Some(slot) = settings_request {
            self.pending_settings = Some(slot);
        }
        // Time, once, whichever pane the pointer was over: the panes share the
        // candles' x axis, so a sideways drag or a scroll there has to move the
        // same viewport the candles do — otherwise the same gesture would mean
        // one thing over the bars and nothing one pane below them.
        if total > 0 && pane_time_gesture.pan_x != 0.0 {
            self.viewport.pan_pixels(pane_time_gesture.pan_x, total);
        }
        // One wheel, one meaning at a time: while the ruler is walking a
        // bracket out from an aim, the same travel must not also zoom.
        if pane_time_gesture.scroll_y.abs() > 0.0 && !chrome.paper.consumed_scroll() {
            self.viewport
                .zoom(2.0_f32.powf(pane_time_gesture.scroll_y / SCROLL_ZOOM_PX));
        }
        // The dividers last of all: registered after every pane body so the
        // grab band takes the drag that would otherwise pan the pane behind
        // it, exactly as the canvas split's divider is registered after both
        // its panes and the lane's after the candles.
        let plot = areas.chart;
        for (view, slot) in self
            .indicators
            .visible_panes_mut()
            .zip(&areas.indicator_panes)
        {
            if let Some(sizing) = pane_divider_gesture(
                ui,
                egui::Id::new(("pane_divider", pane_id, view.slot)),
                slot,
                plot,
            ) {
                view.sizing = sizing;
            }
        }
    }
}
