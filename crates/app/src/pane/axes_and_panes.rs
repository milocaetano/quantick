//! The gestures on the axes and on the indicator panes, and the free functions they share.
//!
//! One grammar: an axis zooms its axis, a pane's body pans the scale its gutter scales, and the
//! later egui claim wins the overlap, so arms register in this order and after the chart body. Two
//! arms of [`ChartPane::handle_navigation`] plus the `pane_*_gesture` and `axis_zoom_gesture`
//! helpers only they call.

use crate::pane::constants::PANE_DIVIDER_HANDLE_PX;
use eframe::egui;

use crate::indicator_render;
use crate::indicators::{MIN_PANE_HEIGHT_PX, PaneSizing};
use crate::plot_area::{PlotAreas, split_time_strip};
use crate::price_view::PriceView;
use quantick_layers::ChartLayer;

use super::{ChartPane, LANE_HANDLE_HALF_WIDTH_PX, PaneChrome, SCROLL_ZOOM_PX, live_chip_rect};
use quantick_chart_interaction::pane::{Effect, Intent, Model, update};
use quantick_chart_interaction::pane_axis::{
    AxisGesture, IndicatorGesture, ScaleAction, ScaleTarget, Sizing,
};

/// The divider along a pane's top edge, as a resize handle. The band it opens is the pane *below*
/// it: drag up and that pane grows into the chart, drag down and it gives the room back. Double
/// click hands the pane back to the automatic layout, the same escape the price axis and every
/// pane's own scale offer.
///
/// Returns the sizing the pane should now have, or `None` if the divider was untouched this frame.
fn pane_divider_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    slot: &crate::indicators::PaneSlot,
    plot: egui::Rect,
) -> Option<(bool, f32, f32)> {
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
        return Some((true, 0.0, slot.rect.height()));
    }
    if handle.dragged() {
        let delta = handle.drag_delta().y;
        if delta != 0.0 {
            // Dragging the top edge upwards makes the band below it taller. The floor is not
            // applied here: `PaneSizing::desired` owns it, so a drag and the automatic layout
            // cannot disagree about how short a pane may be.
            return Some((false, delta, slot.rect.height()));
        }
    }
    None
}

/// The gesture that pans a pane's own scale: press inside the pane and drag up or down, as a press
/// on the candles drags price. Separate from [`axis_zoom_gesture`] because they are different verbs
/// on the same axis (the gutter *scales* it, the body *moves* it), and a pane whose body did
/// nothing left the axis reachable only from the gutter at the far side of the chart.
///
/// `auto` is the range the last frame fitted; without one there is nothing to take manual control
/// *from*, and only the reset stays available. `primary_free` is false while the primary button
/// belongs to something else (a drawing tool placing an object, or a drawing being dragged); the
/// wheel and the axis still answer, since an armed tool takes the *button*, not the pane (audit
/// S2).
fn pane_pan_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    body: egui::Rect,
    model: &mut Model,
    slot: u64,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
    primary_free: bool,
) -> (PaneGesture, egui::Response) {
    let response = ui.interact(body, id, egui::Sense::click_and_drag());
    if primary_free {
        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
    }
    let mut gesture = PaneGesture::default();
    if primary_free && response.dragged() {
        gesture.pan_x = response.drag_delta().x;
    }
    if response.hovered() {
        gesture.scroll_y = ui.input(|input| input.raw_scroll_delta.y);
    }
    let effects = update(
        model,
        Intent::Axis(AxisGesture::PaneBody {
            slot,
            reset: response.double_clicked(),
            primary_free,
            drag: [
                0.0,
                if response.dragged() {
                    response.drag_delta().y
                } else {
                    0.0
                },
            ],
            height: body.height(),
            auto,
            total: 0,
            scroll: 0.0,
            scroll_gain: SCROLL_ZOOM_PX,
        }),
    );
    for effect in effects {
        if let Effect::Scale { action, .. } = effect {
            scale_action(view, action);
        }
    }
    (gesture, response)
}

/// What a drag or scroll over a pane body owes the *chart*; the pane's own axis has already been
/// moved by the time this is returned. A pane is a band of the same time axis the candles draw, so
/// a sideways drag pans it and scroll zooms it. Collected rather than applied on the spot because
/// the viewport belongs to the pane's owner, and every stacked pane would otherwise apply its own
/// copy of the same drag.
#[derive(Default)]
struct PaneGesture {
    /// Horizontal drag, in pixels.
    pan_x: f32,
    /// Wheel travel while hovering, in egui's scroll units.
    scroll_y: f32,
}

