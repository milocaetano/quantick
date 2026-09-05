//! A drawing gesture in flight — everything a press has resolved but a release
//! has not yet finished with.
//!
//! The methods that read and write this already live next door in
//! [`super::drawing_gestures`]; only the state stayed on
//! [`super::ChartPane`], where fifteen fields of it sat among the pane's
//! geometry and its menus. They belong together because they share one
//! lifetime — a gesture — and because a gesture is the one thing on a pane
//! that spans frames without being a measurement: a press resolves what it
//! landed on, and the frames until the release must not ask again.
//!
//! Two things are deliberately *not* here. [`super::ChartPane::drawings`] is
//! the objects themselves, which outlive every gesture; and the pointer's
//! plain hover position is chart chrome, read by the crosshair whether or not
//! a tool is armed.

use eframe::egui;

use super::{DrawingDrag, PaneIndex, ParkedHand, SharedDrag};
use crate::drawings::ChartPoint;

/// Drawing placement and movement state. Anchors are chart coordinates; only
/// the current hover and press position are transient pixels.
///
/// See the module docs for what a gesture owns and what it does not.
#[derive(Default)]
pub struct GestureState {
    /// Where the next anchor would land, as the input pass resolved it.
    pub hover: Option<ChartPoint>,
    /// The object whose *content* is being edited off-canvas right now —
    /// the on-chart note editor's subject. Told to the pane by the host each
    /// frame, because the editor is chrome and lives above the canvas; the
    /// object it holds the words for must not paint them twice.
    pub content_editing: Option<usize>,
    /// The band the next anchor would land in, as the input pass resolved it.
    /// The draw pass puts the accent hairline on its top edge — one band at a
    /// time, and none at all when no tool is armed.
    pub(super) band_hint: Option<egui::Rect>,
    /// Where the press that may become this gesture landed, in pixels.
    pub press_position: Option<egui::Pos2>,
    /// Whether that press landed on empty canvas.
    pub press_started_empty: bool,
    /// The hand a run has when nobody is at the mouse — the
    /// `QUANTICK_DRAWING_DRAFT` harness hook. `None` for every real session.
    ///
    /// The live preview of a half-placed object is the whole feedback of a
    /// multi-anchor gesture, and it is the one surface a click-free launch
    /// could not reach: it exists only between two clicks, and only while a
    /// pointer is over the chart. Both halves are read exactly where the real
    /// pointer and the real modifier are read and nowhere else, so everything
    /// downstream — the tool's shaping, the hint chip, the rubber band — runs
    /// the same code a hand runs.
    pub parked_hand: Option<ParkedHand>,
    /// The last screen position a freehand stroke actually recorded, so the
    /// capture decimates as it goes rather than storing every mouse event.
    pub(super) freehand_last_position: Option<egui::Pos2>,
    /// What the press resolved under the Pointer tool, held until the click
    /// it belongs to completes. `Some(None)` is a real answer — a press on
    /// empty canvas, the one that deselects.
    ///
    /// It exists because the canvas is not the same shape before and after a
    /// selection: the pinned inspector is a side panel laid out *before* the
    /// central panel, so the frame a selection appears is the frame the chart
    /// narrows by the panel's width and every drawing slides left with it.
    /// Re-hit-testing on the release would be asking a different chart.
    pub press_pick: Option<Option<usize>>,
    /// Where a move/resize gesture pressed, while it is still under the drag
    /// threshold. `None` once the threshold is passed — from then on the
    /// object follows the pointer for the rest of the gesture.
    ///
    /// Without it, one pixel of hand tremor during a *click* re-angles a
    /// channel or shifts a level the trader placed deliberately, and records
    /// it as an undo step. Placement already refused to turn a twitch into a
    /// drag (`DRAWING_DRAG_THRESHOLD_PX`); moving now refuses too.
    pub drag_pending_from: Option<egui::Pos2>,
    /// The move or resize this gesture is, once it is one.
    pub drag: DrawingDrag,
    /// A gesture this pane is running on a mark the other pane holds, and the
    /// two pieces of pointer state it needs: where the press landed while the
    /// drag threshold is still unmet, and the market instant and price the
    /// pointer was last over — what a body drag sends its deltas against.
    pub(super) shared_drag: SharedDrag,
    /// The pane whose mark [`Self::shared_drag`] is moving, for as long as it
    /// is moving it.
    pub(super) shared_drag_owner: Option<PaneIndex>,
    /// See [`Self::shared_drag`].
    pub(super) shared_drag_pending_from: Option<egui::Pos2>,
    /// See [`Self::shared_drag`].
    pub(super) shared_pointer_mark: Option<(i64, f64)>,
    /// Test-only trace of the drawing section's widgets, the
    /// `layer_menu_rects` idiom: label → rect, rebuilt per menu frame.
    #[cfg(test)]
    pub menu_rects: Vec<(&'static str, egui::Rect)>,
}
