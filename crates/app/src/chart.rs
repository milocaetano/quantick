//! Pure chart geometry: mapping bars to pixel positions.
//!
//! This module has no dependency on egui, so the coordinate math — price → y,
//! bar index → x, auto-scaling to the visible range — is unit-testable in CI
//! without a display. The egui layer ([`crate::app`]) only turns these
//! positions into shapes.
//!
//! Prices are `Decimal` in the engine for exact arithmetic; here, at the display
//! boundary, they become `f64` (pixels are floating-point and determinism no
//! longer applies once we're drawing).

use quantick_engine::Bar;
use rust_decimal::prelude::ToPrimitive as _;

/// A `Decimal` price as an `f64` for pixel math (display only).
#[must_use]
pub fn to_f64(price: rust_decimal::Decimal) -> f64 {
    price.to_f64().unwrap_or(0.0)
}

/// Vertical price → y-pixel mapping, auto-scaled to a price range.
///
/// `hi` maps to `top` (smaller y, screen coordinates grow downward) and `lo`
/// maps to `bottom` — unless the scale is [inverted](Self::with_inverted),
/// when low prices ride at the top: the trader's upside-down view. Orientation
/// lives here, at the one place price becomes pixels, so every layer reading
/// this scale turns over together and none can be flipped alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceScale {
    lo: f64,
    hi: f64,
    top: f32,
    bottom: f32,
    inverted: bool,
}

impl PriceScale {
    /// Auto-scale to the high/low range of `bars` (and the forming `partial`),
    /// padded by `pad_frac` of the range on each side so candles don't touch the
    /// edges. Returns `None` if there is nothing to scale.
    #[must_use]
    pub fn auto<'a>(
        bars: impl IntoIterator<Item = &'a Bar>,
        partial: Option<&'a Bar>,
        top: f32,
        bottom: f32,
        pad_frac: f64,
    ) -> Option<Self> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for bar in bars.into_iter().chain(partial) {
            lo = lo.min(to_f64(bar.low));
            hi = hi.max(to_f64(bar.high));
        }
        if !lo.is_finite() || !hi.is_finite() {
            return None;
        }
        // Pad; guarantee a non-zero span even when hi == lo (a flat range).
        let span = (hi - lo).max(f64::EPSILON);
        let pad = span * pad_frac;
        Some(Self {
            lo: lo - pad,
            hi: hi + pad,
            top,
            bottom,
            inverted: false,
        })
    }

    /// A scale over an explicit `[lo, hi]` price range mapped to `[top, bottom]`
    /// pixels. Used when the price axis is under manual pan/zoom rather than
    /// auto-fitting the visible bars.
    #[must_use]
    pub fn from_range(lo: f64, hi: f64, top: f32, bottom: f32) -> Self {
        // Guard against a reversed or zero span.
        let (lo, hi) = if hi > lo {
            (lo, hi)
        } else {
            (lo - 0.5, lo + 0.5)
        };
        Self {
            lo,
            hi,
            top,
            bottom,
            inverted: false,
        }
    }

    /// The same scale turned upside down when `inverted`: `lo` maps to `top`
    /// and `hi` to `bottom`. `(lo, hi)` stay ordered as prices — orientation
    /// is how the range meets the screen, not a property of the range.
    #[must_use]
    pub fn with_inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }

    /// Whether low prices ride at the top of the pane.
    #[must_use]
    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// Pixels per unit of price, computed from the f64 range directly.
    ///
    /// Positive either way up: orientation never changes the density, and the
    /// consumers (row heights, LOD ladders) want a size, not a direction.
    ///
    /// Never derive this by subtracting two `y()` values: `y` returns f32,
    /// and at an index-future price the pixel position of price 0 sits a
    /// million pixels off-screen, where f32's resolution is 0.0625px — the
    /// difference of two such values is rounding noise, not a density
    /// (`y(0) - y(0.01)` blinked between 0.000 and 0.062 with every pan,
    /// and took the footprint's whole LOD ladder with it).
    #[must_use]
    pub fn px_per_price(&self) -> f64 {
        let span = self.hi - self.lo;
        if span.abs() < f64::EPSILON {
            return 0.0;
        }
        f64::from(self.bottom - self.top) / span
    }

    /// The y-pixel for `price`.
    #[must_use]
    pub fn y(&self, price: f64) -> f32 {
        let span = self.hi - self.lo;
        if span.abs() < f64::EPSILON {
            return f32::midpoint(self.top, self.bottom);
        }
        let frac = if self.inverted {
            ((price - self.lo) / span) as f32
        } else {
            ((self.hi - price) / span) as f32
        };
        self.top + frac * (self.bottom - self.top)
    }

    /// The price at a given y-pixel — the inverse of [`y`](PriceScale::y), for a
    /// crosshair readout.
    #[must_use]
    pub fn price_at(&self, y: f32) -> f64 {
        let height = self.bottom - self.top;
        if height.abs() < f32::EPSILON {
            return f64::midpoint(self.lo, self.hi);
        }
        let frac = f64::from((y - self.top) / height); // 0 at top, 1 at bottom
        if self.inverted {
            self.lo + frac * (self.hi - self.lo)
        } else {
            self.hi - frac * (self.hi - self.lo)
        }
    }

    /// The padded `(lo, hi)` price range this scale covers.
    #[must_use]
    pub fn range(&self) -> (f64, f64) {
        (self.lo, self.hi)
    }
}

/// The price window to draw a frame with, given what is in view.
///
/// Normally that is [`PriceScale::auto`] over the visible bars. When the view
/// holds none of them — a rebuild re-cut the series under a panned viewport,
/// or the user panned into the empty space past the newest bar — the chart
/// still has to read as a chart, with its axis, its tape and its badges, so
/// the window falls back to `last` (the previous frame's, so the axis holds
/// still instead of jumping) and then to the newest bar of the series. `None`
/// only when there is no data anywhere to scale to.
#[must_use]
pub fn price_window<'a>(
    visible: impl IntoIterator<Item = &'a Bar>,
    visible_partial: Option<&'a Bar>,
    last: Option<(f64, f64)>,
    newest: Option<&Bar>,
    top: f32,
    bottom: f32,
) -> Option<PriceScale> {
    PriceScale::auto(visible, visible_partial, top, bottom, AUTO_PAD_FRAC)
        .or_else(|| last.map(|(lo, hi)| PriceScale::from_range(lo, hi, top, bottom)))
        .or_else(|| PriceScale::auto(&[], newest, top, bottom, AUTO_PAD_FRAC))
}

