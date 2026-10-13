//! A drawing gesture in flight — everything a press has resolved but a release
//! has not yet finished with.
//!
//! Pointer and shared-mark updates live on this owner in `pointer`.
//! Placement updates live here too, implemented in `placing`.
//! These fields once sat among the pane's
//! geometry and its menus. They belong together because they share one
//! lifetime — a gesture — and because a gesture is the one thing on a pane
//! that spans frames without being a measurement: a press resolves what it
//! landed on, and the frames until the release must not ask again.
//!
//! Three things are deliberately *not* here. The drawing menu's test-only
//! widget trace is one — it shares the menu's lifetime, not a gesture's, and
//! lives on the window's context menu with the rest of the menu's state. The
//! pane's drawings are the objects themselves, which outlive every gesture;
//! and the pointer's plain hover position is chart chrome, read by the
//! crosshair whether or not a tool is armed.
//!
//! Everything here is plain data in screen pixels ([`Px`]): the window reads
//! its pointer into a [`PointerFrame`] or a [`PlacementFrame`] and carries out
//! the outcome it is handed.

mod band;
mod placing;
mod pointer;
mod projection;
mod shared;

#[cfg(test)]
mod marks_tests;

pub use band::{
    Band, Bands, MAX_BANDS, band_at, band_of, drawing_in_band, magnet_value_of, pane_key,
};
pub use placing::{
    PlacementCompletion, PlacementDefaults, PlacementFrame, PlacementOptions, PlacementOutcome,
    placement_surface,
};
pub use pointer::{PointerFrame, PointerOutcome};
pub use projection::{DrawingProjection, PaneSeriesRead, magnet_price_of, snap_bar_to_tape};
pub use shared::{
    PaneIndex, SharedDrag, SharedEdit, SharedInteraction, SharedMarksMut, SharedPick,
    SharedPointer, SharedSource,
};

#[cfg(any(feature = "drawing-harness", test))]
use crate::Constrain;
use crate::{ChartPoint, DrawingTool, Px, PxRect};

/// The marks of one pane, as the gestures edit them.
type Marks = crate::Drawings<DrawingTool>;
/// One mark, as the gestures read it.
type Mark = crate::Drawing<DrawingTool>;

/// Hit radius for selecting a drawing anchor, in logical pixels.
pub const DRAWING_SELECT_RADIUS_PX: f32 = 10.0;

/// Hit radius for a selected drawing's editable anchor.
pub const DRAWING_ANCHOR_RADIUS_PX: f32 = 12.0;

/// How near the pointer must be to one of an indicator band's own values
/// for the magnet to take the anchor there.
pub const MAGNET_REACH_PX: f32 = 12.0;

/// How far past a candle's high or low (inside it always counts) the magnet
/// still takes the anchor onto its nearest open / high / low / close: enough
/// to catch a swing from a hand's wobble away, little enough to still draw a
/// free diagonal through empty price.
pub const CANDLE_MAGNET_REACH_PX: f32 = 24.0;

/// The candle magnet has no reach: [`crate::AnchorSnap::NearestOhlc`]
/// never lets go, however far the pointer floats from the candle.
pub const MAGNET_REACH_UNLIMITED_PX: f32 = f32::INFINITY;

/// Minimum pointer travel that turns one press/release into drag placement.
pub const DRAWING_DRAG_THRESHOLD_PX: f32 = 4.0;

/// How far a press must travel before its release counts as "the trader
/// dragged this object out" instead of "the trader clicked".
///
/// Deliberately larger than [`DRAWING_DRAG_THRESHOLD_PX`], which answers a
/// different question: whether a gesture already under way is a drag at all.
/// This one decides whether a *release* finishes an object, and it was
/// sharing that four-pixel answer — which is inside the wander of an ordinary
/// click.
///
/// What that cost, reported from the running build: a click meant to start a
/// fixed-range profile placed **both** its anchors, so the object was born
/// less than one bar wide ("1 of 1 bars"), and completing it disarmed the
/// tool. Moving the pointer afterwards then did nothing at all, which reads
/// exactly like a frozen chart — the trader is waiting for a range to follow
/// their hand and there is no longer a draft to follow it.
///
/// A release under this distance leaves the draft alive instead, so the
/// gesture becomes the click-move-click the hand was already doing.
pub const DRAWING_DRAG_COMPLETES_PX: f32 = 12.0;

/// How far the pointer must travel before a freehand stroke records another
/// point. Chosen so a hand-drawn circle keeps its shape while a half-second
/// scribble stores tens of anchors instead of hundreds — every anchor is
/// paint and hit-test work on every frame for the rest of the session.
pub const FREEHAND_MIN_STEP_PX: f32 = 4.0;

/// Hard ceiling on one stroke, so a pointer that never stops moving cannot
/// turn a single drawing into an unbounded cost.
pub const FREEHAND_MAX_POINTS: usize = 512;

/// A pointer and a modifier for a run with nobody at the keyboard — see
/// [`PaneGestures::parked_hand`]. Never constructed outside the harness hook
/// and the tests.
#[cfg(any(feature = "drawing-harness", test))]
#[derive(Debug, Clone, Copy)]
pub struct ParkedHand {
    pub position: Px,
    pub constrain: Constrain,
}

/// What the pointer is currently doing to a drawing, if anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawingDrag {
    #[default]
    None,
    Translate,
    /// A grab on one of the object's handles. `handle` indexes the tool's own
    /// handle list, which is the anchors for almost every tool but not for
    /// all of them — a channel's rail handles move anchors they do not sit
    /// on, so only the tool may turn this index into new geometry.
    Handle {
        drawing_index: usize,
        handle: usize,
    },
    /// The press landed on a locked drawing: the gesture belongs to the
    /// object (the chart must not pan) but the geometry stays put.
    Blocked,
}

