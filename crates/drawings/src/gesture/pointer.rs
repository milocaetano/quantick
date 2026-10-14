//! Pointer-tool and mirrored-mark updates belong to their retained gesture.
//! Inputs borrow coordinates and the independent store; effects return to chrome.
use super::band::{self, Band};
use super::projection::{ClickKind, DrawingProjection, PressTarget};
use super::{
    DRAWING_DRAG_THRESHOLD_PX, DrawingDrag, GestureCursor, Marks, PaneGestures, SharedDrag,
    SharedInteraction, SharedPick, SharedPointer, TranslateFrom,
};
use crate::{Constrain, DrawingId, PointerHint, Px};

/// One frame of the pointer under the Pointer tool, as the window read it.
pub struct PointerFrame<'a> {
    pub bands: &'a [Band],
    // Shared marks use the prior paint's carve, not the new local input carve.
    pub cached_bands: &'a [Band],
    pub pointer: &'a SharedPointer,
    /// Whether the pointer is on the pane's own chrome — an indicator pane's
    /// chevron, header or divider — which no drawing gesture may take.
    pub on_pane_chrome: bool,
    /// Alt held: a click walks down the z-order.
    pub alt: bool,
    /// Shift held: a handle drag keeps its level.
    pub shift: bool,
    /// The chart reported a double click this frame.
    pub double_clicked: bool,
    pub paper_gesture: bool,
    /// Whether the armed tool is the Pointer; any other tool yields.
    pub pointer_tool: bool,
    pub shared_pick: Option<SharedPick>,
    pub shared: SharedInteraction,
    /// Whether a strategy instance is bound to this drawing, armed or not:
    /// its extent is that strategy's window, so a double click must not
    /// move it.
    pub held: &'a dyn Fn(DrawingId) -> bool,
}

#[derive(Default)]
pub struct PointerOutcome {
    pub consumed: bool,
    pub cursor: Option<GestureCursor>,
    pub begin_text_edit: bool,
    pub shared: SharedInteraction,
}
/// What releasing a press as a click did beyond selecting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseAnswer {
    Plain,
    /// A note was double clicked: type in it.
    TextEdit,
    /// A tool took the click; the chart must not also read it.
    ClickTaken,
    /// A tool took the double click; the chart must not also read it.
    DoubleClickTaken,
}

/// How near the last click's press a press must land for egui's time-only
/// double click to complete that click: a few pixels of hand wander.
const DOUBLE_CLICK_SLOP_PX: f32 = 6.0;

/// The last release the Pointer tool read as a click.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LastClick {
    /// Where it pressed.
    pub press: Px,
    /// The drawing it left selected, if any.
    pub drawing: Option<DrawingId>,
    /// The store's revision right after a tool took it and recorded its
    /// undo step; the double click it begins folds into that step.
    pub step: Option<u64>,
}

impl PointerOutcome {
    pub(super) fn set_cursor(&mut self, cursor: GestureCursor) {
        self.cursor = Some(cursor);
    }
}

#[derive(Clone, Copy)]
struct LocalPick<'a> {
    position: Px,
    band: Option<&'a Band>,
    history_right: f32,
    total: usize,
}

struct LocalDragFrame<'a> {
    shift: bool,
    bands: &'a [Band],
    pointer: &'a SharedPointer,
}

