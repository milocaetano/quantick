//! Marks shared between the panes of one tab: the contract the tab carries
//! between them, and the pane's side of it.
//!
//! The two panes cut the same trades into different bars, so a bar index means
//! nothing across them; the market timestamp each anchor captured at placement
//! is the only coordinate they share (`docs/ux/drawing-tools-2026-08.md` §D7).
//! Everything here speaks in market time and price: `reproject` reads a foreign
//! mark into this pane's slots, `paint_shared_from` paints it, `shared_pick`
//! and `interact_shared` let the trader take hold of it here, and
//! `apply_shared_edit` lands the edit back on the pane that owns the object.
//! A pure move out of `pane.rs`.

use eframe::egui;
use smallvec::SmallVec;

use crate::bands;
use crate::drawings::{self, ChartPoint, DrawContext, Drawing, DrawingBand, DrawingStyle};

use super::{
    ChartPane, DRAWING_DRAG_THRESHOLD_PX, DRAWING_SELECT_RADIUS_PX, PaneChrome, anchor_hit,
};

/// What a pane resolved on *another* pane's shared marks this frame.
///
/// Said in market time and price, because those are the only coordinates two
/// panes of a tab agree on — a bar index means nothing across two cuts of the
/// same tape (`docs/ux/drawing-tools-2026-08.md` §D7). The pane that holds the
/// object turns them back into its own bar space, so the trader edits the one
/// object rather than a copy of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SharedEdit {
    /// Take the selection, without moving anything.
    Select(usize),
    /// Put one anchor at this instant and price.
    MoveAnchor {
        index: usize,
        anchor: usize,
        time_ms: i64,
        price: f64,
    },
    /// Shift every anchor of the object by this much time and price.
    Translate {
        index: usize,
        delta_ms: i64,
        delta_price: f64,
    },
}

/// A shared mark under the pointer, as the tab resolved it for one pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SharedPick {
    /// Which pane's store holds it, as a [`PaneIndex`].
    ///
    /// Explicit rather than "the other pane": with a context stack beside the
    /// flow pane there is more than one other, and a mark shared across three
    /// panes has two panes mirroring it. An edit that guessed its owner would
    /// land on whichever chart the guess named.
    pub owner: PaneIndex,
    /// Its index in the owning pane's store.
    pub index: usize,
    /// Which handle was grabbed, or `None` for the body.
    pub anchor: Option<usize>,
    /// Locked geometry refuses to move, exactly as it does on its own pane.
    pub locked: bool,
}

/// What a pane did to another pane's marks in one frame.
///
/// The gesture flags bracket the edits so a whole drag lands on the owning
/// store as one undo entry — the same coalescing a drag on the object's own
/// chart gets, because it is the same gesture on the same object.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SharedInteraction {
    /// The pane whose store this lands on.
    ///
    /// Carried rather than recomputed: a drag outlives the pick that started
    /// it — `commit_gesture` fires on release, by which time the pointer may
    /// be nowhere near the mark — so the owner is remembered with the gesture
    /// or it is lost exactly when it is needed.
    pub owner: Option<PaneIndex>,
    pub edit: Option<SharedEdit>,
    pub begin_gesture: bool,
    pub commit_gesture: bool,
}

/// What the pointer is doing this frame, as the shared-mark handler needs it.
pub(super) struct SharedPointer {
    /// Where it is, wherever that is. The band below decides what counts.
    pub(super) position: Option<egui::Pos2>,
    /// The band drawings live in on this pane, this frame.
    ///
    /// A press must land inside it. A drag already running is *clamped* into
    /// it instead, exactly as a drag on this pane's own marks is: the canvas
    /// narrows by the inspector's width on the very frame a press opens it
    /// (§D8), and a gesture that stopped whenever the pointer left the
    /// shrunken pane would die on the frame it was born.
    pub(super) area: egui::Rect,
    /// Whether floating chrome — the inspector, the manager, a flyout — is
    /// under it right now.
    ///
    /// Read at press time only, like every other pointer path here:
    /// continuity, not priority. A drag that started on the canvas keeps
    /// running while the pointer crosses a panel, which matters most here of
    /// all — the press that selects a mark is what *opens* the inspector, and
    /// the inspector opens over the chart the mark is on.
    pub(super) over_chrome: bool,
    pub(super) pressed: bool,
    pub(super) down: bool,
    pub(super) released: bool,
    pub(super) history_right: f32,
    pub(super) total: usize,
    pub(super) magnet: bool,
}