/// Fraction of the price span left as breathing room above and below the
/// candles, so they never touch the edges of the plot.
pub const AUTO_PAD_FRAC: f64 = 0.05;

/// An axis-aligned candle body in pixel coordinates.
///
/// [`candle_geometry`] guarantees finite coordinates ordered as
/// `left <= right` and `top <= bottom`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PixelRect {
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl PixelRect {
    /// Width of the rectangle in pixels.
    #[must_use]
    pub fn width(self) -> f32 {
        self.right - self.left
    }

    /// Height of the rectangle in pixels.
    #[must_use]
    pub fn height(self) -> f32 {
        self.bottom - self.top
    }
}

/// One vertical wick segment in pixel coordinates.
///
/// Wicks are split around the body instead of being drawn through it. A
/// zero-length segment is omitted by [`candle_geometry`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerticalSegment {
    pub x: f32,
    pub top: f32,
    pub bottom: f32,
}

impl VerticalSegment {
    /// Length of the segment in pixels.
    #[must_use]
    pub fn length(self) -> f32 {
        self.bottom - self.top
    }
}

/// Renderer-independent geometry for one candlestick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleGeometry {
    pub body: PixelRect,
    pub upper_wick: Option<VerticalSegment>,
    pub lower_wick: Option<VerticalSegment>,
}

// Constraining intermediates keeps additions and subtractions finite even for
// hostile inputs close to `f32::MAX`.
const SAFE_PIXEL_LIMIT: f32 = f32::MAX / 8.0;
const FALLBACK_HALF_WIDTH: f32 = 0.5;
const FALLBACK_BODY_HEIGHT: f32 = 1.0;

fn safe_pixel(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-SAFE_PIXEL_LIMIT, SAFE_PIXEL_LIMIT)
    } else {
        fallback
    }
}

fn scale_fallback_y(scale: &PriceScale) -> f32 {
    match (scale.top.is_finite(), scale.bottom.is_finite()) {
        (true, true) => safe_pixel(f32::midpoint(scale.top, scale.bottom), 0.0),
        (true, false) => safe_pixel(scale.top, 0.0),
        (false, true) => safe_pixel(scale.bottom, 0.0),
        (false, false) => 0.0,
    }
}

fn safe_scaled_y(scale: &PriceScale, price: rust_decimal::Decimal) -> f32 {
    safe_pixel(scale.y(to_f64(price)), scale_fallback_y(scale))
}

/// Map one OHLC bar into safe, renderer-independent pixel geometry.
///
/// A sub-pixel body is expanded symmetrically around the open/close midpoint
/// until it reaches `min_body_height`. Upper and lower wicks are separate
/// segments ending at the body edges, so neither can show through a transparent
/// or outline-only body. Invalid dimensions receive visible finite fallbacks.
#[must_use]
pub fn candle_geometry(
    scale: &PriceScale,
    bar: &Bar,
    xc: f32,
    half_width: f32,
    min_body_height: f32,
) -> CandleGeometry {
    let xc = safe_pixel(xc, 0.0);
    let half_width = if half_width.is_finite() {
        half_width
            .abs()
            .clamp(FALLBACK_HALF_WIDTH, SAFE_PIXEL_LIMIT)
    } else {
        FALLBACK_HALF_WIDTH
    };
    let min_body_height = if min_body_height.is_finite() && min_body_height >= 0.0 {
        min_body_height.min(SAFE_PIXEL_LIMIT)
    } else {
        FALLBACK_BODY_HEIGHT
    };

    let y_open = safe_scaled_y(scale, bar.open);
    let y_close = safe_scaled_y(scale, bar.close);
    let raw_top = y_open.min(y_close);
    let raw_bottom = y_open.max(y_close);
    let raw_height = raw_bottom - raw_top;
    let (body_top, body_bottom) = if raw_height < min_body_height {
        let center = f32::midpoint(raw_top, raw_bottom);
        let half_height = min_body_height / 2.0;
        (center - half_height, center + half_height)
    } else {
        (raw_top, raw_bottom)
    };

    let body = PixelRect {
        left: xc - half_width,
        right: xc + half_width,
        top: body_top,
        bottom: body_bottom,
    };

    let high_y = safe_scaled_y(scale, bar.high);
    let low_y = safe_scaled_y(scale, bar.low);
    // Screen-space extremes rather than the price names: on an inverted scale
    // the high's pixel sits *below* the body, and reading "upper" from
    // `bar.high` alone would drop both wicks.
    let wick_top = high_y.min(low_y);
    let wick_bottom = high_y.max(low_y);
    let upper_top = wick_top.min(body.top);
    let lower_bottom = wick_bottom.max(body.bottom);
    let upper_wick = (upper_top < body.top).then_some(VerticalSegment {
        x: xc,
        top: upper_top,
        bottom: body.top,
    });
    let lower_wick = (lower_bottom > body.bottom).then_some(VerticalSegment {
        x: xc,
        top: body.bottom,
        bottom: lower_bottom,
    });

    CandleGeometry {
        body,
        upper_wick,
        lower_wick,
    }
}

