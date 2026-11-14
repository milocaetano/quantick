//! The canvas's fixed measures: the divider, the rail a dragged-shut pane
//! folds to and the threshold that folds it. A leaf: every module of the
//! canvas reads these and this reads nothing, so no two modules lean on
//! each other for a number.

/// Width of the draggable divider between two panes, in pixels.
pub const CANVAS_DIVIDER_PX: f32 = 4.0;

/// Width of a collapsed pane, in pixels.
///
/// **Never zero.** The vertical axis already settled this question — see
/// `quantick_chart::indicator_views::COLLAPSED_PANE_HEIGHT_PX`, "a pane that vanished would be an
/// indicator the user added and the chart silently dropped" — and the answer
/// is the same here: a pane with no width has no handle, and a pane with no
/// handle cannot be brought back from the canvas it left.
///
/// The two axes differ in pixels because they differ in what they must hold. A
/// collapsed indicator pane keeps a readable row of text, so it needs 20 px of
/// height; a collapsed chart pane keeps only a grip, so 8 px of width is
/// enough. What they share — and what may not drift — is the rule that
/// collapse leaves something to click.
pub const COLLAPSED_PANE_WIDTH_PX: f32 = 8.0;

/// How narrow a divider drag has to get before the pane is dismissed rather
/// than squeezed.
///
/// **In pixels, deliberately, and below [`MIN_PANE_WIDTH_PX`].** A share of
/// the canvas cannot be both: a fraction that sits under the floor on a
/// trading monitor sits over it on a laptop, and over it the floor never
/// binds — the pane would be dismissed while it still had room to be a chart,
/// and every width between the threshold and the floor would be unreachable.
///
/// Half the floor is the gesture: drag to the minimum and the pane holds
/// there; keep going, well past the point it stopped narrowing, and it is
/// dismissed. The travel past the floor is what makes the dismissal
/// deliberate.
pub const COLLAPSE_AT_PX: f32 = MIN_PANE_WIDTH_PX / 2.0;

/// How wide the collapsed rail's *hit* area is, against the
/// [`COLLAPSED_PANE_WIDTH_PX`] it paints.
///
/// The rail gives up almost all its width and keeps all of its reachability:
/// the extra reaches into the neighbouring chart, where it costs nothing but
/// the pointer's first few pixels. 24 px is the floor a pointer target is held
/// to, and a rail that photographed well but could not be hit would be a
/// picture of an affordance rather than one.
pub const COLLAPSED_HIT_PX: f32 = 24.0;

/// How far a press on a rail may travel, in points, and still be a click,
/// which restores the rail, rather than a drag: the toolkit's own click
/// slop, so a click on a rail is a click wherever the window reads one.
pub const RAIL_CLICK_PX: f32 = 6.0;

/// Narrowest the column painter squeezes a pane to while it is still open.
///
/// A pane is not all chart. The price axis takes `plot_area`'s gutter constant (64 px)
/// off its right edge before a single candle is drawn, and the flow pane's
/// tape takes more. This was 120 px and the doc said "below this a chart stops
/// being one" — which was true of 120 px itself: a floored pane came out with
/// **22 px** of chart on a 1124 px canvas, the axis having claimed the rest.
/// The number has to leave a chart behind, not just a pane.
///
/// A trader who wants less than this wants the pane collapsed, which is a
/// different request with a different affordance — and, at half this width,
/// one the same drag reaches by carrying on.
pub const MIN_PANE_WIDTH_PX: f32 = 240.0;

/// The floor a pointer target is held to, checked where it cannot be missed.
///
/// A compile error rather than a failing test: this is a property of the
/// constant above, and a reader lowering it should be stopped by the build
/// rather than by a test run they might not reach.
const _: () = assert!(
    COLLAPSED_HIT_PX >= 24.0,
    "the collapsed rail's hit area is under the minimum size a pointer target is held to"
);
const _: () = assert!(
    COLLAPSE_AT_PX > 0.0,
    "a collapse threshold at zero can never be crossed, so the pane could never be dismissed"
);
const _: () = assert!(
    COLLAPSE_AT_PX < MIN_PANE_WIDTH_PX,
    "a collapse threshold at or above the pane's own floor makes the floor unreachable:      the pane would be dismissed while it still had room to be a chart"
);