impl DrawingDrag {
    #[must_use]
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// The cursor a gesture asks the window to show, by meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureCursor {
    /// The body of an object a press would move.
    Move,
    /// A handle a press would reshape.
    Resize,
    /// Geometry that refuses: locked, or a band that cannot take an anchor.
    NotAllowed,
    /// A band an armed tool would place an anchor in.
    Crosshair,
}

/// A body drag in flight: the drawing it grabbed, where the pointer was last
/// frame, and the anchors as the hand alone has moved them. Each frame adds
/// only that frame's travel, read under that frame's axes, so a rescale or a
/// pan under a still hand moves nothing; the magnet snaps a copy, so its
/// correction never accumulates into `free`.
pub struct TranslateFrom {
    pub index: usize,
    pub last: Px,
    pub free: Vec<ChartPoint>,
}

impl PaneGestures {
    /// [`crate::Drawings::shift_bars`] for the one copy of anchors
    /// the store does not hold: history landing under a body drag must not
    /// make the object jump.
    pub fn shift_bars(&mut self, delta: isize) {
        for point in self
            .translate_from
            .iter_mut()
            .flat_map(|from| &mut from.free)
        {
            point.bar += delta as f32;
        }
    }

    /// End any drawing drag in flight, its grab with it.
    pub fn cancel_drag(&mut self) {
        self.drag = DrawingDrag::None;
        self.translate_from = None;
    }
}

/// One pane's marks beside the series they anchor to: the store, and the
/// gesture holding the one copy of anchors the store does not. A change of
/// bar space has to reach both, or an object jumps under the hand.
pub struct PaneMarksMut<'a> {
    pub drawings: &'a mut Marks,
    pub gestures: &'a mut PaneGestures,
    pub series: PaneSeriesRead<'a>,
}

impl PaneMarksMut<'_> {
    /// Re-express the marks against the series held now, `old_slots` being
    /// the length of the series they were anchored to.
    ///
    /// The one call behind every re-cut — a timeframe switch, a bar-kind
    /// switch, a rewind, a symbol change. Marks are never dropped by a state
    /// change: the trader placed them and the trader removes them.
    pub fn reanchor(self, old_slots: usize) {
        let slots = (old_slots, self.series.slots());
        let slot_of = |time| self.series.slot_of_time(time);
        self.drawings.reanchor(slots.0, slots.1, slot_of);
        // A body drag in flight holds its own copy of the anchors: same map.
        if let Some(from) = self.gestures.translate_from.as_mut() {
            crate::reanchor_points(&mut from.free, slots, &slot_of);
        }
    }

    /// Move the marks with `delta` bars that arrived in front of them, then
    /// put any mark the longer series now reaches back on its own instant.
    pub fn shift_bars(self, delta: isize) {
        self.drawings.shift_bars(delta);
        self.gestures.shift_bars(delta);
        self.drawings
            .resettle_off_series(|time| self.series.slot_of_time(time));
    }
}

/// Drawing placement and movement state. Anchors are chart coordinates; only
/// the current hover and press position are transient pixels.
///
/// See the module docs for what a gesture owns and what it does not.
#[derive(Default)]
pub struct PaneGestures {
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
    pub band_hint: Option<PxRect>,
    /// Where the press that may become this gesture landed, in pixels.
    pub press_position: Option<Px>,
    /// Whether that press landed on empty canvas.
    pub press_started_empty: bool,
    /// The hand a run has when nobody is at the mouse — the
    /// `QUANTICK_DRAWING_DRAFT` harness hook. `None` for every real session:
    /// only the harness build and the tests set it.
    ///
    /// The live preview of a half-placed object is the whole feedback of a
    /// multi-anchor gesture, and it is the one surface a click-free launch
    /// could not reach: it exists only between two clicks, and only while a
    /// pointer is over the chart. Both halves are read exactly where the real
    /// pointer and the real modifier are read and nowhere else, so everything
    /// downstream — the tool's shaping, the hint chip, the rubber band — runs
    /// the same code a hand runs.
    #[cfg(any(feature = "drawing-harness", test))]
    pub parked_hand: Option<ParkedHand>,
    /// The last screen position a freehand stroke actually recorded, so the
    /// capture decimates as it goes rather than storing every mouse event.
    pub freehand_last_position: Option<Px>,
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
    pub drag_pending_from: Option<Px>,
    /// The move or resize this gesture is, once it is one.
    pub drag: DrawingDrag,
    /// What a body drag grabbed; see [`TranslateFrom`].
    pub translate_from: Option<TranslateFrom>,
    /// A gesture this pane is running on a mark the other pane holds, and the
    /// two pieces of pointer state it needs: where the press landed while the
    /// drag threshold is still unmet, and the market instant and price the
    /// pointer was last over — what a body drag sends its deltas against.
    pub shared_drag: SharedDrag,
    /// The pane whose mark [`Self::shared_drag`] is moving, for as long as it
    /// is moving it.
    pub shared_drag_owner: Option<PaneIndex>,
    /// See [`Self::shared_drag`].
    pub shared_drag_pending_from: Option<Px>,
    /// See [`Self::shared_drag`].
    pub shared_pointer_mark: Option<(i64, f64)>,
    /// The held mark's own magnet, joining the rail's for this gesture.
    pub shared_drag_magnet: bool,
}