/// Round "nice" price ticks spanning `[lo, hi]`, aiming for about `target`
/// labels, using Heckbert's nice-numbers algorithm so labels land on values
/// like 100, 102.5, 105 rather than 100.37, 102.71, ….
#[must_use]
pub fn nice_ticks(lo: f64, hi: f64, target: usize) -> Vec<f64> {
    if hi <= lo || target == 0 {
        return Vec::new();
    }
    let step = nice_num(nice_num(hi - lo, false) / target as f64, true);
    if step <= 0.0 || !step.is_finite() {
        return Vec::new();
    }
    let first = (lo / step).ceil() * step;
    let mut ticks = Vec::new();
    let mut v = first;
    // Guard the loop count in case of pathological inputs.
    for _ in 0..(target * 4 + 8) {
        if v > hi + step * 0.001 {
            break;
        }
        if v >= lo - step * 0.001 {
            ticks.push(v);
        }
        v += step;
    }
    ticks
}

/// Pixels of axis height per label *asked for*.
///
/// The ask is not a promise: [`nice_ticks`] rounds the step to 1, 2 or 5,
/// which can hand back half of what was asked or nearly double it. So an axis
/// asks generously — one label per 20 pixels — and thins the result to fit
/// ([`AXIS_LABEL_MIN_GAP_PX`]). Asking modestly instead is what left a 163 px
/// CVD pane drawing a single label: an ask of three rounded down to one, and
/// there was nothing left to thin.
const AXIS_LABEL_SPACING_PX: f32 = 20.0;
/// Fewest labels an axis asks for: below two there is no scale to read, only
/// a number floating in a band.
const AXIS_MIN_TICKS: usize = 2;
/// Most labels an axis asks for, however tall it grows.
const AXIS_MAX_TICKS: usize = 8;
/// Least vertical room between two labels, in pixels. Below this the column
/// reads as texture rather than as numbers, and the axis drops every other
/// label until it clears.
const AXIS_LABEL_MIN_GAP_PX: f32 = 18.0;
/// Thresholds a tick label is abbreviated at, largest first, with the suffix
/// that replaces the zeros. Below the smallest the value is printed as it is.
///
/// Same spellings as the aggression bubbles' own `format_quantity`: one chart,
/// one way to write a thousand.
const AXIS_UNITS: [(f64, &str); 3] = [(1e9, "B"), (1e6, "M"), (1e3, "K")];
/// Least horizontal room between two time labels, in pixels.
///
/// The horizontal twin of [`AXIS_LABEL_MIN_GAP_PX`]. Smaller than it because a
/// time label is read as one word and its neighbours are far apart in bars;
/// the price column is read as a column and needs more air.
const TIME_LABEL_MIN_GAP_PX: f32 = 12.0;
/// Comfortable distance between two time labels, in pixels.
///
/// The ask, as [`AXIS_LABEL_SPACING_PX`] is for price: a label roughly this
/// far apart reads as a scale rather than as a ribbon of numbers. It is a
/// floor on the spacing, never a cap — the collision rule can only push
/// labels further apart.
const TIME_LABEL_SPACING_PX: f32 = 110.0;
/// Fewest time labels a strip is worth writing at a given format. Below two
/// there is no scale to read, only an instant floating in a band — and that is
/// the signal to write the same axis in a shorter format instead.
const TIME_MIN_LABELS: usize = 2;
/// Time label font size, in pixels.
pub const TIME_LABEL_FONT_PX: f32 = 10.0;

/// How a time label is written, longest first.
///
/// Dropping seconds is the time axis' version of the price axis' `1.2M`: the
/// same instant, written coarser, so the axis stays a scale on a strip too
/// narrow for the full form. It is never the *labels* that are dropped to make
/// room — thinning already guarantees they cannot collide; this is what keeps
/// a narrow strip from being reduced to one lonely timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeLabelFormat {
    /// `HH:MM:SS` — the full instant.
    Full,
    /// `HH:MM` — seconds dropped.
    Short,
}

impl TimeLabelFormat {
    /// Longest first: the axis takes the first one that fits.
    pub const ALL: [Self; 2] = [Self::Full, Self::Short];

    /// A representative string of this format's exact width. Monospace, so
    /// every instant written this way measures the same — one measurement per
    /// frame answers for every label on the strip.
    #[must_use]
    pub fn sample(self) -> &'static str {
        match self {
            Self::Full => "00:00:00",
            Self::Short => "00:00",
        }
    }

    /// Write `hours`, `minutes` and `seconds` in this format.
    #[must_use]
    pub fn write(self, hours: i64, minutes: i64, seconds: i64) -> String {
        match self {
            Self::Full => format!("{hours:02}:{minutes:02}:{seconds:02}"),
            Self::Short => format!("{hours:02}:{minutes:02}"),
        }
    }
}

/// Bar slots to advance between two time labels so that neighbours can never
/// touch, given how wide one candle and one label are on screen.
///
/// This is the time axis' [`thin_to_fit`]: the old rule asked for a fixed six
/// labels however narrow the strip had become, and six `HH:MM:SS` need some
/// 300 px of text — which the history strip stops having once the live lane
/// takes its share of the chart. Counting labels cannot answer a question
/// about pixels.
///
/// Returns at least 1: a stride of zero would be an infinite loop, and one
/// label per bar is what a chart zoomed right in genuinely wants.
#[must_use]
pub fn time_label_stride(candle_width_px: f32, label_width_px: f32) -> usize {
    if !candle_width_px.is_finite() || candle_width_px <= 0.0 {
        return 1;
    }
    let label = if label_width_px.is_finite() {
        label_width_px.max(0.0)
    } else {
        0.0
    };
    // Whichever is further apart: comfortable, or far enough not to collide.
    let needed = (label + TIME_LABEL_MIN_GAP_PX).max(TIME_LABEL_SPACING_PX);
    let stride = (needed / candle_width_px).ceil();
    if stride.is_finite() && stride >= 1.0 {
        // A strip narrower than one label asks for a stride wider than any
        // chart has bars; saturating keeps that a very large number rather
        // than an undefined cast.
        stride as usize
    } else {
        1
    }
}

/// Whether a label of this width, centred at `x`, fits wholly inside
/// `[left, right]`.
///
/// Centre-only containment let the label at the strip's live end draw its
/// right half over the price gutter. A label half in the gutter is not a
/// smaller label, it is a smudge.
#[must_use]
pub fn label_fits(x: f32, label_width_px: f32, left: f32, right: f32) -> bool {
    let half = label_width_px / 2.0;
    x - half >= left && x + half <= right
}

