//! Tunable paddings, label spacings, strip sizes, flip thresholds and
//! display cutoffs of the crate-root chart modules. One place for later
//! config wiring; `flow_execution/constants.rs` holds regional FLOW's own.
//! Public items keep their old paths through re-exports in their modules.

// Price axis fit (`geometry.rs`).

/// Fraction of the price span left as breathing room above and below the
/// candles, so they never touch the edges of the plot.
pub const AUTO_PAD_FRAC: f64 = 0.05;

/// Keep a positive price span between the two decorated tape edges.
pub(crate) const MAX_TAPE_PADDING_SHARE: f64 = 0.45;

/// Fraction of the held tape axis's useful span the recent prices must still
/// fill for the axis to hold; at or below it the axis refits and releases the
/// spare room.
pub(crate) const TAPE_AXIS_REFIT_SPAN_RATIO: f64 = 0.5;

// Candle geometry guards (`geometry.rs`).

// Constraining intermediates keeps additions and subtractions finite even for
// hostile inputs close to `f32::MAX`.
pub(crate) const SAFE_PIXEL_LIMIT: f32 = f32::MAX / 8.0;

/// Candle half width, in pixels, when the computed one is not finite.
pub(crate) const FALLBACK_HALF_WIDTH: f32 = 0.5;

/// Candle body height, in pixels, when the computed one is not finite.
pub(crate) const FALLBACK_BODY_HEIGHT: f32 = 1.0;

// Price and time axis labels (`geometry.rs`).

/// Pixels of axis height per label *asked for*.
///
/// The ask is not a promise: [`nice_ticks`](crate::geometry::nice_ticks) rounds the step to 1, 2 or 5,
/// which can hand back half of what was asked or nearly double it. So an axis
/// asks generously — one label per 20 pixels — and thins the result to fit
/// ([`AXIS_LABEL_MIN_GAP_PX`]). Asking modestly instead is what left a 163 px
/// CVD pane drawing a single label: an ask of three rounded down to one, and
/// there was nothing left to thin.
pub(crate) const AXIS_LABEL_SPACING_PX: f32 = 20.0;

/// Fewest labels an axis asks for: below two there is no scale to read, only
/// a number floating in a band.
pub(crate) const AXIS_MIN_TICKS: usize = 2;

/// Most labels an axis asks for, however tall it grows.
pub(crate) const AXIS_MAX_TICKS: usize = 8;

/// Least vertical room between two labels, in pixels. Below this the column
/// reads as texture rather than as numbers, and the axis drops every other
/// label until it clears.
pub(crate) const AXIS_LABEL_MIN_GAP_PX: f32 = 18.0;

/// Least horizontal room between two time labels, in pixels.
///
/// The horizontal twin of [`AXIS_LABEL_MIN_GAP_PX`]. Smaller than it because a
/// time label is read as one word and its neighbours are far apart in bars;
/// the price column is read as a column and needs more air.
pub(crate) const TIME_LABEL_MIN_GAP_PX: f32 = 12.0;

/// Comfortable distance between two time labels, in pixels.
///
/// The ask, as [`AXIS_LABEL_SPACING_PX`] is for price: a label roughly this
/// far apart reads as a scale rather than as a ribbon of numbers. It is a
/// floor on the spacing, never a cap — the collision rule can only push
/// labels further apart.
pub(crate) const TIME_LABEL_SPACING_PX: f32 = 110.0;

/// Fewest time labels a strip is worth writing at a given format. Below two
/// there is no scale to read, only an instant floating in a band — and that is
/// the signal to write the same axis in a shorter format instead.
pub(crate) const TIME_MIN_LABELS: usize = 2;

/// Time label font size, in pixels.
pub const TIME_LABEL_FONT_PX: f32 = 10.0;

/// Gap between the axis rule and a tick label, in pixels. Shared by the price
/// gutter and every pane's, so the numbers form one column down the chart.
pub const AXIS_LABEL_GAP_PX: f32 = 6.0;

/// Tick label font size, in pixels. Shared for the same reason.
pub const AXIS_LABEL_FONT_PX: f32 = 11.0;