/// Where a pane sits in its tab: `0` is the flow pane, `1..` the context
/// stack, top to bottom.
///
/// An address, never a position on screen — the context stack is drawn left of
/// the flow pane, and a reader who took this for a left-to-right order would
/// mirror every edit.
pub type PaneIndex = usize;

/// A gesture a pane is running on another pane's mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum SharedDrag {
    #[default]
    None,
    /// Dragging the whole object.
    Body { index: usize },
    /// Dragging one handle.
    Anchor { index: usize, anchor: usize },
    /// The mark is locked: the gesture is ours (the chart must not pan under
    /// it) but the geometry stays exactly where the trader left it.
    Blocked,
}

impl SharedDrag {
    pub(super) const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl ChartPane {
    /// Work a shared mark that lives on the other pane, from this one.
    ///
    /// Every answer leaves in market time and price — the coordinates two cuts
    /// of one tape agree on — so the tab can hand them to the pane that holds
    /// the object without either pane learning the other's bar space.
    ///
    /// The pointer rules are the ones this pane already applies to its own
    /// marks: a handle before a body, a locked object that takes the gesture
    /// and refuses to move, and a drag threshold so a click never re-angles a
    /// level by two pixels of hand tremor.
    pub(super) fn interact_shared(
        &mut self,
        ui: &egui::Ui,
        chrome: &mut PaneChrome<'_>,
        pointer: SharedPointer,
    ) {
        // The band under the pointer, on this pane's own last carve: a shared
        // mark is grabbed where it is painted, and where it is painted is the
        // band whose axis its value belongs to. Reading the candles' scale for
        // a CVD mark would send a price back to the pane that owns it.
        let mark = |pane: &Self, position: egui::Pos2| {
            let band = bands::band_at(&pane.frame.bands, position)?;
            pane.drawing_point_at(
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
                .zip(chrome.shared_pick)
        {
            // Selecting is not moving (§D9): the press takes the object
            // whether or not the drag that may follow is allowed.
            chrome.shared.owner = Some(pick.owner);
            chrome.shared.edit = Some(SharedEdit::Select(pick.index));
            self.gestures.shared_drag_owner = Some(pick.owner);
            self.gestures.shared_drag_pending_from = Some(position);
            self.gestures.shared_pointer_mark = mark(self, position);
            self.gestures.shared_drag = if pick.locked {
                SharedDrag::Blocked
            } else {
                chrome.shared.begin_gesture = true;
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

        if !self.gestures.shared_drag.is_active() {
            // Hover feedback, so a mirrored mark does not feel deader than the
            // object it is: the same three cursors its own pane shows.
            if !pointer.over_chrome
                && pointer
                    .position
                    .is_some_and(|position| pointer.area.contains(position))
                && let Some(pick) = chrome.shared_pick
            {
                ui.ctx().set_cursor_icon(match (pick.locked, pick.anchor) {
                    (true, _) => egui::CursorIcon::NotAllowed,
                    (false, Some(_)) => egui::CursorIcon::ResizeNwSe,
                    (false, None) => egui::CursorIcon::Move,
                });
            }
            return;
        }

        if pointer.released {
            chrome.shared.owner = self.gestures.shared_drag_owner;
            chrome.shared.commit_gesture = true;
            self.gestures.shared_drag = SharedDrag::None;
            self.gestures.shared_drag_owner = None;
            self.gestures.shared_drag_pending_from = None;
            self.gestures.shared_pointer_mark = None;
            return;
        }
        if !pointer.down {
            return;
        }
        // Under the threshold the object does not move at all, so a click on
        // the mirror stays a click.
        if let Some(origin) = self.gestures.shared_drag_pending_from {
            let travelled = pointer
                .position
                .is_some_and(|position| (position - origin).length() >= DRAWING_DRAG_THRESHOLD_PX);
            if !travelled {
                return;
            }
            self.gestures.shared_drag_pending_from = None;
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
        let Some((time_ms, price)) = mark(self, position) else {
            return;
        };
        // Every edit this gesture emits belongs to the pane the gesture took
        // hold of, whatever the pointer is over now.
        chrome.shared.owner = self.gestures.shared_drag_owner;
        match self.gestures.shared_drag {
            SharedDrag::Anchor { index, anchor } => {
                chrome.shared.edit = Some(SharedEdit::MoveAnchor {
                    index,
                    anchor,
                    time_ms,
                    price,
                });
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            SharedDrag::Body { index } => {
                if let Some((last_time, last_price)) = self.gestures.shared_pointer_mark {
                    chrome.shared.edit = Some(SharedEdit::Translate {
                        index,
                        delta_ms: time_ms - last_time,
                        delta_price: price - last_price,
                    });
                }
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
            SharedDrag::Blocked => ui.ctx().set_cursor_icon(egui::CursorIcon::NotAllowed),
            SharedDrag::None => {}
        }
        self.gestures.shared_pointer_mark = Some((time_ms, price));
    }

    /// What a pointer at `pos` grabs among `source`'s shared marks: a handle
    /// anywhere first, then the topmost body — the same order, and the same
    /// primitives, this pane uses on its own objects.
    ///
    /// Runs on the projection cached by the last frame, which is the geometry
    /// the trader was looking at when they pressed (§D8: no gesture re-measures
    /// a world it moved).
    ///
    /// Per-frame cost: nothing until a mark is actually shared, and bounded by
    /// the handful of objects on the chart when one is.
    pub fn shared_pick(&self, source: &Self, pos: egui::Pos2) -> Option<(usize, Option<usize>)> {
        if !source.drawings.items().iter().any(Drawing::shared) {
            return None;
        }
        let (_, history_right, total, _) = self.last_projection()?;
        // Only the band the pointer is in: a mirrored CVD level and a
        // mirrored price level can be one pixel apart on screen and mean
        // unrelated things, exactly as they can on the pane that owns them.
        let band = bands::band_at(&self.frame.bands, pos)?;
        let scale = band.scale?;
        let mut body = None;
        for (index, drawing) in source.drawings.items().iter().enumerate().rev() {
            if !drawing.shared()
                || !source.drawings.is_visible(index)
                || !bands::drawing_in_band(drawing, band)
            {
                continue;
            }
            let Some((anchors, _)) = self.reproject(drawing) else {
                continue;
            };
            let points: SmallVec<[egui::Pos2; 4]> = anchors
                .iter()
                .map(|anchor| self.drawing_screen_point(*anchor, history_right, total, &scale))
                .collect();
            let ctxt = DrawContext {
                payload: drawing.payload.as_ref(),
                anchors: &anchors,
                scale: &scale,
                px_per_bar: self.viewport.px_per_bar(),
                unit: band.unit(),
                primary_band: true,
                style: drawing.style,
                selected: source.drawings.selected() == Some(index),
                halo: false,
                content_editing: false,
            };
            // Handle drags cross the pane boundary as "move anchor N", so a
            // tool whose handles are not its anchors — a channel's rail
            // handles move anchors they do not sit on — offers none on the
            // mirror. The mark still selects and still moves as a whole
            // there; reshaping it happens on the chart it was drawn on. An
            // invisible grab point on the mirror would be worse than an
            // absent one: the ring the trader sees is the ring they get.
            if drawing.tool.handles_are_anchors(band.rect, &points, &ctxt)
                && let Some(anchor) = anchor_hit(&points, pos)
            {
                return Some((index, Some(anchor)));
            }
            if body.is_none()
                && drawing
                    .tool
                    .hit_test(band.rect, &points, pos, DRAWING_SELECT_RADIUS_PX, &ctxt)
            {
                body = Some((index, None));
            }
        }
        body
    }

    /// Apply an edit another pane of this tab made to one of this pane's
    /// shared marks, back in this pane's own bar space.
    ///
    /// The instants arrive as they were read off the other chart and are
    /// resolved here, which is what makes the two views one object rather than
    /// two copies of one.
    pub fn apply_shared_edit(&mut self, edit: SharedEdit) {
        match edit {
            SharedEdit::Select(index) => self.drawings.select(Some(index)),
            SharedEdit::MoveAnchor {
                index,
                anchor,
                time_ms,
                price,
            } => {
                let Some(bar) = self.slot_of_time(time_ms) else {
                    return;
                };
                self.drawings.move_anchor(
                    index,
                    anchor,
                    ChartPoint::at_time(bar, price, Some(time_ms)),
                );
            }
            SharedEdit::Translate {
                index,
                delta_ms,
                delta_price,
            } => self.translate_shared(index, delta_ms, delta_price),
        }
    }

    /// Move a whole shared object by an amount of market time and price.
    ///
    /// Time, not bars: the two panes cut the tape differently, so the same
    /// drag is a different number of bars on each — and market time is what
    /// both of them mean by it.
    ///
    /// Resolved in full before anything is written. A drag that would put part
    /// of the object where this pane's series cannot reach moves nothing at
    /// all, rather than leaving a shape with one end on a bar and the other on
    /// an instant that has none.
    fn translate_shared(&mut self, index: usize, delta_ms: i64, delta_price: f64) {
        let Some(drawing) = self.drawings.items().get(index) else {
            return;
        };
        if drawing.locked {
            return;
        }
        let mut moved: SmallVec<[ChartPoint; 4]> = SmallVec::new();
        for point in &drawing.points {
            let Some(time) = point.time_ms.and_then(|time| time.checked_add(delta_ms)) else {
                return;
            };
            let Some(bar) = self.slot_of_time(time) else {
                return;
            };
            moved.push(ChartPoint::at_time(
                bar,
                point.price + delta_price,
                Some(time),
            ));
        }
        for (anchor, point) in moved.into_iter().enumerate() {
            self.drawings.move_anchor(index, anchor, point);
        }
    }

    /// Paint the shared drawings that live on `source`, re-expressed on this
    /// pane (`docs/ux/drawing-tools-2026-08.md` §D7).
    ///
    /// The two panes cut the same trades into different bars, so a bar index
    /// means nothing across them — the market timestamp each anchor captured
    /// at placement is the only coordinate they share. Every anchor goes back
    /// through this pane's own `slot_at_time`.
    ///
    /// Read-only here, on purpose: selection, dragging and the inspector
    /// belong to the pane the object was drawn on. A mark that could be
    /// grabbed in two places would be two versions of one object, which is
    /// exactly the confusion sharing exists to remove.
    ///
    /// Per-frame cost: nothing at all until a drawing is actually shared —
    /// the loop below runs over `source.drawings` and does nothing for the
    /// `ThisChart` default every object opens with.
    pub fn paint_shared_from(&self, painter: &egui::Painter, source: &Self) {
        if !source.drawings.items().iter().any(Drawing::shared) {
            return;
        }
        let Some((_, history_right, total, _)) = self.last_projection() else {
            return;
        };
        for (index, drawing) in source.drawings.items().iter().enumerate() {
            if !drawing.shared() || !source.drawings.is_visible(index) {
                continue;
            }
            let Some((anchors, clamped)) = self.reproject(drawing) else {
                continue;
            };
            // A clamped anchor is an honest half-truth: the object really is
            // off the end of this pane's series, and it says so by fading
            // rather than by pretending to sit on the edge bar.
            let style = if clamped {
                DrawingStyle {
                    color: drawing.style.color.gamma_multiply(Self::CLAMPED_OPACITY),
                    fill_alpha: 0,
                    ..drawing.style
                }
            } else {
                drawing.style
            };
            // A value is portable only inside the same value space. The x half
            // crosses through market time, but a CVD level means nothing on a
            // price axis — so a band drawing appears on this pane only where
            // the same indicator does, and nowhere else. Refused by
            // construction, not by a warning the trader could ignore.
            //
            // A time-only object is the case where sharing is most obviously
            // right: one instant, marked through every band of both charts.
            for (band_index, band) in self.frame.bands.iter().enumerate() {
                if !bands::drawing_in_band(drawing, band) {
                    continue;
                }
                let Some(scale) = band.scale else {
                    continue;
                };
                let clipped = painter.with_clip_rect(band.rect);
                // Stack-allocated: this is a per-frame path, and every shipped
                // tool has at most three anchors, so the heap is never touched.
                let points: SmallVec<[egui::Pos2; 4]> = anchors
                    .iter()
                    .map(|anchor| self.drawing_screen_point(*anchor, history_right, total, &scale))
                    .collect();
                // A mark selected here shows it here. The trader can take and
                // move it from this pane (`Self::interact_shared`), and a
                // selection that painted only on the other chart would leave
                // the gesture with no visible subject.
                let selected = source.drawings.selected() == Some(index);
                let ctxt = DrawContext {
                    payload: drawing.payload.as_ref(),
                    anchors: &anchors,
                    scale: &scale,
                    px_per_bar: self.viewport.px_per_bar(),
                    unit: band.unit(),
                    primary_band: drawing.band != DrawingBand::AllBands || band_index == 0,
                    style,
                    selected,
                    halo: false,
                    // The object lives on `source`, so that is the pane that knows
                    // whether its words are in an editor right now. Left
                    // hardcoded, a shared note being typed kept painting its
                    // old words on the companion chart — the same double
                    // render the editor stands the original down to avoid.
                    content_editing: source.gestures.content_editing == Some(index),
                };
                // Both halves, so a shared object is the same object on both
                // charts. A tool whose body lives in the background pass —
                // the volume profile's histogram — would otherwise cross to
                // the companion pane as two edge lines and a level, with the
                // volume shape the trader shared missing entirely.
                //
                // The mirrored copy paints over the candles rather than under
                // them: this pass runs after the host pane's own candles are
                // down, and reaching under them would mean carving a third
                // time on the far pane's geometry. A shared mark is a
                // reference to something living on another chart, and reading
                // as one is the honest outcome.
                drawing
                    .tool
                    .paint_under(&clipped, band.rect, style, &points, &ctxt);
                // Locked geometry shows no handles on either chart: they would
                // advertise a drag that is refused.
                drawing.tool.paint(
                    &clipped,
                    band.rect,
                    style,
                    &points,
                    &ctxt,
                    selected && !drawing.locked,
                );
            }
        }
    }

    /// Re-express a foreign drawing's anchors in this pane's bar space.
    /// Returns the anchors and whether any of them had to be clamped to the
    /// end of this pane's series.
    fn reproject(&self, drawing: &Drawing) -> Option<(SmallVec<[ChartPoint; 4]>, bool)> {
        let slots = self.slots();
        if slots == 0 {
            return None;
        }
        let mut anchors = SmallVec::new();
        let mut clamped = false;
        for point in &drawing.points {
            let time = point.time_ms?;
            let slot = match self.slot_at_time(time) {
                Some(slot) => slot.min(slots - 1),
                // Before this pane's first bar: the series does not reach
                // back that far, so the anchor sits on the oldest bar and is
                // marked as clamped.
                None => {
                    clamped = true;
                    0
                }
            };
            // A time chart can also place an instant *past* its newest bar,
            // on the same fixed interval its bars already run on — which is
            // where a trend line pointing into the future belongs. Without
            // this the future end of a shared line would pile up on the right
            // edge instead of running on.
            if let Some(future) = self.future_slot_at_time(time) {
                anchors.push(ChartPoint::at_time(future + 0.5, point.price, Some(time)));
                continue;
            }
            // The other clamp: a time past the end of this pane's series
            // lands on the newest slot because `slot_at_time` cannot go
            // further than the tape has. Only a *closed* bar can prove that,
            // by having ended before the anchor's instant — a time inside the
            // forming bar is simply now, and fading it would be a lie in the
            // other direction.
            clamped |= self
                .closed_bar(slot)
                .is_some_and(|bar| bar.close_time < time);
            anchors.push(ChartPoint::at_time(
                slot as f32 + 0.5,
                point.price,
                Some(time),
            ));
        }
        Some((anchors, clamped))
    }
}
