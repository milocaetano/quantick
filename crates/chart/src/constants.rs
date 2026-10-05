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

// Footprint level of detail (`footprint_lod.rs`).

/// Smallest font the ladder draws its quantities at, in pixels.
///
/// Seven, down from eight. Monospace digits hold their shape a size below what
/// prose needs — they are a fixed, familiar alphabet of ten — and every text
/// floor below is measured from this number, so a pixel here is worth several
/// pixels of candle in how soon the numbers arrive.
pub const LADDER_MIN_FONT_PX: f32 = 7.0;

/// Advance width of a monospace glyph, as a fraction of the font size.
pub const GLYPH_EM: f32 = 0.6;

/// Glyphs in the widest quantity the ladder writes (`58.1k`).
pub const QUANTITY_GLYPHS: f32 = 5.0;

/// Width of that quantity at the smallest font, in pixels.
pub const QUANTITY_PX: f32 = QUANTITY_GLYPHS * GLYPH_EM * LADDER_MIN_FONT_PX;

/// Clearance kept around a quantity inside the body it is drawn in.
///
/// A pixel and a half a side, not the six the row layout reserves when it is
/// *sizing* the font: what a floor has to guarantee is that the digits do not
/// reach the next candle, and the body already sits inside a gap
/// ([`crate::style::DEFAULT_CANDLE_GAP`]) that keeps them apart.
pub const QUANTITY_PADDING_PX: f32 = 3.0;

/// The share of a slot a candle body takes at the default style. The numbers
/// are drawn inside the *body*, so this is what turns a text budget into a
/// candle width.
pub const TYPICAL_BODY_FRAC: f32 = 0.72;

/// Candle-width floors per level, in pixels — the typography budget of what
/// each level draws.
///
/// The two text levels are **derived, never chosen**: Compact fits one
/// quantity across the body, Detailed one per half of it. Writing them as
/// arithmetic is what keeps the retune honest — the floors moved because
/// [`LADDER_MIN_FONT_PX`] moved (8 px → 7 px), and anyone tightening them
/// further has to move a number that means something first.
///
/// That gap is much of why the layer read as *slow to arrive*: a trader zoomed
/// in for numbers, got marks, and had nothing saying how much further to go
/// (the legend now says it).
///
/// The two levels that draw no text answer to geometry instead, and had no
/// such excuse for waiting. Marks are a POC dot and a zone tick — visible from
/// a candle six pixels wide. The profile is a textless histogram whose *shape*
/// is the signal, readable at twelve pixels while narrower candles show marks.
///
/// The footprint config's `detail_scale` moves all four together, for a trader
/// who wants detail earlier still (and tighter) or later and roomier.
pub const COMPACT_MIN_WIDTH: f32 = (QUANTITY_PX + QUANTITY_PADDING_PX) / TYPICAL_BODY_FRAC;

/// Profile's candle-width floor, in pixels.
pub const PROFILE_MIN_WIDTH: f32 = 12.0;

/// Marks' candle-width floor, in pixels.
pub const MARKS_MIN_WIDTH: f32 = 6.0;

/// Row-height floors per level. Profile rows survive down to hairline bands;
/// text rows need a legible line.
pub const DETAILED_MIN_ROW: f32 = 12.0;

/// Compact's row-height floor, in pixels.
pub const COMPACT_MIN_ROW: f32 = 11.0;

/// The two-sided dead band around every footprint floor, as a ratio: a change
/// in either direction must clear the floor with 15% to spare. A coarser
/// answer waits until the zoom is 15% past failing the current floor; a finer
/// one is adopted only once it clears its own floor by 15%. It holds the
/// detail level ([`LevelMemory::resolve`], scaling candle width and row
/// height) and the row multiple ([`LevelMemory::resolve_multiple`], scaling
/// the row floor), across one step only — a further jump is taken at once.
/// So a trackpad hovering on a boundary, or the auto-fit breathing with every
/// print, cannot blink the chart between modes mid-gesture.
///
/// [`LevelMemory::resolve`]: crate::footprint_lod::LevelMemory::resolve
/// [`LevelMemory::resolve_multiple`]: crate::footprint_lod::LevelMemory::resolve_multiple
pub(crate) const LEVEL_HYSTERESIS: f32 = 1.15;

/// The narrower dead band between Marks and Profile: one wheel step must
/// reverse that transition, yet a 1% nudge must not blink it.
pub(crate) const PROFILE_HYSTERESIS: f32 = 1.02;

/// Display-grouping multiples, smallest first. Integer multiples of the
/// capture grid keep row merges exact; round values keep the effective
/// grouping a number a trader can say out loud. The ladder runs to 10 000×
/// deliberately: a feed that never reports its tick leaves the capture grid
/// on the 0.01 fallback, and an index future at 180 000 needs a 200–500×
/// merge before a row is even one visible pixel — capping at 100× silently
/// locked those charts in Marks at every zoom.
pub const GROUP_SNAP: [i64; 16] = [
    1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000, 10_000,
];
