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

#[derive(Clone, Copy)]
struct LocalPick<'a> {
    position: egui::Pos2,
    band: Option<&'a bands::Band>,
    history_right: f32,
    total: usize,
}

struct LocalDragFrame<'a> {
    ui: &'a egui::Ui,
    bands: &'a Bands,
    pointer: &'a SharedPointer,
}

impl PaneGestures {
    pub(super) fn handle_pointer_tool(
        &mut self,
        drawings: &mut drawings::Drawings,
        projection: &DrawingProjection<'_>,
        frame: PointerFrame<'_>,
    ) -> PointerOutcome {
        let mut outcome = PointerOutcome {
            shared: frame.shared,
            ..Default::default()
        };
        if frame.paper_gesture || frame.tool != Tool::Pointer {
            self.yield_pointer();
            return outcome;
        }
        let pointer = frame.pointer;
        // Local picks use the current carve; shared marks use the last paint.
        let pointer_band = pointer
            .position
            .filter(|_| !pointer.over_chrome)
            .filter(|position| !pane_chrome_hit(frame.areas, *position))
            .and_then(|position| bands::band_at(frame.bands, position))
            .filter(|band| band.drawable());
        let pick = pointer.position.map(|position| LocalPick {
            position,
            band: pointer_band,
            history_right: pointer.history_right,
            total: pointer.total,
        });
        if let Some(pick) = pick {
            if let Some(cursor) = local_hover_cursor(drawings, projection, pick) {
                outcome.set_cursor(cursor);
            }
            // Release-as-click precedes a new press. The press answer survives
            // inspector resize, including a captured miss (Some(None)).
            if pointer.released && self.drag_pending_from.is_some() {
                outcome.begin_text_edit = self.select_released_press(
                    drawings,
                    projection,
                    pick,
                    frame.ui.input(|input| input.modifiers.alt),
                    frame.chart.double_clicked(),
                );
            }
        }
        let started = pointer.pressed
            && pick.is_some_and(|pick| self.begin_local_drag(drawings, projection, pick));
        // Keep threshold calculation unconditional and before initiation gating.
        let travel = self.local_travel(pointer.position, frame.pointer_delta);
        self.advance_local_drag(
            drawings,
            projection,
            LocalDragFrame {
                ui: frame.ui,
                bands: frame.bands,
                pointer,
            },
            if started { None } else { travel },
            &mut outcome,
        );
        if !self.drag.is_active() {
            self.interact_shared(
                projection,
                frame.cached_bands,
                frame.shared_pick,
                &mut outcome,
                SharedPointer {
                    position: pointer.position,
                    area: pointer.area,
                    over_chrome: pointer.over_chrome,
                    pressed: pointer.pressed,
                    down: pointer.down,
                    released: pointer.released,
                    history_right: pointer.history_right,
                    total: pointer.total,
                    magnet: pointer.magnet,
                },
            );
        }
        // Local release remains consumed even after its state is cleared.
        outcome.consumed = self.drag.is_active() || self.shared_drag.is_active();
        if pointer.released {
            self.finish_local_release(drawings);
        }
        outcome
    }

