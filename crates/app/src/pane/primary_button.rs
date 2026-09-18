//! Who has the primary button this frame, decided in the order the frame
//! hands it out: an armed drawing tool first (in `drawing_gestures.rs`), then
//! the paper simulator's lines and aim, then the pointer tool over the
//! drawings and the mirrored marks, and only then the pan.
//!
//! Paper arbitration takes the pointer the frame already read and answers
//! whether paper consumed it. `PaneGestures` then updates local and mirrored
//! drawings in `pointer_gestures`; navigation receives both answers before pan.
//! The pane-chrome classifier is shared with placement and the gesture owner.

use eframe::egui;

use crate::bands::{self, Bands};
use crate::indicator_render;
use crate::paper_trading::ChartInput;
use crate::plot_area::PlotAreas;
use crate::toolrail::Tool;
use quantick_layers::ChartLayer;

use super::axes_and_panes::PANE_DIVIDER_HANDLE_PX;
use super::{ChartPane, PaneChrome, SharedPointer, tape_switch_rect};

impl ChartPane {
    /// Who owns the primary button before the drawings are asked: the paper
    /// simulator's grabbed lines, its cancel targets and the cmd-trading aim.
    ///
    /// One arm of [`ChartPane::handle_navigation`], called once per frame with
    /// the pointer the frame already read. Returns whether paper took the
    /// gesture, which is what keeps the chart from panning under a held line.
    pub(super) fn handle_paper_input(
        &self,
        ui: &egui::Ui,
        chrome: &mut PaneChrome<'_>,
        areas: &PlotAreas,
        bands: &Bands,
        pointer: &SharedPointer,
        tool_armed: bool,
    ) -> bool {
        // Orders are placed at a price, so the scale is the candles' own.
        let drawing_scale = bands[0].scale;
        let &SharedPointer {
            position: pointer_position,
            area: drawing_area,
            over_chrome,
            pressed: primary_pressed,
            down: primary_down,
            released: primary_released,
            history_right,
            total,
            ..
        } = pointer;
        // Simulated order lines take the pointer before the drawings: they
        // sit higher in the draw stack, and a grabbed stop is operational,
        // not annotational. The flag mirrors the drawings' gesture
        // consumption so the chart never pans under a held line; the chrome
        // gate applies at press time only, like everywhere else. Escape is
        // deliberately absent here — cancels live in the app's single escape
        // stack (`handle_drawing_keys`). Only the focused pane offers the
        // gesture, because the whole tab shares one simulator.
        //
        // Not while a drawing tool is armed: the button is the tool's, and it
        // can only be handed out once. Without this gate a click meant to
        // drop an anchor would *also* reach an order's ✕ and cancel it —
        // the early return this replaced used to hide that.
        //
        // The cmd-trading aim is the one paper gesture that claims the
        // *whole* plot rather than a line or a ✕, so it alone yields to an
        // annotation already under the pointer. The pane answers that
        // question here — paper never reads the drawings — for the same
        // reason it answers "a tool is armed": the button can be handed
        // out once, and the default buy modifier is Shift, the very key
        // that levels a channel corner.
        //
        // **Handles only, never a body.** A handle is a 12 px target where
        // the two gestures genuinely collide: Shift on a corner levels the
        // object, and there is no other way to ask for that. A body is a
        // region, and some bodies are enormous — a fixed-range profile's
        // hit test claims its whole histogram strip on purpose
        // (`fixed_range_profile.rs`), so yielding bodies meant a chart with
        // a profile on it had a hole where the aim simply did not appear.
        // Moving a body needs no modifier at all, and a body drag reads
        // Shift every frame, so pressing first and then holding it still
        // constrains the move.
        //
        // The canvas's own chrome counts too, and it does *not* live in a
        // floating layer, so `over_chrome` never sees it: the tape chip
        // and an indicator pane's header or divider are pixels a press
        // already means something on, and a modifier resting under the
        // hand must not turn "put the tape back" into "rest an order".
        //
        // Per-frame path, so it costs nothing on a frame with no modifier
        // down: the aim cannot exist without one, and only then is the
        // pick worth running — the same bounded, visible-objects-only
        // handle pick the drag initiation in `handle_pointer_tool` performs, so an
        // *unselected* object's handle keeps its pixel too.
        let modifiers = ui.input(|input| input.modifiers);
        let modifier_down = modifiers.shift || modifiers.command || modifiers.alt;
        let canvas_claimed = pointer_position
            .filter(|_| modifier_down)
            .is_some_and(|position| {
                Self::pane_chrome_hit(areas, position)
                    || tape_switch_rect(areas.chart).contains(position)
                    || (chrome.toolrail.tool() == Tool::Pointer
                        && !over_chrome
                        && bands::band_at(bands, position)
                            .filter(|band| band.drawable())
                            .is_some_and(|band| {
                                self.drawing_projection()
                                    .drawing_handle_at(
                                        &self.drawings,
                                        position,
                                        band,
                                        history_right,
                                        total,
                                    )
                                    .is_some()
                            }))
            });
        let paper_layer_visible = self.layer_visible(ChartLayer::PaperTrading, chrome.style);
        // The wheel over the plot, offered to the paper layer first: with an
        // aim up it belongs to the ruler, and the chart's zoom is told in
        // `handle_navigation` to leave that frame's travel alone.
        let paper_scroll = pointer_position
            .filter(|position| drawing_area.contains(*position))
            .map_or(0.0, |_| {
                ui.input(|input| {
                    let delta = input.raw_scroll_delta;
                    // Windows turns a vertical wheel into *horizontal* scroll
                    // while a modifier is held, so the value the ruler needs
                    // arrives on `x` for exactly the gesture the ruler is
                    // made of. Reading only `y` meant the ruler saw nothing
                    // whenever the trader was actually holding the key.
                    if delta.y.abs() > f32::EPSILON {
                        delta.y
                    } else {
                        delta.x
                    }
                })
            });
        let paper_gesture = if chrome.paper_takes_input && !tool_armed {
            chrome.paper.handle_chart_input(&ChartInput {
                chart: drawing_area,
                scale: drawing_scale.as_ref(),
                pointer: pointer_position,
                primary_pressed: primary_pressed && !over_chrome,
                primary_down,
                primary_released,
                modifiers,
                canvas_claimed,
                scroll_y: paper_scroll,
                middle_pressed: ui
                    .input(|input| input.pointer.button_pressed(egui::PointerButton::Middle)),
                layer_visible: paper_layer_visible,
            })
        } else {
            if chrome.paper_takes_input {
                // A drawing tool owns the hand this frame; a stale cmd
                // preview must not keep painting under it.
                chrome.paper.clear_cmd_preview();
            }
            false
        };
        // The paper lines announce their grabbability (audit paper M3/M4):
        // drawings get hover cursors in `handle_pointer_tool`, and a draggable stop must not
        // feel deader than an annotation — nor may the entry line's blocked
        // band refuse a pan with no explanation at all.
        // The layer gate lives inside `hover_cursor` itself, next to the
        // frame's other decisions, so it cannot be forgotten by a caller.
        if chrome.paper_takes_input
            && !over_chrome
            && !tool_armed
            && let Some(position) =
                pointer_position.filter(|position| drawing_area.contains(*position))
            && let Some(scale) = drawing_scale.as_ref()
            && let Some(cursor) = chrome.paper.hover_cursor(position, drawing_area, scale)
        {
            ui.ctx().set_cursor_icon(cursor);
        }
        paper_gesture
    }

