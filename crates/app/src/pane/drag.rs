//! The pointer's grip on a drawing: what a drag is holding, how far it
//! has to travel to count, and how far a magnet reaches.

// Only the parked-hand value names the UI library, and it exists only for
// the harness and the tests.
#[cfg(any(feature = "drawing-harness", test))]
use eframe::egui;

use crate::drawings;

/// Hit radius for selecting a drawing anchor, in logical pixels.
pub const DRAWING_SELECT_RADIUS_PX: f32 = 10.0;

/// Hit radius for a selected drawing's editable anchor.
pub const DRAWING_ANCHOR_RADIUS_PX: f32 = 12.0;

/// Minimum pointer travel that turns one press/release into drag placement.
/// How near the pointer must be to a bar's open / high / low / close for
/// the magnet to take the anchor. Generous enough to catch the swing you
/// aimed at, tight enough to still draw a free diagonal between bars.
pub const MAGNET_REACH_PX: f32 = 12.0;

/// The candle magnet has no reach: [`drawings::AnchorSnap::NearestOhlc`]
/// never lets go, however far the pointer floats from the candle.
pub const MAGNET_REACH_UNLIMITED_PX: f32 = f32::INFINITY;

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

/// A pointer and a modifier for a run with nobody at the keyboard — see
/// [`PaneGestures::parked_hand`]. Never constructed outside the harness hook.
#[cfg(any(feature = "drawing-harness", test))]
#[derive(Debug, Clone, Copy)]
pub struct ParkedHand {
    pub position: egui::Pos2,
    pub constrain: drawings::Constrain,
}

/// How far the pointer must travel before a freehand stroke records another
/// point. Chosen so a hand-drawn circle keeps its shape while a half-second
/// scribble stores tens of anchors instead of hundreds — every anchor is
/// paint and hit-test work on every frame for the rest of the session.
pub const FREEHAND_MIN_STEP_PX: f32 = 4.0;

/// Hard ceiling on one stroke, so a pointer that never stops moving cannot
/// turn a single drawing into an unbounded cost.
pub const FREEHAND_MAX_POINTS: usize = 512;

/// Why an armed instance's region cannot honestly be tested right now, or
/// `None` when it can.
///
/// One rule, two readers: [`ChartPane::strategy_region`] shuts the gate on it
/// and [`ChartPane::badge_text_for`] prints it. Two copies would let the chart
/// paint a running bot over a region every bar is refused against — the
/// divergence a trader only discovers by watching a setup go by, which is
/// exactly how this was found.
///
/// The order is the order the trader can act on: another market needs the
/// region redrawn, a lost series needs the drawing re-anchored, a hidden one
/// needs a click.
pub fn region_pause(drawing: &drawings::Drawing, all_hidden: bool) -> Option<&'static str> {
    if drawing.foreign_market {
        return Some("region on another market — paused");
    }
    if drawing.off_series {
        return Some("region off its series — paused");
    }
    if drawing.hidden || all_hidden {
        return Some("region hidden — paused");
    }
    None
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
    pub const fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}