impl PaneGestures {
    pub fn handle_pointer_tool(
        &mut self,
        drawings: &mut Marks,
        projection: &DrawingProjection<'_>,
        frame: PointerFrame<'_>,
    ) -> PointerOutcome {
        let mut outcome = PointerOutcome {
            shared: frame.shared,
            ..Default::default()
        };
        if frame.paper_gesture || !frame.pointer_tool {
            self.yield_pointer();
            return outcome;
        }
        let pointer = frame.pointer;
        // Local picks use the current carve; shared marks use the last paint.
        let pointer_band = pointer
            .position
            .filter(|_| !pointer.over_chrome)
            .filter(|_| !frame.on_pane_chrome)
            .and_then(|position| band::band_at(frame.bands, position))
            .filter(|band| band.drawable());
        let pick = pointer.position.map(|position| LocalPick {
            position,
            band: pointer_band,
            history_right: pointer.history_right,
            total: pointer.total,
        });
        let mut click_taken = false;
        let mut single_click_taken = false;
        let mut click_press = None;
        if let Some(pick) = pick {
            let (cursor, hint) = local_hover_cursor(drawings, projection, pick, frame.held);
            // Alt+click only selects: no band announces an extension.
            let hint = hint.filter(|hint| !(frame.alt && *hint == PointerHint::ExtendSide));
            // The hint's cursor replaces the move grip, never joins it; a
            // drag in flight keeps the cursor it started with.
            let cursor = match hint {
                Some(hint) if !self.drag.is_active() => Some(hint_cursor(hint)),
                _ => cursor,
            };
            if let Some(cursor) = cursor {
                outcome.set_cursor(cursor);
            }
            // Release-as-click precedes a new press. The press answer survives
            // inspector resize, including a captured miss (Some(None)).
            if pointer.released && self.drag_pending_from.is_some() {
                click_press = self.drag_pending_from;
                let answer = self.select_released_press(
                    drawings,
                    projection,
                    pick,
                    frame.alt,
                    frame.held,
                    frame.double_clicked,
                );
                outcome.begin_text_edit = answer == ReleaseAnswer::TextEdit;
                single_click_taken = answer == ReleaseAnswer::ClickTaken;
                click_taken = single_click_taken || answer == ReleaseAnswer::DoubleClickTaken;
            }
        }
        let started = pointer.pressed
            && pick.is_some_and(|pick| self.begin_local_drag(drawings, projection, pick));
        // Keep threshold calculation unconditional and before initiation gating.
        let past_threshold = self.past_drag_threshold(pointer.position);
        self.advance_local_drag(
            drawings,
            projection,
            LocalDragFrame {
                shift: frame.shift,
                bands: frame.bands,
                pointer,
            },
            !started && past_threshold,
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
        outcome.consumed = click_taken || self.drag.is_active() || self.shared_drag.is_active();
        if pointer.released {
            self.finish_local_release(drawings);
            // A click's undo step stays open to the double click it begins.
            self.last_click = click_press.map(|press| LastClick {
                press,
                drawing: drawings.selected_id(),
                step: single_click_taken.then(|| drawings.revision()),
            });
        }
        outcome
    }

    fn select_released_press(
        &mut self,
        drawings: &mut Marks,
        projection: &DrawingProjection<'_>,
        pick: LocalPick<'_>,
        alt: bool,
        // Which drawings a strategy holds: their extent is its window.
        held: &dyn Fn(DrawingId) -> bool,
        double_clicked: bool,
    ) -> ReleaseAnswer {
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
            return ReleaseAnswer::TextEdit;
        }
        // Any other tool may own a click or a double click on its body: a
        // click in a rectangle's band extends that side, a double click
        // inside puts its original size back. A press that grabbed a
        // resize handle stays a resize, Alt+click only walks the z-order,
        // and a drawing a strategy is bound to keeps its extent, that
        // strategy's window. The change arrives already worked out, so
        // selecting and applying it cannot half happen: either the object
        // changes and takes the click, or nothing moves and the chart reads it.
        if alt || matches!(self.drag, DrawingDrag::Handle { .. }) {
            return ReleaseAnswer::Plain;
        }
        let Some(band) = pointer_band else {
            return ReleaseAnswer::Plain;
        };
        let target = |kind| {
            projection.click_target(
                drawings,
                selected,
                position,
                kind,
                band,
                history_right,
                total,
                held,
            )
        };
        // egui's double click is time-only: it completes the last click only
        // when its press lands near that one's, on that click's drawing.
        // Anything else - the left band, then quickly the right - is a click
        // of its own. A first click that hit no drawing names none to match.
        let repeat = self.last_click.filter(|last| {
            double_clicked
                && self
                    .drag_pending_from
                    .is_some_and(|press| press.distance(last.press) <= DOUBLE_CLICK_SLOP_PX)
        });
        let (index, payload, answer, step) =
            match repeat.map(|last| (last, target(ClickKind::Double))) {
                Some((last, Some((index, payload))))
                    if last
                        .drawing
                        .is_none_or(|id| id == drawings.items()[index].id) =>
                {
                    (index, payload, ReleaseAnswer::DoubleClickTaken, last.step)
                }
                // A double click that changes nothing leaves it to the chart.
                Some((_, None)) => return ReleaseAnswer::Plain,
                _ => match target(ClickKind::Single) {
                    Some((index, payload)) => (index, payload, ReleaseAnswer::ClickTaken, None),
                    None => return ReleaseAnswer::Plain,
                },
            };
        // A double click folds into its first click's step: one Ctrl+Z goes
        // back to before that click, never to the state between.
        if let Some(step) = step {
            drawings.fold_into_last_step(step);
        }
        // Baseline first, so the change is one undo step.
        drawings.begin_gesture();
        drawings.items_mut()[index].payload = payload;
        drawings.select(Some(index));
        answer
    }

    fn begin_local_drag(
        &mut self,
        drawings: &mut Marks,
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
        // actually looking at when they pressed - the one hover asks too.
        let target = projection.press_target(drawings, position, band, history_right, total);
        self.press_pick = Some(target.map(PressTarget::index));
        self.drag_pending_from = Some(position);
        self.translate_from = None;
        if let Some(PressTarget::Handle {
            index: drawing_index,
            handle,
        }) = target
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
        } else if let Some(PressTarget::Body { index, .. }) = target {
            drawings.select(Some(index));
            self.drag = if drawings.items()[index].locked {
                DrawingDrag::Blocked
            } else {
                drawings.begin_gesture();
                self.translate_from = Some(TranslateFrom {
                    index,
                    last: position,
                    free: drawings.items()[index].points.clone(),
                });
                DrawingDrag::Translate
            };
        }
        // A press that hits no geometry is not ours to interpret: it
        // belongs to whatever the window routed it to (inspector, manager,
        // chart pan). Deselection happens through the window-routed
        // click above, which already respects floating windows.
        self.drag.is_active()
    }

    /// Whether the pointer has travelled past the drag threshold in this
    /// gesture; both moves then read the pointer's position, not a delta.
    fn past_drag_threshold(&mut self, pointer_position: Option<Px>) -> bool {
        match (self.drag_pending_from, pointer_position) {
            (Some(origin), Some(position)) => {
                let past = (position - origin).length() >= DRAWING_DRAG_THRESHOLD_PX;
                if past {
                    self.drag_pending_from = None;
                }
                past
            }
            // No pending origin: the threshold was passed earlier.
            (None, _) => true,
            (Some(_), None) => false,
        }
    }

    fn advance_local_drag(
        &mut self,
        drawings: &mut Marks,
        projection: &DrawingProjection<'_>,
        frame: LocalDragFrame<'_>,
        past_threshold: bool,
        outcome: &mut PointerOutcome,
    ) {
        if !frame.pointer.down || !past_threshold {
            return;
        }
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
            DrawingDrag::Translate => {
                // A selection that moved off the grabbed drawing, or anchors
                // re-cut under it, ends the move: the object holds still.
                let grabbed = (self.translate_from.as_mut())
                    .filter(|from| drawings.selected() == Some(from.index));
                match grabbed {
                    Some(from) => translate_body(drawings, projection, &frame, from),
                    None => self.drag = DrawingDrag::Blocked,
                }
            }
            DrawingDrag::Blocked => outcome.set_cursor(GestureCursor::NotAllowed),
            DrawingDrag::None => {}
        }
    }

    fn finish_local_release(&mut self, drawings: &mut Marks) {
        // One gesture, one undo entry — recorded only if it moved.
        drawings.commit_gesture();
        self.drag = DrawingDrag::None;
        self.translate_from = None;
        // A press that ended in a drag rather than a click leaves its
        // answer unconsumed; it must not survive to decide the *next*
        // click, which may be somewhere else entirely. The click path
        // above already ran this frame and took it if it was a click.
        self.press_pick = None;
        self.drag_pending_from = None;
    }

    fn yield_pointer(&mut self) {
        self.drag = DrawingDrag::None;
        self.translate_from = None;
        self.press_pick = None;
        self.drag_pending_from = None;
        self.last_click = None;
        self.shared_drag = SharedDrag::None;
        self.shared_drag_pending_from = None;
        self.shared_pointer_mark = None;
        self.shared_drag_magnet = false;
        self.shared_drag_snap = crate::AnchorSnap::default();
    }
}