/// The widest [`TimeLabelFormat`] that still writes [`TIME_MIN_LABELS`] labels
/// across a strip this wide, given each format's measured width.
///
/// `measured` answers with the pixel width of a format's
/// [`sample`](TimeLabelFormat::sample). Falls back to the shortest form when
/// even that does not fit — something has to be written, and the coarser
/// instant is still true.
#[must_use]
pub fn time_label_format(
    strip_width_px: f32,
    measured: impl Fn(TimeLabelFormat) -> f32,
) -> TimeLabelFormat {
    TimeLabelFormat::ALL
        .into_iter()
        .find(|format| {
            let width = measured(*format);
            let per_label = width + TIME_LABEL_MIN_GAP_PX;
            per_label > 0.0 && strip_width_px >= per_label * TIME_MIN_LABELS as f32
        })
        .unwrap_or(TimeLabelFormat::Short)
}

/// Gap between the axis rule and a tick label, in pixels. Shared by the price
/// gutter and every pane's, so the numbers form one column down the chart.
pub const AXIS_LABEL_GAP_PX: f32 = 6.0;
/// Tick label font size, in pixels. Shared for the same reason.
pub const AXIS_LABEL_FONT_PX: f32 = 11.0;
/// Most decimals a tick label ever shows. Past this the step is so fine that
/// the digits stop distinguishing neighbouring labels.
const AXIS_MAX_DECIMALS: usize = 4;
/// Relative tolerance for "this many decimals writes the step back exactly".
/// Steps are 1/2/5 × a power of ten, so the only error to absorb is the one
/// binary floating point introduces.
const AXIS_STEP_EPSILON: f64 = 1e-6;
/// Decimals a compact readout shows below the smallest abbreviation unit.
const COMPACT_VALUE_DECIMALS: usize = 2;

/// One value for a compact readout (the indicator legend's last-value cell):
/// the axis units' own spellings — `1.20M`, `3.40K` — so the legend and the
/// axis beside it can never write a thousand two ways. Non-finite values are
/// an honest `—`, never a `NaN` on screen.
#[must_use]
pub fn compact_value(value: f64) -> String {
    if !value.is_finite() {
        return "—".to_owned();
    }
    let (unit, suffix) = AXIS_UNITS
        .into_iter()
        .find(|(threshold, _)| value.abs() >= *threshold)
        .unwrap_or((1.0, ""));
    format!("{:.COMPACT_VALUE_DECIMALS$}{suffix}", value / unit)
}

/// Labelled ticks for a vertical axis `height_px` tall spanning `[lo, hi]`:
/// round numbers, written against the step they advance by and the magnitude
/// they reach — so a CVD in the millions reads `1.2M` while an oscillator
/// reads `70`.
///
/// Every label shares one unit, because a column mixing `900` and `1.1k` is a
/// column you have to read twice. The height decides both how many labels are
/// asked for and how many the column has room to keep.
#[must_use]
pub fn axis_labels(lo: f64, hi: f64, height_px: f32) -> Vec<(f64, String)> {
    let ticks = thin_to_fit(
        nice_ticks(lo, hi, tick_target(height_px)),
        hi - lo,
        height_px,
    );
    let step = match ticks.as_slice() {
        [first, second, ..] => second - first,
        _ => hi - lo,
    };
    let largest = ticks.iter().fold(0.0_f64, |acc, tick| acc.max(tick.abs()));
    let (unit, suffix) = AXIS_UNITS
        .into_iter()
        .find(|(threshold, _)| largest >= *threshold)
        .unwrap_or((1.0, ""));
    let decimals = tick_decimals(step / unit);
    ticks
        .into_iter()
        .map(|tick| {
            // Zero is zero in every unit; "0.0M" is three characters of noise
            // on the one label a flow pane is read against.
            let label = if is_zero_tick(tick, step) {
                "0".to_owned()
            } else {
                format!("{:.*}{suffix}", decimals, tick / unit)
            };
            (tick, label)
        })
        .collect()
}

/// Whether `tick` is the sequence's zero.
///
/// Not `tick == 0.0`: [`nice_ticks`] walks the range by adding `step`, so a
/// step that is not exact in binary (0.1, 0.05, …) lands on `-2.8e-17` where
/// zero should be — which prints as `-0.0` on the one label a flow pane is
/// read against, and hides zero from the thinning anchor. Anything within a
/// rounding error of a step is the zero.
fn is_zero_tick(tick: f64, step: f64) -> bool {
    tick.abs() <= step.abs() * AXIS_STEP_EPSILON
}

/// How many labels an axis `height_px` tall asks for.
fn tick_target(height_px: f32) -> usize {
    let by_room = if height_px.is_finite() && height_px > 0.0 {
        (height_px / AXIS_LABEL_SPACING_PX) as usize
    } else {
        0
    };
    by_room.clamp(AXIS_MIN_TICKS, AXIS_MAX_TICKS)
}

/// Keep every `stride`-th label until the column clears
/// [`AXIS_LABEL_MIN_GAP_PX`], anchored on zero when zero is one of them — it
/// is the line a flow pane is read against, and dropping it to keep a tidy
/// stride would cost the pane its only landmark.
///
/// What survives is still round: the ticks are an arithmetic sequence, so
/// keeping every other one only multiplies the step.
fn thin_to_fit(ticks: Vec<f64>, span: f64, height_px: f32) -> Vec<f64> {
    let [first, second, ..] = ticks.as_slice() else {
        return ticks;
    };
    let step = second - first;
    let gap_px = f64::from(height_px) * (step / span);
    if !gap_px.is_finite() || gap_px <= 0.0 || gap_px >= f64::from(AXIS_LABEL_MIN_GAP_PX) {
        return ticks;
    }
    let stride = (f64::from(AXIS_LABEL_MIN_GAP_PX) / gap_px).ceil() as usize;
    let anchor = ticks
        .iter()
        .position(|tick| is_zero_tick(*tick, step))
        .unwrap_or(0);
    ticks
        .into_iter()
        .enumerate()
        .filter(|(index, _)| index % stride == anchor % stride)
        .map(|(_, tick)| tick)
        .collect()
}

