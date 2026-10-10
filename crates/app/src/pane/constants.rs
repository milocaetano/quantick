//! The chart pane's tunables: drag and zoom gains, the tape switch, the
//! FLOW caption and candle overlays. One file, so retuning touches one place.

use eframe::egui;

/// Half-height of the grab band over a pane's top edge, in pixels. The rule stays a hairline (a
/// thick bar reads as a wall in the data); the handle around it makes it catchable and the resize
/// cursor announces it, the same bargain the live lane's divider and the canvas split strike.
pub(super) const PANE_DIVIDER_HANDLE_PX: f32 = 4.0;

/// How often the forming bar's footprint ladder is re-snapshotted for
/// drawing, in seconds. ~10 Hz: the eye reads the pattern, not the ticking
/// digits, and a layout that repaints per print reflows under the pointer.
pub(super) const LIVE_LADDER_REFRESH_S: f64 = 0.1;

/// The tape switch's chip, in logical pixels.
///
/// A fixed size rather than one measured off its own text: the hit rect is
/// registered in the input pass and painted in the pass after it, and two
/// measurements of one chip are two chances for the button to be somewhere the
/// click is not.
pub(super) const TAPE_SWITCH_SIZE: egui::Vec2 = egui::vec2(54.0, 18.0);
/// Inset of that chip from the canvas's top-right corner.
pub(super) const TAPE_SWITCH_INSET: egui::Vec2 = egui::vec2(8.0, 4.0);
/// Gap between the switch and whatever sits to its left.
pub(super) const TAPE_SWITCH_GAP_PX: f32 = 6.0;

/// Room the switch takes off the right edge, for anything else that wants the
/// same corner — the book status badge is the one thing that does.
pub(super) const TAPE_SWITCH_RESERVED_PX: f32 =
    TAPE_SWITCH_SIZE.x + TAPE_SWITCH_INSET.x + TAPE_SWITCH_GAP_PX;

/// Corner radius of the chip, matching the status badge it sits beside.
pub(super) const TAPE_SWITCH_ROUNDING_PX: f32 = 3.0;

/// Chip background opacity over the canvas, resting and hovered. The resting
/// value is the status badge's, so the two read as one family of chrome.
pub(super) const TAPE_SWITCH_FILL_ALPHA: u8 = 165;
pub(super) const TAPE_SWITCH_HOVER_FILL_ALPHA: u8 = 210;

/// Opacity of the hover outline, relative to the chip's own accent.
pub(super) const TAPE_SWITCH_HOVER_STROKE_ALPHA: f32 = 0.7;
/// Width of every line the chip draws.
pub(super) const TAPE_SWITCH_STROKE_PX: f32 = 1.0;

/// State dot: how far its centre sits from the chip's left edge, and its
/// radius. Filled means the tape is on the canvas, hollow means it is not.
pub(super) const TAPE_SWITCH_DOT_X_PX: f32 = 9.0;
pub(super) const TAPE_SWITCH_DOT_RADIUS_PX: f32 = 3.0;

/// Where the label starts, measured from the same edge as the dot.
pub(super) const TAPE_SWITCH_LABEL_X_PX: f32 = 17.0;
/// Label size, matching the status badge's.
pub(super) const TAPE_SWITCH_FONT_PX: f32 = 11.0;
/// Opacity of the candle-aggression weighted-price dots over the candles (0-1).
pub(super) const CANDLE_AGGRESSION_DOT_OPACITY: f32 = 0.35;
/// Share of the candle lane a sidebar candle's half-width takes when the lane is narrowed.
pub(super) const SIDEBAR_BODY_FRAC: f32 = 0.35;
/// How soon a frame with FLOW work still pending asks to be painted again.
pub(super) const FLOW_PENDING_REPAINT: std::time::Duration = std::time::Duration::from_millis(16);
/// FLOW caption distance below the history pane's top (plus the legend inset), in pixels.
pub(super) const FLOW_CAPTION_TOP_PX: f32 = 6.0;
/// Gap between the native legend and the FLOW caption under it, in pixels.
pub(super) const FLOW_CAPTION_GAP_PX: f32 = 3.0;
