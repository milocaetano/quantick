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
/// maps to `bottom`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PriceScale {
    lo: f64,
    hi: f64,
    top: f32,
    bottom: f32,
}

impl PriceScale {
    /// Auto-scale to the high/low range of `bars` (and the forming `partial`),
    /// padded by `pad_frac` of the range on each side so candles don't touch the
    /// edges. Returns `None` if there is nothing to scale.
    #[must_use]
    pub fn auto(
        bars: &[Bar],
        partial: Option<&Bar>,
        top: f32,
        bottom: f32,
        pad_frac: f64,
    ) -> Option<Self> {
        let mut lo = f64::INFINITY;
        let mut hi = f64::NEG_INFINITY;
        for bar in bars.iter().chain(partial) {
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
        })
    }

    /// A scale over an explicit `[lo, hi]` price range mapped to `[top, bottom]`
    /// pixels. Used when the price axis is under manual pan/zoom rather than
    /// auto-fitting the visible bars.
    #[must_use]
    pub fn from_range(lo: f64, hi: f64, top: f32, bottom: f32) -> Self {
        // Guard against an inverted or zero span.
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
        }
    }

    /// The y-pixel for `price`.
    #[must_use]
    pub fn y(&self, price: f64) -> f32 {
        let span = self.hi - self.lo;
        if span.abs() < f64::EPSILON {
            return f32::midpoint(self.top, self.bottom);
        }
        let frac = ((self.hi - price) / span) as f32;
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
        let frac = f64::from((y - self.top) / height); // 0 at top (hi), 1 at bottom (lo)
        self.hi - frac * (self.hi - self.lo)
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
pub fn price_window(
    visible: &[Bar],
    visible_partial: Option<&Bar>,
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
    let upper_top = high_y.min(body.top);
    let lower_bottom = low_y.max(body.bottom);
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
const AXIS_UNITS: [(f64, &str); 3] = [(1e9, "B"), (1e6, "M"), (1e3, "k")];
/// Most decimals a tick label ever shows. Past this the step is so fine that
/// the digits stop distinguishing neighbouring labels.
const AXIS_MAX_DECIMALS: usize = 4;
/// Relative tolerance for "this many decimals writes the step back exactly".
/// Steps are 1/2/5 × a power of ten, so the only error to absorb is the one
/// binary floating point introduces.
const AXIS_STEP_EPSILON: f64 = 1e-6;

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
            let label = if tick == 0.0 {
                "0".to_owned()
            } else {
                format!("{:.*}{suffix}", decimals, tick / unit)
            };
            (tick, label)
        })
        .collect()
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
    let gap_px = f64::from(height_px) * ((second - first) / span);
    if !gap_px.is_finite() || gap_px <= 0.0 || gap_px >= f64::from(AXIS_LABEL_MIN_GAP_PX) {
        return ticks;
    }
    let stride = (f64::from(AXIS_LABEL_MIN_GAP_PX) / gap_px).ceil() as usize;
    let anchor = ticks.iter().position(|tick| *tick == 0.0).unwrap_or(0);
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
}