/// The gesture that scales a vertical axis, wherever its numbers live: drag up to compress the
/// span, down to expand, scroll to zoom, double-click to hand the axis back to auto-fit. One
/// implementation for the price gutter and every indicator pane's, so a new band that wants an axis
/// (a volume profile, the tape) registers it rather than copying it and no two axes drift apart in
/// feel.
///
/// `auto` is the range the last frame fitted; `None` means nothing is computed to scale yet, and
/// only the reset stays available.
///
/// `flip_span` is the price gutter's privilege: an expanding drag past that many flip spans turns
/// the chart upside down ([`PriceView::drag_zoom_against`]) and the drag's sense mirrors with it.
/// Indicator gutters pass `None`: a pane's values have no upside down. The wheel never flips.
///
/// Returns the band's response, so the price gutter can hang its context menu off the region the
/// gesture owns.
fn axis_zoom_gesture(
    ui: &egui::Ui,
    id: egui::Id,
    band: egui::Rect,
    model: &mut Model,
    target: ScaleTarget,
    view: &mut PriceView,
    auto: Option<(f64, f64)>,
    flip_span: Option<f64>,
) -> egui::Response {
    let response = ui.interact(band, id, egui::Sense::click_and_drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
    }
    let effects = update(
        model,
        Intent::Axis(AxisGesture::Scale {
            target,
            reset: response.double_clicked(),
            drag: if response.dragged_by(egui::PointerButton::Primary) {
                response.drag_delta().y
            } else {
                0.0
            },
            scroll: if response.hovered() {
                ui.input(|input| input.raw_scroll_delta.y)
            } else {
                0.0
            },
            auto,
            flip_span,
            inverted: view.is_inverted(),
        }),
    );
    for effect in effects {
        if let Effect::Scale { action, .. } = effect {
            scale_action(view, action);
        }
    }
    response
}

fn scale_action(view: &mut PriceView, action: ScaleAction) {
    match action {
        ScaleAction::Reset => view.reset(),
        ScaleAction::Zoom {
            factor,
            auto,
            flip_span,
        } => match flip_span {
            Some(span) => view.drag_zoom_against(factor, auto, span),
            None => view.zoom(factor, auto),
        },
        ScaleAction::Pan {
            delta,
            height,
            auto,
        } => view.pan_pixels(delta, height, auto),
    }
}

pub(super) fn apply_scale(pane: &mut ChartPane, target: ScaleTarget, action: ScaleAction) {
    match target {
        ScaleTarget::Price => scale_action(&mut pane.price_view, action),
        ScaleTarget::Indicator(slot) => {
            if let Some(view) = pane
                .indicators
                .view_mut(crate::indicator_worker::SlotId(slot))
            {
                scale_action(&mut view.scale, action);
            }
        }
    }
}
pub(super) fn pane_sizing(sizing: Sizing) -> PaneSizing {
    match sizing {
        Sizing::Auto => PaneSizing::Auto,
        Sizing::Manual(height) => PaneSizing::Manual(height),
        Sizing::Collapsed => PaneSizing::Collapsed,
    }
}