    /// Pixels inside a band that belong to the pane's own chrome rather than
    /// to its canvas: the collapse chevron and the divider grab band.
    ///
    /// A drawing gesture never takes them. egui hands an overlapping rect to
    /// whoever registers last, and both of those register after the canvas —
    /// but the drawing path reads the raw pointer rather than a response, so
    /// it has to honour that order itself instead of inheriting it. Without
    /// this, arming a tool silently kills the chevron and the pane resize.
    pub(super) fn pane_chrome_hit(areas: &PlotAreas, pos: egui::Pos2) -> bool {
        pane_chrome_hit(areas, pos)
    }
}

/// Pixels inside a band that belong to the pane's own chrome rather than
/// to its canvas: the collapse chevron and the divider grab band.
///
/// A drawing gesture never takes them. egui hands an overlapping rect to
/// whoever registers last, and both of those register after the canvas —
/// but the drawing path reads the raw pointer rather than a response, so
/// it has to honour that order itself instead of inheriting it. Without
/// this, arming a tool silently kills the chevron and the pane resize.
pub(super) fn pane_chrome_hit(areas: &PlotAreas, pos: egui::Pos2) -> bool {
    areas.indicator_panes.iter().any(|slot| {
        indicator_render::pane_disclosure_rect(slot.rect, slot.collapsed).contains(pos)
            // The header opens the pane's settings; like the chevron and
            // the divider, arming a drawing tool must not silently kill it.
            || indicator_render::pane_header_rect(slot.rect, slot.collapsed).contains(pos)
            || (pos.y - slot.rect.top()).abs() <= PANE_DIVIDER_HANDLE_PX
    })
}