    fn select_released_press(
        &mut self,
        drawings: &mut drawings::Drawings,
        projection: &DrawingProjection<'_>,
        pick: LocalPick<'_>,
        alt: bool,
        double_clicked: bool,
    ) -> bool {
        let LocalPick {
            position,
            band: pointer_band,
            history_right,
            total,
        } = pick;
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
        let selected = if alt {
            pointer_band.and_then(|band| {
                projection.drawing_below_selection(drawings, position, band, history_right, total)
            })
        } else {
            self.press_pick.take().unwrap_or_else(|| {
                // No press was recorded (it landed on chrome, or off
                // any band): fall back to asking now.
                pointer_band.and_then(|band| {
                    projection.drawing_pick_at(drawings, position, band, history_right, total)
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
        if double_clicked
            && let Some(index) = selected
            && drawings
                .items()
                .get(index)
                .is_some_and(|drawing| drawing.tool.holds_text() && !drawing.locked)
        {
            self.content_editing = Some(index);
            return true;
        }
        false
    }

    fn begin_local_drag(
        &mut self,
        drawings: &mut drawings::Drawings,
        projection: &DrawingProjection<'_>,
        pick: LocalPick<'_>,
    ) -> bool {
        let LocalPick {
            position,
            band,
            history_right,
            total,
        } = pick;
        let Some(band) = band else {
            return false;
        };
        // One question, asked once, on the geometry the user was
        // actually looking at when they pressed.
        self.press_pick =
            Some(projection.drawing_pick_at(drawings, position, band, history_right, total));
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
        self.drag.is_active()
    }

    fn local_travel(
        &mut self,
        pointer_position: Option<egui::Pos2>,
        pointer_delta: egui::Vec2,
    ) -> Option<egui::Vec2> {
        match (self.drag_pending_from, pointer_position) {
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
        }
    }

    fn advance_local_drag(
        &self,
        drawings: &mut drawings::Drawings,
        projection: &DrawingProjection<'_>,
        frame: LocalDragFrame<'_>,
        travel: Option<egui::Vec2>,
        outcome: &mut PointerOutcome,
    ) {
        if !frame.pointer.down {
            return;
        }
        let Some(travel) = travel else {
            return;
        };
        match self.drag {
            DrawingDrag::Handle {
                drawing_index,
                handle,
            } => move_handle(
                drawings,
                projection,
                &frame,
                (drawing_index, handle),
                outcome,
            ),
            DrawingDrag::Translate => translate_body(drawings, projection, frame.bands, travel),
            DrawingDrag::Blocked => outcome.set_cursor(egui::CursorIcon::NotAllowed),
            DrawingDrag::None => {}
        }
    }

    fn finish_local_release(&mut self, drawings: &mut drawings::Drawings) {
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

    fn yield_pointer(&mut self) {
        self.drag = DrawingDrag::None;
        self.press_pick = None;
        self.drag_pending_from = None;
        self.shared_drag = SharedDrag::None;
        self.shared_drag_pending_from = None;
        self.shared_pointer_mark = None;
    }

    fn interact_shared(
        &mut self,
        projection: &DrawingProjection<'_>,
        cached_bands: &Bands,
        shared_pick: Option<SharedPick>,
        outcome: &mut PointerOutcome,
        pointer: SharedPointer,
    ) {
        if self.begin_shared_drag(projection, cached_bands, shared_pick, outcome, &pointer) {
            return;
        }
        if !self.shared_drag.is_active() {
            if let Some(cursor) = shared_hover_cursor(&pointer, shared_pick) {
                outcome.set_cursor(cursor);
            }
            return;
        }
        // Shared cleanup intentionally precedes the combined consumed answer.
        if pointer.released {
            self.finish_shared_release(outcome);
            return;
        }
        if !pointer.down || !self.shared_threshold_crossed(pointer.position) {
            return;
        }
        // Held gestures clamp into the prior paint's carve; no future tick
        // instant is invented when the pointer has no market-time anchor.
        let Some(position) = pointer.position.map(|position| {
            egui::pos2(
                position.x.clamp(pointer.area.left(), pointer.area.right()),
                position.y.clamp(pointer.area.top(), pointer.area.bottom()),
            )
        }) else {
            return;
        };
        let Some((time_ms, price)) = shared_mark_at(projection, cached_bands, &pointer, position)
        else {
            return;
        };
        self.advance_shared_drag(time_ms, price, outcome);
    }

    fn begin_shared_drag(
        &mut self,
        projection: &DrawingProjection<'_>,
        cached_bands: &Bands,
        shared_pick: Option<SharedPick>,
        outcome: &mut PointerOutcome,
        pointer: &SharedPointer,
    ) -> bool {
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
            self.shared_pointer_mark = shared_mark_at(projection, cached_bands, pointer, position);
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
            return true;
        }
        false
    }

    fn finish_shared_release(&mut self, outcome: &mut PointerOutcome) {
        outcome.shared.owner = self.shared_drag_owner;
        outcome.shared.commit_gesture = true;
        self.shared_drag = SharedDrag::None;
        self.shared_drag_owner = None;
        self.shared_drag_pending_from = None;
        self.shared_pointer_mark = None;
    }

    fn shared_threshold_crossed(&mut self, position: Option<egui::Pos2>) -> bool {
        if let Some(origin) = self.shared_drag_pending_from {
            let travelled = position
                .is_some_and(|position| (position - origin).length() >= DRAWING_DRAG_THRESHOLD_PX);
            if !travelled {
                return false;
            }
            self.shared_drag_pending_from = None;
        }
        true
    }

    fn advance_shared_drag(&mut self, time_ms: i64, price: f64, outcome: &mut PointerOutcome) {
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

fn local_hover_cursor(
    drawings: &drawings::Drawings,
    projection: &DrawingProjection<'_>,
    pick: LocalPick<'_>,
) -> Option<egui::CursorIcon> {
    let LocalPick {
        position,
        band,
        history_right,
        total,
    } = pick;
    let band = band?;
    if let Some(selected) = drawings.selected()
        && projection
            .drawing_handle_in(drawings, selected, position, band, history_right, total)
            .is_some()
    {
        return Some(if drawings.items()[selected].locked {
            egui::CursorIcon::NotAllowed
        } else {
            egui::CursorIcon::ResizeNwSe
        });
    } else if let Some(hovered) =
        projection.drawing_at(drawings, position, band, history_right, total)
    {
        return Some(if drawings.items()[hovered].locked {
            egui::CursorIcon::NotAllowed
        } else {
            egui::CursorIcon::Move
        });
    }
    None
}

fn move_handle(
    drawings: &mut drawings::Drawings,
    projection: &DrawingProjection<'_>,
    frame: &LocalDragFrame<'_>,
    (drawing_index, handle): (usize, usize),
    outcome: &mut PointerOutcome,
) {
    let bands = frame.bands;
    let pointer_position = frame.pointer.position;
    let history_right = frame.pointer.history_right;
    let total = frame.pointer.total;
    let magnet = frame.pointer.magnet;
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
        if let Some(point) =
            projection.drawing_point_at(position, history_right, total, magnet, handle_snap, band)
        {
            projection.drag_drawing_handle(
                drawings,
                drawing_index,
                handle,
                point,
                band,
                history_right,
                total,
                if frame.ui.input(|input| input.modifiers.shift) {
                    drawings::Constrain::Level
                } else {
                    drawings::Constrain::Free
                },
            );
        }
        outcome.set_cursor(egui::CursorIcon::ResizeNwSe);
    }
}

fn translate_body(
    drawings: &mut drawings::Drawings,
    projection: &DrawingProjection<'_>,
    bands: &Bands,
    travel: egui::Vec2,
) {
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
        let delta_value = sign * f64::from(travel.y / band.rect.height()) * (hi - lo);
        drawings.translate_selected(delta_bar, delta_value);
        // Market time is what every other pane reads the
        // object through; a move that left it behind
        // would drag the mark here and leave its shared
        // twin standing where it used to be.
        projection.retime_selected(drawings);
    }
}

fn shared_mark_at(
    projection: &DrawingProjection<'_>,
    cached_bands: &Bands,
    pointer: &SharedPointer,
    position: egui::Pos2,
) -> Option<(i64, f64)> {
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
}

fn shared_hover_cursor(
    pointer: &SharedPointer,
    shared_pick: Option<SharedPick>,
) -> Option<egui::CursorIcon> {
    // Hover feedback, so a mirrored mark does not feel deader than the
    // object it is: the same three cursors its own pane shows.
    if !pointer.over_chrome
        && pointer
            .position
            .is_some_and(|position| pointer.area.contains(position))
        && let Some(pick) = shared_pick
    {
        return Some(match (pick.locked, pick.anchor) {
            (true, _) => egui::CursorIcon::NotAllowed,
            (false, Some(_)) => egui::CursorIcon::ResizeNwSe,
            (false, None) => egui::CursorIcon::Move,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pane::drawing_projection::PaneSeriesRead;
    use crate::state::{BarSpec, ChartState, SpecSelector};

    fn with_projection(test: impl FnOnce(&DrawingProjection<'_>, &Bands)) {
        let spec = SpecSelector::new(BarSpec::Tick(10));
        let mut state = ChartState::new(BarSpec::Tick(10));
        for agg_id in 0..10 {
            state.ingest_live(&quantick_engine::Trade {
                agg_id,
                timestamp_ms: 1000 + agg_id as i64 * 100,
                price: 50.into(),
                quantity: 1.into(),
                side: quantick_engine::Side::Buy,
            });
        }
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
        let bands = smallvec::smallvec![bands::Band {
            key: drawings::DrawingBand::Price,
            rect: egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(400.0, 200.0)),
            scale: Some(crate::chart::PriceScale::from_range(0.0, 100.0, 0.0, 200.0)),
            label: "price".into(),
            refusal: None,
        }];
        test(&projection, &bands);
    }

    fn pointer_at(position: egui::Pos2, bands: &Bands) -> SharedPointer {
        SharedPointer {
            position: Some(position),
            area: bands[0].rect,
            over_chrome: false,
            pressed: false,
            down: true,
            released: false,
            history_right: 200.0,
            total: 1,
            magnet: false,
        }
    }

    fn local_line() -> drawings::Drawings {
        let mut store = drawings::Drawings::default();
        assert!(store.place(
            drawings::DrawingTool::by_id("horizontal-line").unwrap(),
            drawings::ChartPoint::at_time(0.0, 50.0, Some(1000))
        ));
        store.select(None);
        store
    }

    fn owner_frame(
        gestures: &mut PaneGestures,
        drawings: &mut drawings::Drawings,
        projection: &DrawingProjection<'_>,
        bands: &Bands,
        pointer: &SharedPointer,
        delta: egui::Vec2,
    ) -> PointerOutcome {
        let ctx = egui::Context::default();
        let mut result = None;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let response = ui.interact(
                    bands[0].rect,
                    egui::Id::new("owner-test"),
                    egui::Sense::click_and_drag(),
                );
                let areas = PlotAreas {
                    chart: bands[0].rect,
                    indicator_panes: Vec::new(),
                    pane_gutters: Vec::new(),
                    live_strip: None,
                    price_gutter: egui::Rect::NOTHING,
                    time_strip: egui::Rect::NOTHING,
                };
                result = Some(gestures.handle_pointer_tool(
                    drawings,
                    projection,
                    PointerFrame {
                        ui,
                        chart: &response,
                        areas: &areas,
                        bands,
                        cached_bands: bands,
                        pointer,
                        pointer_delta: delta,
                        paper_gesture: false,
                        tool: Tool::Pointer,
                        shared_pick: None,
                        shared: SharedInteraction::default(),
                    },
                ));
            });
        });
        result.unwrap()
    }

    #[test]
    fn local_threshold_returns_full_first_travel_then_frame_delta() {
        let origin = egui::pos2(10.0, 20.0);
        let mut gestures = PaneGestures {
            drag_pending_from: Some(origin),
            ..Default::default()
        };
        let delta = egui::vec2(1.0, 2.0);
        assert_eq!(gestures.local_travel(None, delta), None);
        assert_eq!(
            gestures.local_travel(
                Some(origin + egui::vec2(DRAWING_DRAG_THRESHOLD_PX - 0.5, 0.0)),
                delta
            ),
            None
        );
        assert_eq!(gestures.drag_pending_from, Some(origin));
        let full = egui::vec2(DRAWING_DRAG_THRESHOLD_PX, 0.0);
        assert_eq!(
            gestures.local_travel(Some(origin + full), delta),
            Some(full)
        );
        assert_eq!(gestures.drag_pending_from, None);
        assert_eq!(
            gestures.local_travel(Some(origin + full + delta), delta),
            Some(delta)
        );
    }

    #[test]
    fn a_captured_miss_is_consumed_without_reselecting_release_geometry() {
        with_projection(|projection, bands| {
            let mut store = local_line();
            let pick = LocalPick {
                position: egui::pos2(100.0, 100.0),
                band: Some(&bands[0]),
                history_right: 200.0,
                total: 1,
            };
            assert_eq!(
                projection.drawing_pick_at(&store, pick.position, &bands[0], 200.0, 1),
                Some(0)
            );
            let mut gestures = PaneGestures {
                press_pick: Some(None),
                ..Default::default()
            };
            assert!(!gestures.select_released_press(&mut store, projection, pick, false, false));
            assert_eq!(store.selected(), None);
            assert_eq!(gestures.press_pick, None);
            // An absent capture really does use the same release geometry.
            gestures.select_released_press(&mut store, projection, pick, false, false);
            assert_eq!(store.selected(), Some(0));
        });
    }

    #[test]
    fn press_does_not_move_and_local_release_keeps_its_consumed_answer() {
        with_projection(|projection, bands| {
            let mut store = local_line();
            let original = store.items()[0].points.clone();
            let mut gestures = PaneGestures::default();
            let mut pointer = pointer_at(egui::pos2(100.0, 100.0), bands);
            pointer.pressed = true;
            let press = owner_frame(
                &mut gestures,
                &mut store,
                projection,
                bands,
                &pointer,
                egui::vec2(40.0, 30.0),
            );
            assert!(press.consumed);
            assert!(gestures.drag.is_active());
            assert_eq!(store.items()[0].points, original);
            pointer.pressed = false;
            pointer.position = Some(egui::pos2(110.0, 120.0));
            owner_frame(
                &mut gestures,
                &mut store,
                projection,
                bands,
                &pointer,
                egui::vec2(1.0, 1.0),
            );
            assert_ne!(store.items()[0].points, original);
            pointer.down = false;
            pointer.released = true;
            let release = owner_frame(
                &mut gestures,
                &mut store,
                projection,
                bands,
                &pointer,
                egui::Vec2::ZERO,
            );
            assert!(release.consumed);
            assert_eq!(gestures.drag, DrawingDrag::None);
            assert_eq!(gestures.press_pick, None);
            assert_eq!(gestures.drag_pending_from, None);
        });
    }

    #[test]
    fn shared_threshold_and_missing_tick_instant_emit_no_fabricated_edit() {
        with_projection(|projection, bands| {
            let origin = egui::pos2(projection.viewport.x_at_bar_position(0.0, 200.0, 1), 100.0);
            let mut pointer = pointer_at(origin, bands);
            let mut gestures = PaneGestures {
                shared_drag: SharedDrag::Body { index: 3 },
                shared_drag_owner: Some(7),
                shared_drag_pending_from: Some(origin),
                shared_pointer_mark: Some((1000, 50.0)),
                ..Default::default()
            };
            let mut under = PointerOutcome::default();
            gestures.interact_shared(
                projection,
                bands,
                None,
                &mut under,
                pointer_at(origin, bands),
            );
            assert_eq!(under.shared, SharedInteraction::default());
            assert_eq!(gestures.shared_drag_pending_from, Some(origin));
            let future = egui::pos2(projection.viewport.x_at_bar_position(3.0, 200.0, 1), 120.0);
            assert!(bands[0].rect.contains(future));
            let point = projection
                .drawing_point_at(
                    future,
                    200.0,
                    1,
                    false,
                    drawings::AnchorSnap::Pointer,
                    &bands[0],
                )
                .unwrap();
            assert_eq!(
                point.time_ms, None,
                "future tick slot has a value but no market instant"
            );
            pointer.position = Some(future);
            let mut missing = PointerOutcome::default();
            gestures.interact_shared(projection, bands, None, &mut missing, pointer);
            assert_eq!(missing.shared, SharedInteraction::default());
            assert_eq!(gestures.shared_drag_pending_from, None);
            assert_eq!(gestures.shared_pointer_mark, Some((1000, 50.0)));
            assert_eq!(gestures.shared_drag_owner, Some(7));
        });
    }

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