/// The gestures on the frame around the candles: the lane divider, the time strip and its
/// jump-to-live chip, the lane's own strip, and the price gutter with its menu. One arm of
/// [`ChartPane::handle_navigation`], registered after the chart body so each handle takes the
/// drag that would otherwise pan behind it.
pub(super) fn handle_axis_gestures(
    pane: &mut ChartPane,
    ui: &egui::Ui,
    areas: &PlotAreas,
    chrome: &mut PaneChrome<'_>,
) {
    let auto = pane.frame.auto_range;
    let (tape_only, _) = pane.tape_modes();
    // The lane's divider, as a resize handle. Registered after the chart body so it takes the
    // drag that would otherwise pan the candles behind it; the line stays a hairline and the
    // cursor says it can be moved.
    let divider = pane.frame.lane_divider_x.filter(|_| !tape_only).map(|x| {
        ui.interact(
            egui::Rect::from_min_max(
                egui::pos2(x - LANE_HANDLE_HALF_WIDTH_PX, areas.chart.top()),
                egui::pos2(x + LANE_HANDLE_HALF_WIDTH_PX, areas.chart.bottom()),
            ),
            pane.interaction_id("lane_divider"),
            egui::Sense::drag(),
        )
    });
    if let Some(divider) = &divider {
        if divider.hovered() || divider.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        if divider.dragged() {
            // Drag left → a wider tape, at the expense of the candles.
            super::canvas_gestures::navigate(
                pane,
                Intent::Axis(AxisGesture::ResizeTape {
                    delta: divider.drag_delta().x,
                    width: areas.chart.width(),
                }),
            );
        }
    }

    // Bottom time strip: drag or scroll to zoom. The segment under the
    // lane zooms the lane's window, the rest zooms the candle spacing —
    // each pane's own time axis, under the pane it belongs to.
    let (history_strip, lane_strip) = split_time_strip(areas.time_strip, pane.frame.lane_divider_x);
    let time = ui.interact(
        history_strip,
        pane.interaction_id("time_nav"),
        egui::Sense::click_and_drag(),
    );
    if time.hovered() || time.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    super::canvas_gestures::navigate(
        pane,
        Intent::Axis(AxisGesture::Time {
            drag: if time.dragged() {
                time.drag_delta().x
            } else {
                0.0
            },
            scroll: if time.hovered() {
                ui.input(|i| i.raw_scroll_delta.y)
            } else {
                0.0
            },
            scroll_gain: SCROLL_ZOOM_PX,
            tape: false,
        }),
    );
    // The time axis's own menu, the price gutter's twin: what an axis
    // writes is switched from that axis. This one had no menu at all until
    // the compass gave it something to say.
    time.context_menu(|ui| {
        #[cfg(test)]
        pane.context_menu.layer_menu_rects.clear();
        pane.layer_menu_switch(ui, ChartLayer::PointerTime, chrome);
    });
    // Jump-to-live (audit F6): panned into history, the way back is one click at the axis' live
    // end. Registered after the strip gesture so the click is the chip's, not a zoom-drag's.
    if !pane.model.viewport.follows_live() {
        let chip = ui.interact(
            live_chip_rect(history_strip),
            pane.interaction_id("jump_to_live"),
            egui::Sense::click(),
        );
        if chip.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if chip.clicked() {
            super::canvas_gestures::navigate(
                pane,
                Intent::ReturnToLive {
                    tape: false,
                    overlay: None,
                },
            );
        }
    }
    // A lane strip exists only where a lane does, so this is flow-pane
    // only by construction — the tape is what draws the segment.
    if let Some(lane_strip) = lane_strip
        && pane.orderflow.is_some()
    {
        let lane_time = ui.interact(
            lane_strip,
            egui::Id::new(("lane_time_nav", pane.id)),
            egui::Sense::click_and_drag(),
        );
        super::canvas_gestures::navigate(
            pane,
            Intent::Axis(AxisGesture::Time {
                drag: if lane_time.dragged() {
                    lane_time.drag_delta().x
                } else {
                    0.0
                },
                scroll: if lane_time.hovered() {
                    ui.input(|i| i.raw_scroll_delta.y)
                } else {
                    0.0
                },
                scroll_gain: SCROLL_ZOOM_PX,
                tape: true,
            }),
        );
    }

    // Right price gutter: the candles' own axis gesture. It spans their
    // height only — the bands below belong to the panes.
    let price_gutter = axis_zoom_gesture(
        ui,
        pane.interaction_id("price_nav"),
        areas.price_gutter,
        &mut pane.model,
        ScaleTarget::Price,
        &mut pane.price_view,
        auto,
        auto.map(|(lo, hi)| pane.frame.flip_span.unwrap_or(hi - lo)),
    );
    // The axis's own menu: about the scale and what is written on it, not the canvas (the layer
    // menu stays the canvas's right-click). The compass's price half is offered here because
    // this is where a trader looks for something about *this* axis, and where the mark it
    // switches appears.
    price_gutter.context_menu(|ui| {
        #[cfg(test)]
        pane.context_menu.layer_menu_rects.clear();
        let entry =
            quantick_chart_interaction::pane_menu::inverted_entry(pane.price_view.is_inverted());
        let intents = super::menu_renderer::render(
            ui,
            &mut pane.context_menu,
            &mut pane.model,
            &[entry],
            None,
        );
        pane.apply_menu_intents(intents, chrome);
        ui.separator();
        pane.layer_menu_switch(ui, ChartLayer::PointerPrice, chrome);
    });
}