/// The hover cursor, and the hint the object under the pointer offers.
/// Both read the press's own target, so the cursor names what a press
/// does: a handle answers with its own cursor, a body with the move grip
/// and its tool's hint (none on a drawing a strategy is bound to, which
/// refuses the click). Off every object's hit-test, the hint is the one the
/// click's fallback answers to, so an outline rectangle's bands announce
/// themselves exactly where the click acts.
fn local_hover_cursor(
    drawings: &Marks,
    projection: &DrawingProjection<'_>,
    pick: LocalPick<'_>,
    held: &dyn Fn(DrawingId) -> bool,
) -> (Option<GestureCursor>, Option<PointerHint>) {
    let LocalPick {
        position,
        band,
        history_right,
        total,
    } = pick;
    let Some(band) = band else {
        return (None, None);
    };
    let Some(target) = projection.press_target(drawings, position, band, history_right, total)
    else {
        // No object here takes a press; the cursor stays the chart's own.
        let view = (history_right, total);
        return (
            None,
            projection.missed_click_hint(drawings, position, band, view, held),
        );
    };
    let drawing = &drawings.items()[target.index()];
    if drawing.locked {
        return (Some(GestureCursor::NotAllowed), None);
    }
    match target {
        PressTarget::Handle { handle, .. } => (Some(handle_cursor(drawing, handle)), None),
        // The move grip, unless the tool says the pointer acts otherwise
        // here: the caller then shows that cursor instead.
        PressTarget::Body { hint, .. } => (
            Some(GestureCursor::Move),
            hint.filter(|_| !held(drawing.id)),
        ),
    }
}

/// The cursor that announces a hint: each hint its own.
fn hint_cursor(hint: PointerHint) -> GestureCursor {
    match hint {
        PointerHint::ExtendSide => GestureCursor::ExtendSide,
        PointerHint::ResizeSide { .. } => GestureCursor::ResizeHorizontal,
    }
}

/// The cursor over, or dragging, `handle`: the plain resize unless the tool
/// names a narrower grab.
fn handle_cursor(drawing: &super::Mark, handle: usize) -> GestureCursor {
    drawing
        .tool
        .handle_hint(handle)
        .map_or(GestureCursor::Resize, hint_cursor)
}

fn move_handle(
    drawings: &mut Marks,
    projection: &DrawingProjection<'_>,
    frame: &LocalDragFrame<'_>,
    (drawing_index, handle): (usize, usize),
    outcome: &mut PointerOutcome,
) {
    let bands = frame.bands;
    let pointer_position = frame.pointer.position;
    let history_right = frame.pointer.history_right;
    let total = frame.pointer.total;
    // One read of the grabbed object. Its own band, not the one under the
    // pointer: dragging a CVD anchor up into the candles stretches it to the
    // top of its pane, and never writes a price into a CVD anchor. Moving a
    // mark keeps it glued to a bar's extreme: the rule that placed it is the
    // rule that holds it. The magnet is the rail's, or the object's own.
    let dragged = drawings.items().get(drawing_index).and_then(|drawing| {
        let magnet = frame.pointer.magnet || drawing.own_magnet();
        Some((
            band::band_of(bands, drawing)?,
            drawing.tool.anchor_snap(),
            magnet,
        ))
    });
    if let Some((band, handle_snap, magnet)) = dragged
        && let Some(position) = pointer_position
    {
        let position = Px::new(
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
                if frame.shift {
                    Constrain::Level
                } else {
                    Constrain::Free
                },
            );
        }
        let cursor = drawings
            .items()
            .get(drawing_index)
            .map_or(GestureCursor::Resize, |drawing| {
                handle_cursor(drawing, handle)
            });
        outcome.set_cursor(cursor);
    }
}