/// Decimals that keep two neighbouring labels apart when they advance by
/// `step`: a step of 250 needs none, 2.5k needs one, 0.05 needs two.
///
/// The fewest that write `step` back exactly, so no label is ever rounded
/// into its neighbour ("2.5" and "5.0" must not both print as "2" and "5").
fn tick_decimals(step: f64) -> usize {
    if !step.is_finite() || step <= 0.0 {
        return 0;
    }
    (0..AXIS_MAX_DECIMALS)
        .find(|decimals| {
            let factor = 10f64.powi(i32::try_from(*decimals).unwrap_or(0));
            ((step * factor).round() / factor - step).abs() <= step * AXIS_STEP_EPSILON
        })
        .unwrap_or(AXIS_MAX_DECIMALS)
}

/// A "nice" number near `range`: 1, 2, 5 or 10 × a power of ten. When `round`,
/// picks the nearest nice value; otherwise the smallest nice value ≥ `range`.
fn nice_num(range: f64, round: bool) -> f64 {
    if range <= 0.0 || !range.is_finite() {
        return 0.0;
    }
    let exp = range.log10().floor();
    let frac = range / 10f64.powf(exp);
    let nice = if round {
        if frac < 1.5 {
            1.0
        } else if frac < 3.0 {
            2.0
        } else if frac < 7.0 {
            5.0
        } else {
            10.0
        }
    } else if frac <= 1.0 {
        1.0
    } else if frac <= 2.0 {
        2.0
    } else if frac <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * 10f64.powf(exp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use std::str::FromStr as _;

    /// The legend's value cell speaks the axis' own abbreviations, and a
    /// missing value is a blank, never a `NaN`.
    #[test]
    fn compact_values_share_the_axis_spellings() {
        assert_eq!(compact_value(70.0), "70.00");
        assert_eq!(compact_value(-3.25), "-3.25");
        assert_eq!(compact_value(1_234.0), "1.23K");
        assert_eq!(compact_value(-2_500_000.0), "-2.50M");
        assert_eq!(compact_value(7.2e9), "7.20B");
        assert_eq!(compact_value(f64::NAN), "—");
        assert_eq!(compact_value(f64::INFINITY), "—");
    }

    fn bar(low: &str, high: &str) -> Bar {
        let l = Decimal::from_str(low).unwrap();
        let h = Decimal::from_str(high).unwrap();
        Bar {
            open_time: 0,
            close_time: 0,
            open: l,
            high: h,
            low: l,
            close: h,
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    fn ohlc(open: &str, high: &str, low: &str, close: &str) -> Bar {
        Bar {
            open_time: 0,
            close_time: 0,
            open: Decimal::from_str(open).unwrap(),
            high: Decimal::from_str(high).unwrap(),
            low: Decimal::from_str(low).unwrap(),
            close: Decimal::from_str(close).unwrap(),
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count: 1,
        }
    }

    #[test]
    fn empty_range_has_no_scale() {
        assert!(PriceScale::auto(&[], None, 0.0, 100.0, 0.05).is_none());
    }

    /// With bars in view the window is theirs, exactly as before the fallback
    /// existed.
    #[test]
    fn a_populated_view_scales_to_what_it_shows() {
        let bars = vec![bar("100.0", "110.0")];
        let window = price_window(&bars, None, Some((1.0, 2.0)), None, 0.0, 100.0).unwrap();
        let expected = PriceScale::auto(&bars, None, 0.0, 100.0, AUTO_PAD_FRAC).unwrap();
        assert_eq!(window, expected, "the fallback must not shadow real bars");
    }

    /// The regression behind the dark chart: an empty view used to yield no
    /// scale at all, and the frame was abandoned with only its background
    /// painted.
    #[test]
    fn an_empty_view_holds_the_last_window_instead_of_going_blank() {
        let newest = bar("100.0", "110.0");
        let window = price_window(&[], None, Some((50.0, 60.0)), Some(&newest), 0.0, 100.0)
            .expect("an empty view still draws");
        assert_eq!(window.range(), (50.0, 60.0), "the axis holds still");
    }

    #[test]
    fn an_empty_view_with_no_history_falls_back_to_the_newest_bar() {
        let newest = bar("100.0", "110.0");
        let window =
            price_window(&[], None, None, Some(&newest), 0.0, 100.0).expect("the market is there");
        let (lo, hi) = window.range();
        assert!(
            lo < 100.0 && hi > 110.0,
            "the newest bar, padded: {lo}..{hi}"
        );
    }

    #[test]
    fn with_no_data_at_all_there_is_still_nothing_to_scale() {
        assert!(price_window(&[], None, None, None, 0.0, 100.0).is_none());
    }

    #[test]
    fn hi_maps_to_top_and_lo_to_bottom() {
        let bars = vec![bar("100.0", "110.0")];
        let scale = PriceScale::auto(&bars, None, 0.0, 100.0, 0.0).unwrap();
        // With zero padding, 110 -> top (0), 100 -> bottom (100).
        assert!((scale.y(110.0) - 0.0).abs() < 0.001, "{}", scale.y(110.0));
        assert!((scale.y(100.0) - 100.0).abs() < 0.001, "{}", scale.y(100.0));
        // Midpoint price -> midpoint pixel.
        assert!((scale.y(105.0) - 50.0).abs() < 0.001, "{}", scale.y(105.0));
    }

    #[test]
    fn partial_bar_extends_the_range() {
        let bars = vec![bar("100.0", "110.0")];
        let partial = bar("90.0", "120.0");
        let scale = PriceScale::auto(&bars, Some(&partial), 0.0, 100.0, 0.0).unwrap();
        // The partial's 120/90 now bound the range.
        assert!((scale.y(120.0) - 0.0).abs() < 0.001);
        assert!((scale.y(90.0) - 100.0).abs() < 0.001);
    }

    #[test]
    fn flat_range_maps_to_the_middle() {
        let bars = vec![bar("100.0", "100.0")];
        let scale = PriceScale::auto(&bars, None, 0.0, 100.0, 0.0).unwrap();
        assert!((scale.y(100.0) - 50.0).abs() < 0.001);
    }

    #[test]
    fn price_at_is_the_inverse_of_y() {
        let bars = vec![bar("100.0", "110.0")];
        let scale = PriceScale::auto(&bars, None, 0.0, 100.0, 0.0).unwrap();
        for price in [100.0, 103.0, 107.5, 110.0] {
            let y = scale.y(price);
            assert!(
                (scale.price_at(y) - price).abs() < 1e-6,
                "price_at(y({price})) != {price}"
            );
        }
    }

    #[test]
    fn an_inverted_scale_maps_low_to_the_top() {
        let scale = PriceScale::from_range(100.0, 110.0, 0.0, 100.0).with_inverted(true);
        assert!(scale.is_inverted());
        assert!((scale.y(100.0) - 0.0).abs() < 0.001);
        assert!((scale.y(110.0) - 100.0).abs() < 0.001);
        assert!((scale.y(105.0) - 50.0).abs() < 0.001);
        // The range stays ordered as prices; orientation is not a reversal
        // of the bounds.
        assert_eq!(scale.range(), (100.0, 110.0));
        assert!(scale.px_per_price() > 0.0, "density has no orientation");
    }

    #[test]
    fn price_at_is_the_inverse_of_y_upside_down() {
        let scale = PriceScale::from_range(100.0, 110.0, 0.0, 100.0).with_inverted(true);
        for price in [100.0, 103.0, 107.5, 110.0] {
            let y = scale.y(price);
            assert!(
                (scale.price_at(y) - price).abs() < 1e-6,
                "price_at(y({price})) != {price}"
            );
        }
    }

    #[test]
    fn wicks_survive_the_flip() {
        // high 110 / low 90 around a 100–105 body, upside down: the low's
        // wick rides above the body on screen, the high's below.
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 200.0).with_inverted(true);
        let bar = ohlc("100.0", "110.0", "90.0", "105.0");
        let geometry = candle_geometry(&scale, &bar, 50.0, 4.0, 1.0);
        let upper = geometry.upper_wick.expect("the low's wick, on top");
        let lower = geometry.lower_wick.expect("the high's wick, below");
        assert!((upper.top - scale.y(90.0)).abs() < 0.001);
        assert!((upper.bottom - geometry.body.top).abs() < 0.001);
        assert!((lower.top - geometry.body.bottom).abs() < 0.001);
        assert!((lower.bottom - scale.y(110.0)).abs() < 0.001);
    }

    #[test]
    fn nice_ticks_are_round_and_in_range() {
        let ticks = nice_ticks(100.0, 110.0, 5);
        assert!(!ticks.is_empty());
        for t in &ticks {
            assert!(*t >= 100.0 && *t <= 110.0, "tick {t} out of range");
        }
        let step = ticks[1] - ticks[0];
        for pair in ticks.windows(2) {
            assert!((pair[1] - pair[0] - step).abs() < 1e-9);
        }
        // 100..110 targeting ~5 gives a round 2.0 step: 100,102,...,110.
        assert!((step - 2.0).abs() < 1e-9, "step = {step}");
    }

    #[test]
    fn nice_ticks_handles_degenerate_ranges() {
        assert!(nice_ticks(100.0, 100.0, 5).is_empty());
        assert!(nice_ticks(110.0, 100.0, 5).is_empty());
        assert!(nice_ticks(100.0, 110.0, 0).is_empty());
    }

    #[test]
    fn axis_labels_are_round_numbers_sharing_one_unit() {
        // An axis 120 px tall: what a fifth of a laptop-sized chart comes to.
        let labels = |lo: f64, hi: f64| -> Vec<String> {
            axis_labels(lo, hi, 120.0)
                .into_iter()
                .map(|(_, label)| label)
                .collect()
        };

        // A CVD in the millions: one unit for the whole column, because a
        // column mixing "900000" and "1M" is a column you read twice.
        assert_eq!(labels(0.0, 4.0e6), vec!["0", "1M", "2M", "3M", "4M"]);
        assert_eq!(labels(0.0, 4.0e3), vec!["0", "1K", "2K", "3K", "4K"]);
        // A finer step keeps its decimal rather than rounding two labels onto
        // the same number.
        assert_eq!(labels(0.0, 1.2e6), vec!["0", "0.5M", "1.0M"]);
        // An oscillator: plain integers, no suffix, no decimals.
        assert_eq!(labels(0.0, 100.0), vec!["0", "20", "40", "60", "80", "100"]);

        // A ratio near 1: the step decides the decimals, so no label is
        // rounded into its neighbour — or into a value it does not sit at.
        let ratio = axis_labels(0.9, 1.1, 120.0);
        let printed: Vec<&String> = ratio.iter().map(|(_, label)| label).collect();
        let unique: std::collections::BTreeSet<_> = printed.iter().collect();
        assert_eq!(
            unique.len(),
            printed.len(),
            "no repeated labels: {printed:?}"
        );
        for (tick, label) in &ratio {
            let value: f64 = label.parse().expect("a plain number near 1");
            assert!(
                (value - tick).abs() < 1e-9,
                "{label} must not round {tick} away"
            );
        }
    }

    #[test]
    fn a_degenerate_range_labels_nothing_instead_of_guessing() {
        assert!(axis_labels(5.0, 5.0, 120.0).is_empty());
        assert!(axis_labels(f64::NAN, 1.0, 120.0).is_empty());
        // An axis with no height cannot be measured for room; it still labels
        // round numbers rather than dividing by zero.
        assert!(!axis_labels(0.0, 10.0, 0.0).is_empty());
    }

    /// The bug this ask exists for, measured on a running chart: a 163 px CVD
    /// pane spanning roughly -30..30 asked for three labels, `nice_ticks`
    /// rounded that down to one, and the pane read as a band with a number
    /// floating in it.
    #[test]
    fn an_axis_asks_for_labels_generously_and_thins_them_to_fit() {
        assert_eq!(tick_target(40.0), 2, "a short axis still asks for two");
        assert_eq!(tick_target(163.0), 8);
        assert_eq!(tick_target(f32::NAN), AXIS_MIN_TICKS);

        assert_eq!(
            nice_ticks(-30.7, 29.4, 3).len(),
            1,
            "the shape of the bug, kept as evidence: a modest ask rounds to one"
        );
        let fitted = axis_labels(-30.7, 29.4, 163.0);
        assert!(
            fitted.len() >= 3,
            "asking generously gets the pane a scale: {fitted:?}"
        );

        // And the ask is not blindly honoured. Over 0..100 a 163 px axis is
        // handed eleven labels 16 px apart — texture, not numbers — so every
        // other one goes, and what stays is still round.
        assert_eq!(nice_ticks(0.0, 100.0, 8).len(), 11);
        let thinned = axis_labels(0.0, 100.0, 163.0);
        assert_eq!(
            thinned
                .iter()
                .map(|(_, label)| label.as_str())
                .collect::<Vec<_>>(),
            vec!["0", "20", "40", "60", "80", "100"]
        );
    }

    /// `nice_ticks` walks the range by adding `step`, so a sub-unit step lands
    /// on `-2.8e-17` where zero belongs. Printed naively that is `-0.0` on the
    /// one label a flow pane is read against — a normalised oscillator or a
    /// per-contract delta hits it, a CVD in the thousands never does, which is
    /// why every other test here passes over it.
    #[test]
    fn the_zero_label_survives_a_step_that_is_not_exact_in_binary() {
        for (lo, hi) in [(-0.324, 0.324), (-0.756, 0.756), (-0.19, 0.03)] {
            let labels: Vec<String> = axis_labels(lo, hi, 163.0)
                .into_iter()
                .map(|(_, label)| label)
                .collect();
            assert!(
                labels.iter().any(|label| label == "0"),
                "{lo}..{hi} must label its zero as zero: {labels:?}"
            );
            assert!(
                !labels.iter().any(|label| label.starts_with('-')
                    && label.parse::<f64>().is_ok_and(|value| value == 0.0)),
                "and never as a negative nothing: {labels:?}"
            );
        }
    }

    #[test]
    fn tick_decimals_are_the_fewest_that_write_the_step_back() {
        assert_eq!(tick_decimals(250.0), 0);
        assert_eq!(tick_decimals(1.0), 0);
        // 2500 shown in thousands: "2.5k", never "2k" twice in a column.
        assert_eq!(tick_decimals(2.5), 1);
        assert_eq!(tick_decimals(0.05), 2);
        assert_eq!(tick_decimals(0.0), 0, "a degenerate step asks for nothing");
    }

    #[test]
    fn bull_and_bear_bodies_share_the_same_price_bounds() {
        let scale = PriceScale::from_range(90.0, 120.0, 0.0, 300.0);
        let bull = candle_geometry(&scale, &ohlc("100", "115", "95", "110"), 50.0, 3.0, 1.0);
        let bear = candle_geometry(&scale, &ohlc("110", "115", "95", "100"), 50.0, 3.0, 1.0);

        assert_eq!(bull.body, bear.body);
        assert_eq!(bull.upper_wick, bear.upper_wick);
        assert_eq!(bull.lower_wick, bear.lower_wick);
        assert_eq!(
            bull.body,
            PixelRect {
                left: 47.0,
                right: 53.0,
                top: 100.0,
                bottom: 200.0,
            }
        );
    }

    #[test]
    fn doji_body_has_centered_minimum_height() {
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 200.0);
        let geometry = candle_geometry(&scale, &ohlc("100", "105", "95", "100"), 25.0, 2.0, 3.0);

        assert!((geometry.body.top - 98.5).abs() < 0.001);
        assert!((geometry.body.bottom - 101.5).abs() < 0.001);
        assert!((geometry.body.bottom - geometry.body.top - 3.0).abs() < 0.001);
        assert_eq!(geometry.upper_wick.unwrap().bottom, geometry.body.top);
        assert_eq!(geometry.lower_wick.unwrap().top, geometry.body.bottom);
    }

    #[test]
    fn short_body_expands_around_its_original_midpoint() {
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 200.0);
        let bar = ohlc("100", "105", "95", "100.1");
        let raw_midpoint = f32::midpoint(scale.y(100.0), scale.y(100.1));
        let geometry = candle_geometry(&scale, &bar, 25.0, 2.0, 4.0);

        assert!((geometry.body.bottom - geometry.body.top - 4.0).abs() < 0.001);
        assert!(
            (f32::midpoint(geometry.body.top, geometry.body.bottom) - raw_midpoint).abs() < 0.001
        );
    }

    #[test]
    fn zero_length_wicks_are_omitted() {
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 200.0);
        let geometry = candle_geometry(&scale, &ohlc("100", "105", "100", "105"), 25.0, 2.0, 1.0);

        assert!(geometry.upper_wick.is_none());
        assert!(geometry.lower_wick.is_none());
    }

    #[test]
    fn wick_segments_end_at_body_edges_and_never_cross_it() {
        let scale = PriceScale::from_range(90.0, 120.0, 0.0, 300.0);
        let geometry = candle_geometry(&scale, &ohlc("100", "115", "95", "110"), 50.0, 3.0, 1.0);
        let upper = geometry.upper_wick.unwrap();
        let lower = geometry.lower_wick.unwrap();

        assert!(upper.bottom - upper.top > 0.0);
        assert!(upper.top < upper.bottom);
        assert_eq!(upper.bottom, geometry.body.top);
        assert!(lower.bottom - lower.top > 0.0);
        assert!(lower.top < lower.bottom);
        assert_eq!(lower.top, geometry.body.bottom);
    }

    #[test]
    fn minimum_body_can_cover_wick_prices_without_crossing_segments() {
        let scale = PriceScale::from_range(99.0, 101.0, 0.0, 20.0);
        let geometry = candle_geometry(
            &scale,
            &ohlc("100", "100.05", "99.95", "100"),
            10.0,
            2.0,
            4.0,
        );

        assert_eq!(geometry.body.bottom - geometry.body.top, 4.0);
        assert!(geometry.upper_wick.is_none());
        assert!(geometry.lower_wick.is_none());
    }

    #[test]
    fn non_finite_dimensions_and_scale_produce_finite_safe_geometry() {
        let invalid_scale = PriceScale {
            lo: f64::NAN,
            hi: f64::INFINITY,
            top: f32::NAN,
            bottom: f32::INFINITY,
            inverted: false,
        };
        let geometry = candle_geometry(
            &invalid_scale,
            &ohlc("100", "110", "90", "105"),
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        );

        for value in [
            geometry.body.left,
            geometry.body.right,
            geometry.body.top,
            geometry.body.bottom,
        ] {
            assert!(value.is_finite(), "{value} must be finite");
        }
        assert!(geometry.body.right - geometry.body.left >= 1.0);
        assert!(geometry.body.bottom - geometry.body.top >= 1.0);
        assert!(geometry.upper_wick.is_none());
        assert!(geometry.lower_wick.is_none());
    }

    #[test]
    fn extreme_finite_x_and_negative_width_stay_finite_and_ordered() {
        let scale = PriceScale::from_range(90.0, 110.0, 0.0, 200.0);
        let geometry = candle_geometry(
            &scale,
            &ohlc("100", "105", "95", "100"),
            f32::MAX,
            -f32::MAX,
            f32::MAX,
        );

        assert!(geometry.body.left.is_finite());
        assert!(geometry.body.right.is_finite());
        assert!(geometry.body.top.is_finite());
        assert!(geometry.body.bottom.is_finite());
        assert!(geometry.body.left <= geometry.body.right);
        assert!(geometry.body.top <= geometry.body.bottom);
    }

    /// The defect this rule exists for: the old axis asked for a fixed six
    /// labels however narrow the strip was. Six `HH:MM:SS` need some 300 px of
    /// text, and the history strip stops having that once the live lane takes
    /// its share — so they overlapped. The stride now comes out of pixels, and
    /// a narrower chart simply gets fewer labels.
    #[test]
    fn a_narrow_chart_gets_fewer_labels_rather_than_overlapping_ones() {
        let label = 50.0; // an `HH:MM:SS` in monospace 10 px

        // Wide candles: one label every few bars.
        let wide = time_label_stride(40.0, label);
        // Narrow candles: the same pixel distance costs many more bars.
        let narrow = time_label_stride(4.0, label);
        assert!(
            narrow > wide,
            "thinner candles must space labels further apart in bars:              {wide} vs {narrow}"
        );

        // Whatever the candle width, neighbouring labels clear their own width.
        for candle in [0.5_f32, 1.0, 4.0, 13.0, 40.0, 120.0] {
            let gap = time_label_stride(candle, label) as f32 * candle;
            assert!(
                gap >= label + TIME_LABEL_MIN_GAP_PX,
                "candle {candle}: labels {gap} px apart cannot hold a {label} px label"
            );
        }
    }

    /// A stride of zero would be an infinite loop in the strip's own draw
    /// loop, and a degenerate candle width is exactly what a chart with no
    /// bars reports.
    #[test]
    fn the_stride_is_never_zero_however_degenerate_the_geometry() {
        for candle in [0.0_f32, -1.0, f32::NAN, f32::INFINITY] {
            assert!(time_label_stride(candle, 50.0) >= 1, "candle {candle}");
        }
        assert!(time_label_stride(10.0, f32::NAN) >= 1);
        assert!(time_label_stride(10.0, -5.0) >= 1);
    }

    /// Comfort is a floor on the spacing, never a cap: a chart zoomed so far in
    /// that one bar is wider than a label still must not write one label per
    /// bar, or the strip becomes a ribbon of numbers.
    #[test]
    fn very_wide_candles_still_leave_room_between_labels() {
        assert!(
            time_label_stride(60.0, 50.0) as f32 * 60.0 >= TIME_LABEL_SPACING_PX,
            "labels stay at least a comfortable distance apart"
        );
    }

    /// Seconds are dropped before labels are: a strip too narrow for two full
    /// timestamps writes the same instants coarser rather than showing one
    /// lonely label.
    #[test]
    fn a_strip_too_narrow_for_the_full_format_drops_the_seconds() {
        let measured = |format: TimeLabelFormat| match format {
            TimeLabelFormat::Full => 50.0,
            TimeLabelFormat::Short => 31.0,
        };
        assert_eq!(time_label_format(400.0, measured), TimeLabelFormat::Full);
        // 2 x (50 + 12) = 124: below that the full form stops fitting twice.
        assert_eq!(time_label_format(123.0, measured), TimeLabelFormat::Short);
        // Narrower than even the short form fits twice: it is still what gets
        // written, because a coarser instant is true and a blank axis is not.
        assert_eq!(time_label_format(10.0, measured), TimeLabelFormat::Short);
    }

    /// A label is placed by its centre but occupies its width. Containing only
    /// the centre let the label at the live end draw its right half over the
    /// price gutter.
    #[test]
    fn a_label_must_fit_whole_not_just_its_centre() {
        assert!(label_fits(100.0, 50.0, 0.0, 200.0), "well inside");
        assert!(
            !label_fits(190.0, 50.0, 0.0, 200.0),
            "right half in the gutter"
        );
        assert!(
            !label_fits(10.0, 50.0, 0.0, 200.0),
            "left half off the strip"
        );
        assert!(label_fits(25.0, 50.0, 0.0, 200.0), "exactly flush is fine");
    }
}