/// Most decimals a tick label ever shows. Past this the step is so fine that
/// the digits stop distinguishing neighbouring labels.
pub(crate) const AXIS_MAX_DECIMALS: usize = 4;

/// Relative tolerance for "this many decimals writes the step back exactly".
/// Steps are 1/2/5 × a power of ten, so the only error to absorb is the one
/// binary floating point introduces.
pub(crate) const AXIS_STEP_EPSILON: f64 = 1e-6;

/// Decimals a compact readout shows below the smallest abbreviation unit.
pub(crate) const COMPACT_VALUE_DECIMALS: usize = 2;

// Live strip (`live_strip.rs`).

/// Width of the strip, in pixels. The proposal band is 72–96 px: wide enough
/// for the histogram to read, narrow enough to never crowd the chart.
pub const LIVE_STRIP_WIDTH_PX: f32 = 84.0;

/// Stroke of the best bid/ask touch markers, in pixels.
pub const TOUCH_MARKER_STROKE_PX: f32 = 1.5;

/// Alpha of the strip's left border line, against the chart body.
pub const STRIP_BORDER_ALPHA: f32 = 0.3;

/// Left inset of the strip's content, in pixels, so the border stays visible.
pub const STRIP_ROW_INSET_PX: f32 = 1.0;

/// Opacity of the histogram bars.
pub const HISTOGRAM_ALPHA: f32 = 0.8;

/// Widest histogram bar, as a fraction of the strip's half width, leaving a
/// sliver of background visible even at full scale.
pub const HISTOGRAM_MAX_HALF_FRAC: f32 = 0.94;

// Price view flip (`price_view.rs`).

/// How many auto-fit spans wide the price window can be stretched before an
/// expanding drag flips the chart upside down instead of shrinking it further.
///
/// At 40× the visible bars occupy 1/40 — under 3% — of the pane: flat to the
/// eye. Flipping there mirrors nothing legible, so the drag reads as one
/// continuous motion — shrink, flatten, grow again upside down. Only the drag
/// flips ([`PriceView::drag_zoom`](crate::price_view::PriceView::drag_zoom)); the
/// wheel zooms without a ceiling
/// ([`PriceView::zoom`](crate::price_view::PriceView::zoom)), because zooming far out to read a wide range is a
/// legitimate ask that must not turn the chart over.
pub const FLIP_SPAN_FACTOR: f64 = 40.0;

/// How far back inside [`FLIP_SPAN_FACTOR`] the span must contract before the
/// drag may flip again.
///
/// A flip parks the window at the threshold, where any expanding pixel would
/// cross it again: without this band a hand tremor at the boundary would
/// strobe the chart's orientation at frame rate. 5% is ~8px of gutter travel
/// (`AXIS_ZOOM_DRAG_PX · ln(1/0.95)`) — beyond any tremor, and invisible
/// inside the ~550px gesture that reaches the threshold at all (the bars are
/// equally flat at 95% and 100% of forty auto-fit spans).
pub const FLIP_REARM_FRACTION: f64 = 0.95;

// Footprint projection (`footprint_projection.rs`).

/// How many of the newest *closed* bars feed the adaptive imbalance floor.
pub const ADAPTIVE_FLOOR_BARS: usize = 50;

/// Percentile of per-row volume, over [`ADAPTIVE_FLOOR_BARS`], that sets the
/// adaptive imbalance floor.
pub(crate) const ADAPTIVE_FLOOR_PERCENTILE: usize = 60;

/// Least magnitude a footprint quantity is written in millions: the value
/// that rounds to `1000.0k` at one decimal rolls to `1.0M` instead.
pub(crate) const QTY_MILLIONS_FROM: f64 = 999_950.0;

/// Least magnitude a footprint quantity is written in thousands, for the same
/// rounding reason as [`QTY_MILLIONS_FROM`].
pub(crate) const QTY_THOUSANDS_FROM: f64 = 999.95;

/// Least magnitude a footprint quantity is written without decimals.
pub(crate) const QTY_WHOLE_FROM: f64 = 100.0;