/// Move the grabbed drawing by this frame's travel of the hand, then let its
/// anchors land the way placing them would: by the tool's own snap, or with
/// the magnet on, onto the print under the pointer.
fn translate_body(
    drawings: &mut Marks,
    projection: &DrawingProjection<'_>,
    frame: &LocalDragFrame<'_>,
    from: &mut TranslateFrom,
) {
    let pointer = frame.pointer;
    let Some(position) = pointer.position else {
        return;
    };
    let index = from.index;
    let dragged = drawings.items().get(index).and_then(|drawing| {
        let magnet = pointer.magnet || drawing.own_magnet();
        Some((drawing.tool, magnet, band::band_of(frame.bands, drawing)?))
    });
    if let Some((tool, magnet, band)) = dragged
        && let Some(scale) = band.scale
    {
        let (right, total) = (pointer.history_right, pointer.total);
        // Both ends read under this frame's axes — per band, so a CVD level
        // follows the hand, and inverted or not, so the object tracks the
        // pointer rather than the price axis.
        let bar_at = |x| projection.viewport.bar_at_x(x, right, total);
        let delta_bar = bar_at(position.x) - bar_at(from.last.x);
        let delta_value = scale.price_at(position.y) - scale.price_at(from.last.y);
        from.last = position;
        for point in &mut from.free {
            point.bar += delta_bar;
            point.price += delta_value;
            point.time_ms = projection.series.anchor_time(point.bar);
        }
        let mut moved = from.free.clone();
        match tool.anchor_snap() {
            snap if snap.takes_magnet() => {
                if magnet {
                    projection.snap_body(
                        &mut moved,
                        tool.body_snap(),
                        band,
                        position,
                        (right, total),
                    );
                }
            }
            snap => {
                for point in &mut moved {
                    let at = projection.drawing_screen_point(*point, right, total, &scale);
                    if let Some(snapped) =
                        projection.drawing_point_at(at, right, total, false, snap, band)
                    {
                        *point = snapped;
                    }
                }
            }
        }
        drawings.set_points(index, &moved);
        // Market time is what every other pane reads the
        // object through; a move that left it behind
        // would drag the mark here and leave its shared
        // twin standing where it used to be.
        projection.retime_selected(drawings);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bands, PaneSeriesRead, SharedEdit};
    use super::*;
    use crate::PxRect;
    use crate::chart::PriceScale;
    use quantick_chart::indicator_views::IndicatorViews;
    use quantick_chart::state::{BarSpec, ChartState, SpecSelector};
    use quantick_chart_interaction::viewport::Viewport;

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
        let viewport = Viewport::new();
        let indicators = IndicatorViews::new();
        let projection = DrawingProjection {
            series: PaneSeriesRead {
                history_prefix: &[],
                state: &state,
                spec: &spec,
            },
            viewport: &viewport,
            indicators: &indicators,
        };
        let bands = smallvec::smallvec![Band {
            key: crate::DrawingBand::Price,
            rect: PxRect::from_min_max(Px::default(), crate::pos2(400.0, 200.0)),
            scale: Some(PriceScale::from_range(0.0, 100.0, 0.0, 200.0)),
            label: "price".into(),
            refusal: None,
        }];
        test(&projection, &bands);
    }

    fn pointer_at(position: Px, bands: &Bands) -> SharedPointer {
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

    fn local_line() -> Marks {
        let mut store = Marks::default();
        assert!(store.place(
            crate::DrawingTool::by_id("horizontal-line").unwrap(),
            crate::ChartPoint::at_time(0.0, 50.0, Some(1000))
        ));
        store.select(None);
        store
    }

    fn owner_frame(
        gestures: &mut PaneGestures,
        drawings: &mut Marks,
        projection: &DrawingProjection<'_>,
        bands: &Bands,
        pointer: &SharedPointer,
    ) -> PointerOutcome {
        owner_frame_by(
            gestures,
            drawings,
            projection,
            (bands, pointer),
            Hand::PLAIN,
        )
    }

    /// What the hand does beyond the pointer this frame.
    #[derive(Clone, Copy)]
    struct Hand<'a> {
        alt: bool,
        double_clicked: bool,
        held: &'a dyn Fn(DrawingId) -> bool,
    }

    impl Hand<'static> {
        const PLAIN: Self = Self {
            alt: false,
            double_clicked: false,
            held: &|_| false,
        };
    }

    fn owner_frame_by(
        gestures: &mut PaneGestures,
        drawings: &mut Marks,
        projection: &DrawingProjection<'_>,
        (bands, pointer): (&Bands, &SharedPointer),
        hand: Hand<'_>,
    ) -> PointerOutcome {
        gestures.handle_pointer_tool(
            drawings,
            projection,
            PointerFrame {
                bands,
                cached_bands: bands,
                pointer,
                on_pane_chrome: false,
                alt: hand.alt,
                shift: false,
                double_clicked: hand.double_clicked,
                paper_gesture: false,
                pointer_tool: true,
                shared_pick: None,
                shared: SharedInteraction::default(),
                held: hand.held,
            },
        )
    }

    #[test]
    fn local_threshold_returns_full_first_travel_then_frame_delta() {
        let origin = crate::pos2(10.0, 20.0);
        let mut gestures = PaneGestures {
            drag_pending_from: Some(origin),
            ..Default::default()
        };
        assert!(!gestures.past_drag_threshold(None));
        assert!(!gestures.past_drag_threshold(Some(
            origin + crate::vec2(DRAWING_DRAG_THRESHOLD_PX - 0.5, 0.0)
        )));
        assert_eq!(gestures.drag_pending_from, Some(origin));
        let full = crate::vec2(DRAWING_DRAG_THRESHOLD_PX, 0.0);
        assert!(gestures.past_drag_threshold(Some(origin + full)));
        assert_eq!(gestures.drag_pending_from, None);
        assert!(gestures.past_drag_threshold(Some(origin)));
    }

    #[test]
    fn a_captured_miss_is_consumed_without_reselecting_release_geometry() {
        with_projection(|projection, bands| {
            let mut store = local_line();
            let pick = LocalPick {
                position: crate::pos2(100.0, 100.0),
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
            assert_eq!(
                gestures.select_released_press(
                    &mut store,
                    projection,
                    pick,
                    false,
                    &|_| false,
                    false
                ),
                ReleaseAnswer::Plain
            );
            assert_eq!(store.selected(), None);
            assert_eq!(gestures.press_pick, None);
            // An absent capture really does use the same release geometry.
            gestures.select_released_press(&mut store, projection, pick, false, &|_| false, false);
            assert_eq!(store.selected(), Some(0));
        });
    }

    #[test]
    fn press_does_not_move_and_local_release_keeps_its_consumed_answer() {
        with_projection(|projection, bands| {
            let mut store = local_line();
            let original = store.items()[0].points.clone();
            let mut gestures = PaneGestures::default();
            let mut pointer = pointer_at(crate::pos2(100.0, 100.0), bands);
            pointer.pressed = true;
            let press = owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert!(press.consumed);
            assert!(gestures.drag.is_active());
            assert_eq!(store.items()[0].points, original);
            pointer.pressed = false;
            pointer.position = Some(crate::pos2(110.0, 120.0));
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert_ne!(store.items()[0].points, original);
            pointer.down = false;
            pointer.released = true;
            let release = owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert!(release.consumed);
            assert_eq!(gestures.drag, DrawingDrag::None);
            assert_eq!(gestures.press_pick, None);
            assert_eq!(gestures.drag_pending_from, None);
        });
    }

    #[test]
    fn shared_threshold_and_missing_tick_instant_emit_no_fabricated_edit() {
        with_projection(|projection, bands| {
            let origin = crate::pos2(projection.viewport.x_at_bar_position(0.0, 200.0, 1), 100.0);
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
            let future = crate::pos2(projection.viewport.x_at_bar_position(3.0, 200.0, 1), 120.0);
            assert!(bands[0].rect.contains(future));
            let point = projection
                .drawing_point_at(
                    future,
                    200.0,
                    1,
                    false,
                    crate::AnchorSnap::Pointer,
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
        let viewport = Viewport::new();
        let indicators = IndicatorViews::new();
        let projection = DrawingProjection {
            series: PaneSeriesRead {
                history_prefix: &[],
                state: &state,
                spec: &spec,
            },
            viewport: &viewport,
            indicators: &indicators,
        };
        let area = PxRect::from_min_max(Px::default(), crate::pos2(20.0, 20.0));
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
                magnet: false,
                snap: crate::AnchorSnap::Pointer,
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

    /// An outline-only rectangle wide enough for both side zones and a
    /// centre, with its drawn screen rect.
    fn local_rectangle(projection: &DrawingProjection<'_>, bands: &Bands) -> (Marks, PxRect) {
        let mut store = Marks::default();
        let rectangle = crate::DrawingTool::by_id("rectangle").unwrap();
        assert!(!store.place(rectangle, crate::ChartPoint::at(-20.0, 70.0)));
        assert!(store.place(rectangle, crate::ChartPoint::at(0.0, 30.0)));
        store.select(None);
        let scale = bands[0].scale.as_ref().unwrap();
        let points = projection.projected_drawing_points(&store.items()[0], 200.0, 1, scale);
        let rect = PxRect::from_two_pos(points[0], points[1]);
        assert!(
            rect.width() > 120.0 && bands[0].rect.contains_rect(rect),
            "{rect:?}"
        );
        (store, rect)
    }

    fn extent(store: &Marks) -> (bool, bool) {
        extent_of(store, 0)
    }

    fn extent_of(store: &Marks, index: usize) -> (bool, bool) {
        let payload = store.items()[index]
            .payload
            .as_any()
            .downcast_ref::<crate::RectanglePayload>()
            .unwrap();
        (payload.extend_left, payload.extend_right)
    }

    fn set_extent(store: &mut Marks, left: bool, right: bool) {
        set_extent_of(store, 0, (left, right));
    }

    fn set_extent_of(store: &mut Marks, index: usize, (left, right): (bool, bool)) {
        let payload = store.items_mut()[index]
            .payload
            .as_any_mut()
            .downcast_mut::<crate::RectanglePayload>()
            .unwrap();
        (payload.extend_left, payload.extend_right) = (left, right);
    }

    /// One click released at `position`, the press having picked `picked`.
    fn release_click(
        store: &mut Marks,
        projection: &DrawingProjection<'_>,
        band: &Band,
        (position, picked): (Px, Option<usize>),
        held: &dyn Fn(DrawingId) -> bool,
        double_clicked: bool,
    ) -> ReleaseAnswer {
        // A double click completes a first click pressed on the same spot.
        let mut gestures = PaneGestures {
            press_pick: Some(picked),
            drag_pending_from: Some(position),
            last_click: double_clicked.then_some(LastClick {
                press: position,
                drawing: None,
                step: None,
            }),
            ..Default::default()
        };
        let pick = LocalPick {
            position,
            band: Some(band),
            history_right: 200.0,
            total: 1,
        };
        gestures.select_released_press(store, projection, pick, false, held, double_clicked)
    }

    fn missed_click(
        store: &mut Marks,
        projection: &DrawingProjection<'_>,
        band: &Band,
        position: Px,
    ) -> ReleaseAnswer {
        release_click(store, projection, band, (position, None), &|_| false, false)
    }

    fn missed_double_click(
        store: &mut Marks,
        projection: &DrawingProjection<'_>,
        band: &Band,
        position: Px,
    ) -> ReleaseAnswer {
        release_click(store, projection, band, (position, None), &|_| false, true)
    }

    fn hover(
        store: &mut Marks,
        projection: &DrawingProjection<'_>,
        bands: &Bands,
        position: Px,
    ) -> Option<GestureCursor> {
        hover_by(store, projection, (bands, position), &|_| false)
    }

    fn hover_by(
        store: &mut Marks,
        projection: &DrawingProjection<'_>,
        (bands, position): (&Bands, Px),
        held: &dyn Fn(DrawingId) -> bool,
    ) -> Option<GestureCursor> {
        let mut pointer = pointer_at(position, bands);
        pointer.down = false;
        let hand = Hand {
            held,
            ..Hand::PLAIN
        };
        owner_frame_by(
            &mut PaneGestures::default(),
            store,
            projection,
            (bands, &pointer),
            hand,
        )
        .cursor
    }

    /// A press and release at `position`, as the window reports them.
    fn press_and_release(
        gestures: &mut PaneGestures,
        store: &mut Marks,
        projection: &DrawingProjection<'_>,
        (bands, position): (&Bands, Px),
        hand: Hand<'_>,
    ) -> PointerOutcome {
        let mut pointer = pointer_at(position, bands);
        pointer.pressed = true;
        owner_frame_by(gestures, store, projection, (bands, &pointer), hand);
        pointer.pressed = false;
        pointer.down = false;
        pointer.released = true;
        owner_frame_by(gestures, store, projection, (bands, &pointer), hand)
    }

    /// Place a rectangle from `from` to `to`, unselected, behind nothing.
    fn place_rectangle(store: &mut Marks, from: crate::ChartPoint, to: crate::ChartPoint) {
        let rectangle = crate::DrawingTool::by_id("rectangle").unwrap();
        assert!(!store.place(rectangle, from));
        assert!(store.place(rectangle, to));
        store.select(None);
    }

    fn projected_rect(
        store: &Marks,
        index: usize,
        projection: &DrawingProjection<'_>,
        bands: &Bands,
    ) -> PxRect {
        let scale = bands[0].scale.as_ref().unwrap();
        let points = projection.projected_drawing_points(&store.items()[index], 200.0, 1, scale);
        PxRect::from_two_pos(points[0], points[1])
    }

    fn drawn_rect(store: &Marks, projection: &DrawingProjection<'_>, bands: &Bands) -> PxRect {
        let scale = bands[0].scale.as_ref().unwrap();
        let points = projection.projected_drawing_points(&store.items()[0], 200.0, 1, scale);
        PxRect::from_two_pos(points[0], points[1])
    }

    /// An outline rectangle's band takes one click even past the reach of
    /// its border; its inside leaves a single click to the chart.
    #[test]
    fn one_click_in_an_outline_rectangles_band_extends_that_side() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 0;
            let band = crate::pos2(rect.right() - 15.0, rect.center().y);
            assert_eq!(
                missed_click(&mut store, projection, &bands[0], band),
                ReleaseAnswer::ClickTaken
            );
            assert_eq!(extent(&store), (false, true));
            assert_eq!(store.selected(), Some(0));
            store.select(None);
            assert_eq!(
                missed_click(&mut store, projection, &bands[0], rect.center()),
                ReleaseAnswer::Plain
            );
            assert_eq!(store.selected(), None, "the inside's click falls through");
            assert_eq!(extent(&store), (false, true));
        });
    }

    #[test]
    fn a_double_click_inside_puts_back_the_original_size_and_a_click_does_not() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 0;
            set_extent(&mut store, true, true);
            assert_eq!(
                missed_click(&mut store, projection, &bands[0], rect.center()),
                ReleaseAnswer::Plain
            );
            assert_eq!(extent(&store), (true, true));
            assert_eq!(
                missed_double_click(&mut store, projection, &bands[0], rect.center()),
                ReleaseAnswer::DoubleClickTaken
            );
            assert_eq!(extent(&store), (false, false));
            assert_eq!(store.selected(), Some(0));
        });
    }

    #[test]
    fn a_click_leaves_a_strategy_bound_region_alone() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            set_extent(&mut store, false, true);
            let bound = store.items()[0].id;
            let held: &dyn Fn(DrawingId) -> bool = &move |id| id == bound;
            let band = crate::pos2(rect.left() + 10.0, rect.center().y);
            for (position, double_clicked) in [(rect.center(), true), (band, false), (band, true)] {
                assert_eq!(
                    release_click(
                        &mut store,
                        projection,
                        &bands[0],
                        (position, Some(0)),
                        held,
                        double_clicked
                    ),
                    ReleaseAnswer::Plain
                );
                assert_eq!(
                    extent(&store),
                    (false, true),
                    "the bot's window held at {position:?}"
                );
            }
        });
    }

    /// The sketch's zones, each with its own cursor: the border line resizes
    /// sideways, the band just inside it says a click extends, the inside
    /// moves. No two zones share a cursor.
    #[test]
    fn each_zone_shows_its_own_cursor() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 40;
            let y = rect.center().y;
            let zones = [
                (rect.left(), GestureCursor::ResizeHorizontal),
                (rect.left() + 10.0, GestureCursor::ExtendSide),
                (rect.center().x, GestureCursor::Move),
                (rect.right() - 10.0, GestureCursor::ExtendSide),
                (rect.right(), GestureCursor::ResizeHorizontal),
            ];
            for (x, cursor) in zones {
                let position = crate::pos2(x, y);
                assert_eq!(
                    hover(&mut store, projection, bands, position),
                    Some(cursor),
                    "at {position:?}"
                );
            }
            // Unselected, a corner already resizes on a press: it says so.
            assert_eq!(
                hover(&mut store, projection, bands, rect.left_top()),
                Some(GestureCursor::Resize)
            );
            // Selected, the side line is the side's handle and keeps its own
            // cursor; the corner keeps the corner resize.
            store.select(Some(0));
            let side = crate::pos2(rect.left(), y);
            assert_eq!(
                hover(&mut store, projection, bands, side),
                Some(GestureCursor::ResizeHorizontal)
            );
            assert_eq!(
                hover(&mut store, projection, bands, rect.left_top()),
                Some(GestureCursor::Resize)
            );
        });
    }

    #[test]
    fn dragging_a_side_line_moves_that_side_alone_under_its_own_cursor() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            let mut gestures = PaneGestures::default();
            let mut pointer = pointer_at(crate::pos2(rect.left(), rect.center().y), bands);
            pointer.pressed = true;
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert!(
                matches!(gestures.drag, DrawingDrag::Handle { .. }),
                "{:?}",
                gestures.drag
            );
            pointer.pressed = false;
            pointer.position = Some(crate::pos2(rect.left() - 15.0, rect.center().y + 20.0));
            let drag = owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert_eq!(drag.cursor, Some(GestureCursor::ResizeHorizontal));
            pointer.down = false;
            pointer.released = true;
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            let moved = drawn_rect(&store, projection, bands);
            assert!(moved.left() < rect.left() - 5.0, "{moved:?} from {rect:?}");
            assert!((moved.right() - rect.right()).abs() < 0.01, "{moved:?}");
            assert!((moved.top() - rect.top()).abs() < 0.01, "{moved:?}");
            assert!((moved.bottom() - rect.bottom()).abs() < 0.01, "{moved:?}");
            assert_eq!(extent(&store), (false, false));
        });
    }

    /// A press and release in the band is one click: it extends. A press
    /// that drags from the band moves the rectangle, as anywhere inside.
    #[test]
    fn a_click_in_the_band_extends_and_a_drag_from_it_moves() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 40;
            let band = crate::pos2(rect.right() - 10.0, rect.center().y);
            let mut gestures = PaneGestures::default();
            let mut pointer = pointer_at(band, bands);
            pointer.pressed = true;
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            pointer.pressed = false;
            pointer.down = false;
            pointer.released = true;
            let release = owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert!(release.consumed);
            assert_eq!(extent(&store), (false, true));

            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 40;
            let mut gestures = PaneGestures::default();
            let mut pointer = pointer_at(band, bands);
            pointer.pressed = true;
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            pointer.pressed = false;
            pointer.position = Some(crate::pos2(band.x - 30.0, band.y));
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            pointer.down = false;
            pointer.released = true;
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert_eq!(extent(&store), (false, false), "a drag never extends");
            let moved = drawn_rect(&store, projection, bands);
            assert!(moved.left() < rect.left() - 20.0, "{moved:?} from {rect:?}");
            assert!(
                moved.right() < rect.right() - 20.0,
                "{moved:?} from {rect:?}"
            );
        });
    }

    /// A double click whose press missed every object reaches a rectangle
    /// only inside its drawn extent: a band run to the chart edges leaves
    /// the rest of the chart's double click to the chart.
    #[test]
    fn a_missed_double_click_reaches_only_the_drawn_rectangle() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 0;
            assert!(rect.left() - bands[0].rect.left() > 20.0, "{rect:?}");
            set_extent(&mut store, true, true);
            let painted_only =
                crate::pos2((bands[0].rect.left() + rect.left()) / 2.0, rect.center().y);
            let off_band = crate::pos2(painted_only.x, rect.top() - 20.0);
            for position in [painted_only, off_band] {
                assert_eq!(
                    missed_double_click(&mut store, projection, &bands[0], position),
                    ReleaseAnswer::Plain,
                    "the chart keeps the double click at {position:?}"
                );
                assert_eq!(extent(&store), (true, true), "at {position:?}");
                assert_eq!(store.selected(), None, "at {position:?}");
            }
            assert_eq!(
                missed_double_click(&mut store, projection, &bands[0], rect.center()),
                ReleaseAnswer::DoubleClickTaken
            );
            assert_eq!(extent(&store), (false, false));
        });
    }

    /// A double click the rectangle refuses changes nothing at all: no
    /// selection, no gesture, and the chart keeps the click.
    #[test]
    fn a_refused_double_click_selects_nothing() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 0;
            // Already at its original size: nothing to put back.
            let undo_before = store.undo_depth();
            assert_eq!(
                missed_double_click(&mut store, projection, &bands[0], rect.center()),
                ReleaseAnswer::Plain
            );
            store.commit_gesture();
            assert_eq!(store.selected(), None);
            assert_eq!(store.undo_depth(), undo_before);
            assert_eq!(extent(&store), (false, false));
        });
    }

    /// Hover answers with the click's own predicate: an outline rectangle's
    /// bands show the extend cursor where one click acts, and a side already
    /// run to the edge shows none.
    #[test]
    fn hover_hints_exactly_where_an_outline_rectangles_click_acts() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 0;
            // Inside a band, past the border's click reach.
            let left_band = crate::pos2(rect.left() + 15.0, rect.center().y);
            let right_band = crate::pos2(rect.right() - 15.0, rect.center().y);
            for position in [left_band, right_band] {
                assert_eq!(
                    hover(&mut store, projection, bands, position),
                    Some(GestureCursor::ExtendSide),
                    "at {position:?}"
                );
            }
            assert_eq!(hover(&mut store, projection, bands, rect.center()), None);
            assert_eq!(
                missed_click(&mut store, projection, &bands[0], left_band),
                ReleaseAnswer::ClickTaken
            );
            store.select(None);
            assert_eq!(extent(&store), (true, false));
            assert_eq!(hover(&mut store, projection, bands, left_band), None);
            assert_eq!(
                missed_click(&mut store, projection, &bands[0], left_band),
                ReleaseAnswer::Plain
            );
            assert_eq!(
                hover(&mut store, projection, bands, right_band),
                Some(GestureCursor::ExtendSide)
            );
        });
    }

    /// A side line under another drawing's body is not the lower drawing's
    /// to take: the press goes where the cursor points, the drawing on top.
    #[test]
    fn a_side_line_under_another_drawing_leaves_the_press_to_the_one_on_top() {
        with_projection(|projection, bands| {
            let mut store = Marks::default();
            let at = crate::ChartPoint::at;
            place_rectangle(&mut store, at(-10.0, 70.0), at(0.0, 30.0));
            place_rectangle(&mut store, at(-18.0, 60.0), at(-2.0, 40.0));
            store.items_mut()[1].style.fill_alpha = 40;
            let lower = projected_rect(&store, 0, projection, bands);
            let upper = projected_rect(&store, 1, projection, bands);
            let side = crate::pos2(lower.left(), upper.center().y);
            assert!(
                side.x - upper.left() > 30.0 && upper.right() - side.x > 30.0,
                "{side:?} sits well inside {upper:?}"
            );
            assert_eq!(
                hover(&mut store, projection, bands, side),
                Some(GestureCursor::Move)
            );
            let (lower_before, upper_before) = (
                store.items()[0].points.clone(),
                store.items()[1].points.clone(),
            );
            let mut gestures = PaneGestures::default();
            let mut pointer = pointer_at(side, bands);
            pointer.pressed = true;
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert_eq!(gestures.drag, DrawingDrag::Translate);
            assert_eq!(store.selected(), Some(1));
            pointer.pressed = false;
            pointer.position = Some(crate::pos2(side.x - 20.0, side.y));
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            pointer.down = false;
            pointer.released = true;
            owner_frame(&mut gestures, &mut store, projection, bands, &pointer);
            assert_eq!(store.items()[0].points, lower_before, "the lower one held");
            assert_ne!(store.items()[1].points, upper_before, "the top one moved");
        });
    }

    /// A strategy's region still reshapes by its side line and corners, so
    /// it shows the resize its press makes; only its band's click, which it
    /// refuses, goes unannounced.
    #[test]
    fn a_strategy_held_rectangle_shows_the_resize_its_press_makes() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 40;
            let bound = store.items()[0].id;
            let held: &dyn Fn(DrawingId) -> bool = &move |id| id == bound;
            let side = crate::pos2(rect.left(), rect.center().y);
            for (position, cursor) in [
                (side, GestureCursor::ResizeHorizontal),
                (rect.left_top(), GestureCursor::Resize),
                (crate::pos2(rect.left() + 10.0, side.y), GestureCursor::Move),
            ] {
                assert_eq!(
                    hover_by(&mut store, projection, (bands, position), held),
                    Some(cursor),
                    "at {position:?}"
                );
            }
            let mut gestures = PaneGestures::default();
            let mut pointer = pointer_at(side, bands);
            pointer.pressed = true;
            let hand = Hand {
                held,
                ..Hand::PLAIN
            };
            owner_frame_by(
                &mut gestures,
                &mut store,
                projection,
                (bands, &pointer),
                hand,
            );
            assert!(
                matches!(gestures.drag, DrawingDrag::Handle { .. }),
                "{:?}",
                gestures.drag
            );
        });
    }

    /// Alt+click walks down the z-order: it selects, and nothing else.
    #[test]
    fn alt_click_in_a_band_only_selects() {
        with_projection(|projection, bands| {
            let (mut store, rect) = local_rectangle(projection, bands);
            store.items_mut()[0].style.fill_alpha = 40;
            let band = crate::pos2(rect.right() - 10.0, rect.center().y);
            let alt = Hand {
                alt: true,
                ..Hand::PLAIN
            };
            press_and_release(
                &mut PaneGestures::default(),
                &mut store,
                projection,
                (bands, band),
                alt,
            );
            assert_eq!(store.selected(), Some(0));
            assert_eq!(extent(&store), (false, false), "alt never extends");
        });
    }

    /// A double click in a band is one undo step: its first click extended
    /// that side, the double click put the original size back, and Ctrl+Z
    /// returns to the state before the first click, never the one between.
    #[test]
    fn a_double_click_in_a_band_is_one_undo_step() {
        with_projection(|projection, bands| {
            let double = Hand {
                double_clicked: true,
                ..Hand::PLAIN
            };
            for left_before in [true, false] {
                let (mut store, rect) = local_rectangle(projection, bands);
                store.items_mut()[0].style.fill_alpha = 40;
                set_extent(&mut store, left_before, false);
                let band = crate::pos2(rect.right() - 10.0, rect.center().y);
                let depth = store.undo_depth();
                let mut gestures = PaneGestures::default();
                let at = (bands, band);
                press_and_release(&mut gestures, &mut store, projection, at, Hand::PLAIN);
                assert_eq!(extent(&store), (left_before, true), "the first click");
                press_and_release(&mut gestures, &mut store, projection, at, double);
                assert_eq!(extent(&store), (false, false), "the double click");
                if left_before {
                    assert_eq!(store.undo_depth(), depth + 1, "one step");
                    assert!(store.undo());
                    assert_eq!(extent(&store), (true, false), "before the first click");
                } else {
                    // Back where it started: a pair that changed nothing
                    // records nothing, and Ctrl+Z never shows the state between.
                    assert_eq!(store.undo_depth(), depth);
                }
            }
        });
    }

    /// egui's double click is time-only: a quick click in the left band and
    /// then the right one is two clicks, so both sides extend.
    #[test]
    fn a_quick_click_in_each_band_extends_both_sides() {
        with_projection(|projection, bands| {
            let double = Hand {
                double_clicked: true,
                ..Hand::PLAIN
            };
            for fill in [40, 0] {
                let (mut store, rect) = local_rectangle(projection, bands);
                store.items_mut()[0].style.fill_alpha = fill;
                let y = rect.center().y;
                let mut gestures = PaneGestures::default();
                let left_band = (bands, crate::pos2(rect.left() + 10.0, y));
                press_and_release(
                    &mut gestures,
                    &mut store,
                    projection,
                    left_band,
                    Hand::PLAIN,
                );
                let right_band = (bands, crate::pos2(rect.right() - 10.0, y));
                press_and_release(&mut gestures, &mut store, projection, right_band, double);
                assert_eq!(extent(&store), (true, true), "fill {fill}");
            }
        });
    }

    /// A quick click on another rectangle is that rectangle's own click,
    /// never a double click folded into the first one's undo step.
    #[test]
    fn a_quick_click_on_another_rectangle_keeps_its_own_undo_step() {
        with_projection(|projection, bands| {
            let (mut store, first) = local_rectangle(projection, bands);
            let at = crate::ChartPoint::at;
            place_rectangle(&mut store, at(-20.0, 95.0), at(0.0, 75.0));
            set_extent_of(&mut store, 1, (true, false));
            let second = projected_rect(&store, 1, projection, bands);
            assert!(!second.intersects(first), "{second:?} {first:?}");
            let depth = store.undo_depth();
            let mut gestures = PaneGestures::default();
            let first_band = crate::pos2(first.right() - 10.0, first.center().y);
            let second_band = crate::pos2(second.right() - 10.0, second.center().y);
            let double = Hand {
                double_clicked: true,
                ..Hand::PLAIN
            };
            press_and_release(
                &mut gestures,
                &mut store,
                projection,
                (bands, first_band),
                Hand::PLAIN,
            );
            press_and_release(
                &mut gestures,
                &mut store,
                projection,
                (bands, second_band),
                double,
            );
            assert_eq!(extent_of(&store, 0), (false, true), "the first click");
            assert_eq!(extent_of(&store, 1), (true, true), "the second click");
            assert_eq!(store.undo_depth(), depth + 2, "two steps");
            assert!(store.undo());
            assert_eq!(extent_of(&store, 1), (true, false));
            assert_eq!(extent_of(&store, 0), (false, true), "the first step held");
        });
    }

    /// Alt+click only selects, so with Alt held a band announces no
    /// extension: a filled body shows the move grip, an outline nothing.
    #[test]
    fn alt_shows_no_extend_cursor_in_a_band() {
        with_projection(|projection, bands| {
            let alt = Hand {
                alt: true,
                ..Hand::PLAIN
            };
            for (fill, cursor) in [(40, Some(GestureCursor::Move)), (0, None)] {
                let (mut store, rect) = local_rectangle(projection, bands);
                store.items_mut()[0].style.fill_alpha = fill;
                let band = crate::pos2(rect.right() - 15.0, rect.center().y);
                assert_eq!(
                    hover(&mut store, projection, bands, band),
                    Some(GestureCursor::ExtendSide)
                );
                let mut pointer = pointer_at(band, bands);
                pointer.down = false;
                let outcome = owner_frame_by(
                    &mut PaneGestures::default(),
                    &mut store,
                    projection,
                    (bands, &pointer),
                    alt,
                );
                assert_eq!(outcome.cursor, cursor, "fill {fill}");
            }
        });
    }
}
