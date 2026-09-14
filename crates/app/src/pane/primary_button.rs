//! Who has the primary button this frame, decided in the order the frame
//! hands it out: an armed drawing tool first (in `drawing_gestures.rs`), then
//! the paper simulator's lines and aim, then the pointer tool over the
//! drawings and the mirrored marks, and only then the pan.
//!
//! Two arms of [`ChartPane::handle_navigation`], cut out of it so the input
//! frame reads as that order rather than as nine hundred lines. Each takes the
//! pointer the frame already read — one `SharedPointer`, built once — and
//! gives back one bool the frame needs: whether the button is still the
//! chart's. Nothing is cloned and nothing is re-read; the bodies are the ones
//! that ran inline, at the same indentation.

use eframe::egui;

use crate::bands::{self, Bands};
use crate::chart_layers::ChartLayer;
use crate::drawings;
use crate::indicator_render;
use crate::paper_trading::ChartInput;
use crate::plot_area::PlotAreas;
use crate::toolrail::Tool;

use super::axes_and_panes::PANE_DIVIDER_HANDLE_PX;
use super::{
    ChartPane, DRAWING_DRAG_THRESHOLD_PX, DrawingDrag, PaneChrome, SharedDrag, SharedPointer,
    tape_switch_rect,
};

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
                                self.drawing_handle_at(position, band, history_right, total)
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

    /// The pointer tool over this pane's drawings and over the marks another
    /// pane owns: hover cursors, click-select, drag initiation, the handle and
    /// body drags, and the release that commits one undo entry.
    ///
    /// One arm of [`ChartPane::handle_navigation`], called once per frame after
    /// paper has answered. Returns whether a drawing gesture consumed the
    /// primary button this frame.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn handle_pointer_tool(
        &mut self,
        ui: &egui::Ui,
        chrome: &mut PaneChrome<'_>,
        chart: &egui::Response,
        areas: &PlotAreas,
        bands: &Bands,
        pointer: &SharedPointer,
        pointer_delta: egui::Vec2,
        paper_gesture: bool,
    ) -> bool {
        let &SharedPointer {
            position: pointer_position,
            area: drawing_area,
            over_chrome,
            pressed: primary_pressed,
            down: primary_down,
            released: primary_released,
            history_right,
            total,
            magnet,
        } = pointer;
        let mut drawing_drag_consumes_gesture = false;
        if !paper_gesture && chrome.toolrail.tool() == Tool::Pointer {
            // The band under the pointer decides everything below: a price
            // trend line and a CVD trend line can be one pixel apart on
            // screen and mean unrelated things, so no pick ever crosses one.
            // Refusing bands and the panes' own chrome are not canvases.
            let pointer_band = pointer_position
                .filter(|_| !over_chrome)
                .filter(|position| !Self::pane_chrome_hit(areas, *position))
                .and_then(|position| bands::band_at(bands, position))
                .filter(|band| band.drawable());
            // Hover feedback: a resize cursor over a selected anchor, a move
            // cursor over any visible body, and not-allowed over locked
            // geometry (visible objects in the viewport only — bounded work).
            if let Some(band) = pointer_band
                && let Some(position) = pointer_position
            {
                if let Some(selected) = self.drawings.selected()
                    && self
                        .drawing_handle_in(selected, position, band, history_right, total)
                        .is_some()
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[selected].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::ResizeNwSe
                        });
                } else if let Some(hovered) = self.drawing_at(position, band, history_right, total)
                {
                    ui.ctx()
                        .set_cursor_icon(if self.drawings.items()[hovered].locked {
                            egui::CursorIcon::NotAllowed
                        } else {
                            egui::CursorIcon::Move
                        });
                }
            }
            // A click is the release of a press that never travelled, read
            // from the raw pointer rather than from the candles' response.
            // That is what makes a click in an indicator pane select at all:
            // the pane's own pan gesture covers the same pixels and would
            // otherwise be the only widget to hear it. `over_chrome` is
            // honoured at press time, so a press on a panel leaves no pending
            // origin here and no selection can be stolen through one.
            if primary_released
                && self.gestures.drag_pending_from.is_some()
                && let Some(position) = pointer_position
            {
                // Alt+click walks down the z-order through overlapping
                // objects; a plain click selects the topmost hit.
                //
                // A click selects what the *press* grabbed. The release must
                // not re-decide, because opening the panel moves the chart
                // under the pointer (see `drawing_press_pick`) and the object
                // the user pressed on is no longer at that pixel — the
                // release would wipe the selection the press just made, and
                // the panel would flicker open and shut with the mouse
                // standing still.
                //
                // Alt+click keeps re-deciding on purpose: it walks down the
                // z-order from the current selection, so it only ever runs
                // while a selection already exists and the layout is settled.
                let selected = if ui.input(|input| input.modifiers.alt) {
                    pointer_band.and_then(|band| {
                        self.drawing_below_selection(position, band, history_right, total)
                    })
                } else {
                    self.gestures.press_pick.take().unwrap_or_else(|| {
                        // No press was recorded (it landed on chrome, or off
                        // any band): fall back to asking now.
                        pointer_band.and_then(|band| {
                            self.drawing_pick_at(position, band, history_right, total)
                        })
                    })
                };
                self.drawings.select(selected);
                // A note under the pointer takes a double click: its words
                // *are* the object, so pointing at one and double clicking
                // asks to type in it — the same reading as double clicking a
                // curve to open its settings. It is read here rather than in
                // the free-chart branch above because a click on an object
                // starts a translate gesture, which is exactly what clears
                // that branch's `primary_free`. Without this, fixing a typo
                // meant hunting for a field in a panel that placing a note no
                // longer opens.
                if chart.double_clicked()
                    && let Some(index) = selected
                    && self
                        .drawings
                        .items()
                        .get(index)
                        .is_some_and(|drawing| drawing.tool.holds_text() && !drawing.locked)
                {
                    self.gestures.content_editing = Some(index);
                    *chrome.begin_text_edit = true;
                }
            }
            // Drag initiation reads the raw press (an `interact` per object
            // would be unbounded work), so it must honour the chrome gate
            // itself: a press on the inspector never grabs the stroke or the
            // handle underneath — the panel is opaque by contract.
            let mut drawing_drag_started = false;
            if primary_pressed
                && let Some(band) = pointer_band
                && let Some(position) = pointer_position
            {
                // One question, asked once, on the geometry the user was
                // actually looking at when they pressed.
                self.gestures.press_pick =
                    Some(self.drawing_pick_at(position, band, history_right, total));
                self.gestures.drag_pending_from = Some(position);
                if let Some((drawing_index, handle)) =
                    self.drawing_handle_at(position, band, history_right, total)
                {
                    self.drawings.select(Some(drawing_index));
                    self.gestures.drag = if self.drawings.items()[drawing_index].locked {
                        DrawingDrag::Blocked
                    } else {
                        self.drawings.begin_gesture();
                        DrawingDrag::Handle {
                            drawing_index,
                            handle,
                        }
                    };
                } else if let Some(index) = self.drawing_at(position, band, history_right, total) {
                    self.drawings.select(Some(index));
                    self.gestures.drag = if self.drawings.items()[index].locked {
                        DrawingDrag::Blocked
                    } else {
                        self.drawings.begin_gesture();
                        DrawingDrag::Translate
                    };
                }
                // A press that hits no geometry is not ours to interpret: it
                // belongs to whatever egui routed it to (inspector, manager,
                // chart pan). Deselection happens through the egui-routed
                // click above, which already respects floating windows.
                drawing_drag_started = self.gestures.drag.is_active();
            }
            // A held button is not yet a drag. Until the pointer has left the
            // threshold the object does not move at all, so a click stays a
            // click — the alternative is that selecting a channel re-angles
            // it by two pixels of hand tremor, and the trader's level is
            // quietly no longer where they put it.
            //
            // `travel` is measured from the press, not accumulated per frame,
            // so crossing the threshold hands the gesture the *whole* movement
            // and the object does not trail the cursor by 4 px forever.
            let travel = match (self.gestures.drag_pending_from, pointer_position) {
                (Some(origin), Some(position)) => {
                    let travel = position - origin;
                    if travel.length() < DRAWING_DRAG_THRESHOLD_PX {
                        None
                    } else {
                        self.gestures.drag_pending_from = None;
                        Some(travel)
                    }
                }
                // No pending origin: the threshold was already passed earlier
                // in this gesture, so this frame's own delta drives it.
                (None, _) => Some(pointer_delta),
                (Some(_), None) => None,
            };
            if primary_down
                && !drawing_drag_started
                && let Some(travel) = travel
            {
                match self.gestures.drag {
                    DrawingDrag::Handle {
                        drawing_index,
                        handle,
                    } => {
                        // The object's own band, not the one under the
                        // pointer: dragging a CVD anchor up into the candles
                        // stretches it to the top of its pane, and never
                        // writes a price into a CVD anchor.
                        let dragged = self
                            .drawings
                            .items()
                            .get(drawing_index)
                            .and_then(|drawing| bands::band_of(bands, drawing));
                        // Moving a mark keeps it glued to a bar's extreme:
                        // the rule that placed it is the rule that holds it.
                        let handle_snap = self
                            .drawings
                            .items()
                            .get(drawing_index)
                            .map_or(drawings::AnchorSnap::Pointer, |drawing| {
                                drawing.tool.anchor_snap()
                            });
                        if let Some(band) = dragged
                            && let Some(position) = pointer_position
                        {
                            let position = egui::pos2(
                                position.x.clamp(band.rect.left(), history_right),
                                position.y.clamp(band.rect.top(), band.rect.bottom()),
                            );
                            if let Some(point) = self.drawing_point_at(
                                position,
                                history_right,
                                total,
                                magnet,
                                handle_snap,
                                band,
                            ) {
                                self.drag_drawing_handle(
                                    drawing_index,
                                    handle,
                                    point,
                                    band,
                                    history_right,
                                    total,
                                    if ui.input(|input| input.modifiers.shift) {
                                        drawings::Constrain::Level
                                    } else {
                                        drawings::Constrain::Free
                                    },
                                );
                            }
                            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                        }
                    }
                    DrawingDrag::Translate => {
                        let dragged = self
                            .drawings
                            .selected()
                            .and_then(|index| self.drawings.items().get(index))
                            .and_then(|drawing| bands::band_of(bands, drawing));
                        if let Some(band) = dragged
                            && let Some(scale) = band.scale
                        {
                            let (lo, hi) = scale.range();
                            let delta_bar = travel.x / self.viewport.px_per_bar();
                            // Per *band* height: a pane is a fraction of the
                            // chart's, and dividing by the candles' would move
                            // a CVD level by a fraction of the distance the
                            // pointer travelled. The sign follows the band's
                            // orientation — the object tracks the pointer,
                            // not the price axis.
                            let sign = if scale.is_inverted() { 1.0 } else { -1.0 };
                            let delta_value =
                                sign * f64::from(travel.y / band.rect.height()) * (hi - lo);
                            self.drawings.translate_selected(delta_bar, delta_value);
                            // Market time is what every other pane reads the
                            // object through; a move that left it behind
                            // would drag the mark here and leave its shared
                            // twin standing where it used to be.
                            self.retime_selected();
                        }
                    }
                    DrawingDrag::Blocked => {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed);
                    }
                    DrawingDrag::None => {}
                }
            }
            // Marks the *other* pane owns, worked from this one (§D7).
            //
            // A shared object is one object, so the trader may grab it on
            // either chart it appears on — the alternative is a mark that can
            // be seen here and only deleted over there, which is the split
            // getting in the way of the work. This pane's own objects still
            // win the press: the mirror is the second answer, never the first.
            //
            // Everything below is said in market time and price, and the tab
            // hands it to the pane that holds the object. Nothing is written
            // to a copy.
            if !self.gestures.drag.is_active() {
                self.interact_shared(
                    ui,
                    chrome,
                    SharedPointer {
                        position: pointer_position,
                        area: drawing_area,
                        over_chrome,
                        pressed: primary_pressed,
                        down: primary_down,
                        released: primary_released,
                        history_right,
                        total,
                        magnet,
                    },
                );
            }
            drawing_drag_consumes_gesture =
                self.gestures.drag.is_active() || self.gestures.shared_drag.is_active();
            if primary_released {
                // One gesture, one undo entry — recorded only if it moved.
                self.drawings.commit_gesture();
                self.gestures.drag = DrawingDrag::None;
                // A press that ended in a drag rather than a click leaves its
                // answer unconsumed; it must not survive to decide the *next*
                // click, which may be somewhere else entirely. The click path
                // above already ran this frame and took it if it was a click.
                self.gestures.press_pick = None;
                self.gestures.drag_pending_from = None;
            }
        } else {
            self.gestures.drag = DrawingDrag::None;
            self.gestures.press_pick = None;
            self.gestures.drag_pending_from = None;
            self.gestures.shared_drag = SharedDrag::None;
            self.gestures.shared_drag_pending_from = None;
            self.gestures.shared_pointer_mark = None;
        }
        drawing_drag_consumes_gesture
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
}
