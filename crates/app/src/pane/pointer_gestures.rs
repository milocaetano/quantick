//! Pointer-tool and mirrored-mark updates belong to their retained gesture.
//! Inputs borrow coordinates and the independent store; effects return to chrome.
use super::drawing_projection::DrawingProjection;
use super::primary_button::pane_chrome_hit;
use super::{
    DRAWING_DRAG_THRESHOLD_PX, DrawingDrag, PaneGestures, SharedDrag, SharedEdit,
    SharedInteraction, SharedPick, SharedPointer,
};
use crate::bands::{self, Bands};
use crate::drawings;
use crate::plot_area::PlotAreas;
use crate::toolrail::Tool;
use eframe::egui;

pub(super) struct PointerFrame<'a> {
    pub(super) ui: &'a egui::Ui,
    pub(super) chart: &'a egui::Response,
    pub(super) areas: &'a PlotAreas,
    pub(super) bands: &'a Bands,
    // Shared marks use the prior paint's carve, not the new local input carve.
    pub(super) cached_bands: &'a Bands,
    pub(super) pointer: &'a SharedPointer,
    pub(super) pointer_delta: egui::Vec2,
    pub(super) paper_gesture: bool,
    pub(super) tool: Tool,
    pub(super) shared_pick: Option<SharedPick>,
    pub(super) shared: SharedInteraction,
}

#[derive(Default)]
pub(super) struct PointerOutcome {
    pub(super) consumed: bool,
    pub(super) cursor: Option<egui::CursorIcon>,
    pub(super) begin_text_edit: bool,
    pub(super) shared: SharedInteraction,
}
impl PointerOutcome {
    fn set_cursor(&mut self, cursor: egui::CursorIcon) {
        self.cursor = Some(cursor);
    }
}

