//! The chart pane's tunables: drag and zoom gains, the
//! FLOW caption and candle overlays. One file, so retuning touches one place.

/// Half-height of the grab band over a pane's top edge, in pixels. The rule stays a hairline (a
/// thick bar reads as a wall in the data); the handle around it makes it catchable and the resize
/// cursor announces it, the same bargain the live lane's divider and the canvas split strike.
pub(super) const PANE_DIVIDER_HANDLE_PX: f32 = 4.0;

/// How often the forming bar's footprint ladder is re-snapshotted for
/// drawing, in seconds. ~10 Hz: the eye reads the pattern, not the ticking
/// digits, and a layout that repaints per print reflows under the pointer.
pub(super) const LIVE_LADDER_REFRESH_S: f64 = 0.1;

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