/// The indicator panes' own handles: the gutter zoom, the body pan, the disclosure, the header,
/// and the dividers between them. One arm of [`ChartPane::handle_navigation`], registered last
/// so the later claim wins every overlap with the pan that covers the band.
pub(super) fn handle_indicator_pane_gestures(
    pane: &mut ChartPane,
    ui: &egui::Ui,
    areas: &PlotAreas,
    chrome: &mut PaneChrome<'_>,
    primary_free: bool,
) {
    let total = pane.slots();
    // The same gesture, once per pane, over the gutter band beside it. Keyed by slot *and* pane
    // id: slots are allocated per pane, so a split's two charts can hold the same slot number
    // and a slot-only id would make one pane's axis answer for the other's.
    let pane_id = pane.id;
    let mut pane_time_gesture = PaneGesture::default();
    for ((view, gutter), body) in pane
        .indicators
        .visible_panes_mut()
        .zip(&areas.pane_gutters)
        .zip(&areas.indicator_panes)
    {
        axis_zoom_gesture(
            ui,
            egui::Id::new(("pane_price_nav", pane_id, view.slot)),
            *gutter,
            &mut pane.model,
            ScaleTarget::Indicator(view.slot.0),
            &mut view.scale,
            view.last_auto,
            None,
        );
        if !body.collapsed {
            // The body moves the scale the gutter scales. Registered after the gutter so the
            // two never fight over a pixel: egui gives an overlap to the later claim.
            let (gesture, response) = pane_pan_gesture(
                ui,
                egui::Id::new(("pane_pan", pane_id, view.slot)),
                body.rect,
                &mut pane.model,
                view.slot.0,
                &mut view.scale,
                view.last_auto,
                primary_free,
            );
            pane_time_gesture.pan_x += gesture.pan_x;
            pane_time_gesture.scroll_y += gesture.scroll_y;
            if let Some(enabled) = crate::indicator_guide::menu(&response, view.mouse_vertical_line)
            {
                let _ = update(
                    &mut pane.model,
                    Intent::Indicator(IndicatorGesture::Guide {
                        slot: view.slot.0,
                        enabled,
                    }),
                );
            }
        }
        // The disclosure, in both directions: a control that only opens is half a control, so
        // the square that brings a pane back also puts it away. Registered last, like the body
        // after the gutter, so this corner beats the pan that covers the band.
        let disclosure = ui.interact(
            indicator_render::pane_disclosure_rect(body.rect, body.collapsed),
            egui::Id::new(("pane_disclosure", pane_id, view.slot)),
            egui::Sense::click(),
        );
        if disclosure.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        // The pane's own handle into its settings when open: the header row. Registered after
        // the pan so it takes the double click the body would spend resetting the scale.
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
                let _ = update(
                    &mut pane.model,
                    Intent::Indicator(IndicatorGesture::Settings(view.slot.0)),
                );
            }
        }
        for effect in update(
            &mut pane.model,
            Intent::Indicator(IndicatorGesture::Disclosure {
                slot: view.slot.0,
                clicked: disclosure.clicked(),
                double_clicked: disclosure.double_clicked(),
                collapsed: body.collapsed,
                minimum_height: MIN_PANE_HEIGHT_PX,
            }),
        ) {
            if let Effect::IndicatorSizing { sizing, .. } = effect {
                view.sizing = pane_sizing(sizing);
            }
        }
    }
    // Time, once, whichever pane the pointer was over: the panes share the candles' x axis, so
    // a sideways drag or scroll there must move the same viewport the candles do.
    super::canvas_gestures::navigate(
        pane,
        Intent::Axis(AxisGesture::PaneBody {
            slot: 0,
            reset: false,
            primary_free: true,
            drag: [pane_time_gesture.pan_x, 0.0],
            height: 0.0,
            auto: None,
            total,
            scroll: if chrome.paper.consumed_scroll() {
                0.0
            } else {
                pane_time_gesture.scroll_y
            },
            scroll_gain: SCROLL_ZOOM_PX,
        }),
    );
    // The dividers last of all: registered after every pane body so the grab band takes the
    // drag that would otherwise pan the pane behind it, as the canvas split's divider is
    // registered after both its panes.
    let plot = areas.chart;
    for (view, slot) in pane
        .indicators
        .visible_panes_mut()
        .zip(&areas.indicator_panes)
    {
        if let Some((reset, delta, height)) = pane_divider_gesture(
            ui,
            egui::Id::new(("pane_divider", pane_id, view.slot)),
            slot,
            plot,
        ) {
            for effect in update(
                &mut pane.model,
                Intent::Indicator(IndicatorGesture::Divider {
                    slot: view.slot.0,
                    reset,
                    delta,
                    height,
                }),
            ) {
                if let Effect::IndicatorSizing { sizing, .. } = effect {
                    view.sizing = pane_sizing(sizing);
                }
            }
        }
    }
}