impl PaneGestures {
    pub(super) fn handle_pointer_tool(
        &mut self,
        drawings: &mut drawings::Drawings,
        projection: &DrawingProjection<'_>,
        frame: PointerFrame<'_>,
    ) -> PointerOutcome {
        let PointerFrame {
            ui,
            chart,
            areas,
            bands,
            cached_bands,
            pointer,
            pointer_delta,
            paper_gesture,
            tool,
            shared_pick,
            shared,
        } = frame;
        let mut outcome = PointerOutcome {
            shared,
            ..Default::default()
        };

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
        if !paper_gesture && tool == Tool::Pointer {
            // The band under the pointer decides everything below: a price
            // trend line and a CVD trend line can be one pixel apart on
            // screen and mean unrelated things, so no pick ever crosses one.
            // Refusing bands and the panes' own chrome are not canvases.
            let pointer_band = pointer_position
                .filter(|_| !over_chrome)
                .filter(|position| !pane_chrome_hit(areas, *position))
                .and_then(|position| bands::band_at(bands, position))
                .filter(|band| band.drawable());
            // Hover feedback: a resize cursor over a selected anchor, a move
            // cursor over any visible body, and not-allowed over locked
            // geometry (visible objects in the viewport only — bounded work).
            if let Some(band) = pointer_band
                && let Some(position) = pointer_position
            {
                if let Some(selected) = drawings.selected()
                    && projection
                        .drawing_handle_in(drawings, selected, position, band, history_right, total)
                        .is_some()
                {
                    outcome.set_cursor(if drawings.items()[selected].locked {
                        egui::CursorIcon::NotAllowed
                    } else {
                        egui::CursorIcon::ResizeNwSe
                    });
                } else if let Some(hovered) =
                    projection.drawing_at(drawings, position, band, history_right, total)
                {
                    outcome.set_cursor(if drawings.items()[hovered].locked {
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
                && self.drag_pending_from.is_some()
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
                        projection.drawing_below_selection(
                            drawings,
                            position,
                            band,
                            history_right,
                            total,
                        )
                    })
                } else {
                    self.press_pick.take().unwrap_or_else(|| {
                        // No press was recorded (it landed on chrome, or off
                        // any band): fall back to asking now.
                        pointer_band.and_then(|band| {
                            projection.drawing_pick_at(
                                drawings,
                                position,
                                band,
                                history_right,
                                total,
                            )
                        })
                    })
                };
                drawings.select(selected);
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
                    && drawings
                        .items()
                        .get(index)
                        .is_some_and(|drawing| drawing.tool.holds_text() && !drawing.locked)
                {
                    self.content_editing = Some(index);
                    outcome.begin_text_edit = true;
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
                self.press_pick = Some(projection.drawing_pick_at(
                    drawings,
                    position,
                    band,
                    history_right,
                    total,
                ));
                self.drag_pending_from = Some(position);
                if let Some((drawing_index, handle)) =
                    projection.drawing_handle_at(drawings, position, band, history_right, total)
                {
                    drawings.select(Some(drawing_index));
                    self.drag = if drawings.items()[drawing_index].locked {
                        DrawingDrag::Blocked
                    } else {
                        drawings.begin_gesture();
                        DrawingDrag::Handle {
                            drawing_index,
                            handle,
                        }
                    };
                } else if let Some(index) =
                    projection.drawing_at(drawings, position, band, history_right, total)
                {
                    drawings.select(Some(index));
                    self.drag = if drawings.items()[index].locked {
                        DrawingDrag::Blocked
                    } else {
                        drawings.begin_gesture();
                        DrawingDrag::Translate
                    };
                }
                // A press that hits no geometry is not ours to interpret: it
                // belongs to whatever egui routed it to (inspector, manager,
                // chart pan). Deselection happens through the egui-routed
                // click above, which already respects floating windows.
                drawing_drag_started = self.drag.is_active();
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
            let travel = match (self.drag_pending_from, pointer_position) {
                (Some(origin), Some(position)) => {
                    let travel = position - origin;
                    if travel.length() < DRAWING_DRAG_THRESHOLD_PX {
                        None
                    } else {
                        self.drag_pending_from = None;
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
                match self.drag {
                    DrawingDrag::Handle {
                        drawing_index,
                        handle,
                    } => {
                        // The object's own band, not the one under the
                        // pointer: dragging a CVD anchor up into the candles
                        // stretches it to the top of its pane, and never
                        // writes a price into a CVD anchor.
                        let dragged = drawings
                            .items()
                            .get(drawing_index)
                            .and_then(|drawing| bands::band_of(bands, drawing));
                        // Moving a mark keeps it glued to a bar's extreme:
                        // the rule that placed it is the rule that holds it.
                        let handle_snap = drawings
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
                            if let Some(point) = projection.drawing_point_at(
                                position,
                                history_right,
                                total,
                                magnet,
                                handle_snap,
                                band,
                            ) {
                                projection.drag_drawing_handle(
                                    drawings,
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
                            outcome.set_cursor(egui::CursorIcon::ResizeNwSe);
                        }
                    }
                    DrawingDrag::Translate => {
                        let dragged = drawings
                            .selected()
                            .and_then(|index| drawings.items().get(index))
                            .and_then(|drawing| bands::band_of(bands, drawing));
                        if let Some(band) = dragged
                            && let Some(scale) = band.scale
                        {
                            let (lo, hi) = scale.range();
                            let delta_bar = travel.x / projection.viewport.px_per_bar();
                            // Per *band* height: a pane is a fraction of the
                            // chart's, and dividing by the candles' would move
                            // a CVD level by a fraction of the distance the
                            // pointer travelled. The sign follows the band's
                            // orientation — the object tracks the pointer,
                            // not the price axis.
                            let sign = if scale.is_inverted() { 1.0 } else { -1.0 };
                            let delta_value =
                                sign * f64::from(travel.y / band.rect.height()) * (hi - lo);
                            drawings.translate_selected(delta_bar, delta_value);
                            // Market time is what every other pane reads the
                            // object through; a move that left it behind
                            // would drag the mark here and leave its shared
                            // twin standing where it used to be.
                            projection.retime_selected(drawings);
                        }
                    }
                    DrawingDrag::Blocked => {
                        outcome.set_cursor(egui::CursorIcon::NotAllowed);
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
            if !self.drag.is_active() {
                self.interact_shared(
                    projection,
                    cached_bands,
                    shared_pick,
                    &mut outcome,
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
            drawing_drag_consumes_gesture = self.drag.is_active() || self.shared_drag.is_active();
            if primary_released {
                // One gesture, one undo entry — recorded only if it moved.
                drawings.commit_gesture();
                self.drag = DrawingDrag::None;
                // A press that ended in a drag rather than a click leaves its
                // answer unconsumed; it must not survive to decide the *next*
                // click, which may be somewhere else entirely. The click path
                // above already ran this frame and took it if it was a click.
                self.press_pick = None;
                self.drag_pending_from = None;
            }
        } else {
            self.drag = DrawingDrag::None;
            self.press_pick = None;
            self.drag_pending_from = None;
            self.shared_drag = SharedDrag::None;
            self.shared_drag_pending_from = None;
            self.shared_pointer_mark = None;
        }
        outcome.consumed = drawing_drag_consumes_gesture;
        outcome
    }

    fn interact_shared(
        &mut self,
        projection: &DrawingProjection<'_>,
        cached_bands: &Bands,
        shared_pick: Option<SharedPick>,
        outcome: &mut PointerOutcome,
        pointer: SharedPointer,
    ) {
        // The band under the pointer, on this pane's own last carve: a shared
        // mark is grabbed where it is painted, and where it is painted is the
        // band whose axis its value belongs to. Reading the candles' scale for
        // a CVD mark would send a price back to the pane that owns it.
        let mark = |position: egui::Pos2| {
            let band = bands::band_at(cached_bands, position)?;
            projection
                .drawing_point_at(
                    position,
                    pointer.history_right,
                    pointer.total,
                    pointer.magnet,
                    drawings::AnchorSnap::Pointer,
                    band,
                )
                .and_then(|point| Some((point.time_ms?, point.price)))
        };

        if pointer.pressed
            && !pointer.over_chrome
            && let Some((position, pick)) = pointer
                .position
                .filter(|position| pointer.area.contains(*position))
                .zip(shared_pick)
        {
            // Selecting is not moving (§D9): the press takes the object
            // whether or not the drag that may follow is allowed.
            outcome.shared.owner = Some(pick.owner);
            outcome.shared.edit = Some(SharedEdit::Select(pick.index));
            self.shared_drag_owner = Some(pick.owner);
            self.shared_drag_pending_from = Some(position);
            self.shared_pointer_mark = mark(position);
            self.shared_drag = if pick.locked {
                SharedDrag::Blocked
            } else {
                outcome.shared.begin_gesture = true;
                match pick.anchor {
                    Some(anchor) => SharedDrag::Anchor {
                        index: pick.index,
                        anchor,
                    },
                    None => SharedDrag::Body { index: pick.index },
                }
            };
            return;
        }

        if !self.shared_drag.is_active() {
            // Hover feedback, so a mirrored mark does not feel deader than the
            // object it is: the same three cursors its own pane shows.
            if !pointer.over_chrome
                && pointer
                    .position
                    .is_some_and(|position| pointer.area.contains(position))
                && let Some(pick) = shared_pick
            {
                outcome.set_cursor(match (pick.locked, pick.anchor) {
                    (true, _) => egui::CursorIcon::NotAllowed,
                    (false, Some(_)) => egui::CursorIcon::ResizeNwSe,
                    (false, None) => egui::CursorIcon::Move,
                });
            }
            return;
        }

        if pointer.released {
            outcome.shared.owner = self.shared_drag_owner;
            outcome.shared.commit_gesture = true;
            self.shared_drag = SharedDrag::None;
            self.shared_drag_owner = None;
            self.shared_drag_pending_from = None;
            self.shared_pointer_mark = None;
            return;
        }
        if !pointer.down {
            return;
        }
        // Under the threshold the object does not move at all, so a click on
        // the mirror stays a click.
        if let Some(origin) = self.shared_drag_pending_from {
            let travelled = pointer
                .position
                .is_some_and(|position| (position - origin).length() >= DRAWING_DRAG_THRESHOLD_PX);
            if !travelled {
                return;
            }
            self.shared_drag_pending_from = None;
        }
        // Clamped, not filtered: the gesture is already ours, and it keeps
        // working while the pointer travels off the pane — over the inspector
        // that this very press opened, most of all.
        let Some(position) = pointer.position.map(|position| {
            egui::pos2(
                position.x.clamp(pointer.area.left(), pointer.area.right()),
                position.y.clamp(pointer.area.top(), pointer.area.bottom()),
            )
        }) else {
            return;
        };
        // A pointer over the empty space past the newest bar of a tick or
        // volume chart names no instant, and none is invented: the mark holds
        // still for that frame rather than jumping to a guess.
        let Some((time_ms, price)) = mark(position) else {
            return;
        };
        // Every edit this gesture emits belongs to the pane the gesture took
        // hold of, whatever the pointer is over now.
        outcome.shared.owner = self.shared_drag_owner;
        match self.shared_drag {
            SharedDrag::Anchor { index, anchor } => {
                outcome.shared.edit = Some(SharedEdit::MoveAnchor {
                    index,
                    anchor,
                    time_ms,
                    price,
                });
                outcome.set_cursor(egui::CursorIcon::ResizeNwSe);
            }
            SharedDrag::Body { index } => {
                if let Some((last_time, last_price)) = self.shared_pointer_mark {
                    outcome.shared.edit = Some(SharedEdit::Translate {
                        index,
                        delta_ms: time_ms - last_time,
                        delta_price: price - last_price,
                    });
                }
                outcome.set_cursor(egui::CursorIcon::Move);
            }
            SharedDrag::Blocked => outcome.set_cursor(egui::CursorIcon::NotAllowed),
            SharedDrag::None => {}
        }
        self.shared_pointer_mark = Some((time_ms, price));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::drawing_projection::PaneSeriesRead;
    use crate::state::{BarSpec, ChartState, SpecSelector};

    #[test]
    fn shared_release_keeps_the_press_owner_even_when_the_pick_changes() {
        let spec = SpecSelector::new(BarSpec::Tick(10));
        let state = ChartState::new(BarSpec::Tick(10));
        let viewport = crate::viewport::Viewport::new();
        let indicators = crate::indicators::IndicatorViews::new();
        let projection = DrawingProjection {
            series: PaneSeriesRead {
                history_prefix: &[],
                state: &state,
                spec: &spec,
            },
            viewport: &viewport,
            indicators: &indicators,
        };
        let area = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(20.0, 20.0));
        let pointer = |pressed, released, over_chrome| SharedPointer {
            position: Some(area.center()),
            area,
            over_chrome,
            pressed,
            down: pressed,
            released,
            history_right: area.right(),
            total: 0,
            magnet: false,
        };
        let bands = Bands::new();
        for locked in [false, true] {
            let mut gestures = PaneGestures::default();
            let pick = SharedPick {
                owner: 7,
                index: 3,
                anchor: None,
                locked,
            };
            let mut denied = PointerOutcome::default();
            gestures.interact_shared(
                &projection,
                &bands,
                Some(pick),
                &mut denied,
                pointer(true, false, true),
            );
            assert_eq!(denied.shared, SharedInteraction::default());
            assert!(!gestures.shared_drag.is_active());

            let mut press = PointerOutcome::default();
            gestures.interact_shared(
                &projection,
                &bands,
                Some(pick),
                &mut press,
                pointer(true, false, false),
            );
            assert_eq!(press.shared.owner, Some(7));
            assert_eq!(press.shared.edit, Some(SharedEdit::Select(3)));
            assert_eq!(press.shared.begin_gesture, !locked);
            assert!(gestures.shared_drag.is_active());
            let mut release = PointerOutcome::default();
            gestures.interact_shared(
                &projection,
                &bands,
                Some(SharedPick { owner: 9, ..pick }),
                &mut release,
                pointer(false, true, true),
            );
            assert_eq!(release.shared.owner, Some(7));
            assert!(release.shared.commit_gesture);
            assert_eq!(release.shared.edit, None);
            assert!(!gestures.shared_drag.is_active());
            assert_eq!(gestures.shared_drag_owner, None);
            assert_eq!(gestures.shared_drag_pending_from, None);
            assert_eq!(gestures.shared_pointer_mark, None);
        }
    }
}
