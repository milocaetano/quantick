//! Candle geometry in whole device pixels, by TradingView's rule.
//!
//! The width of a body, its contour and its wick are decided once per zoom
//! from the bar pitch and the display scale, in device pixels, so every
//! edge lands on a pixel boundary and nothing is antialiased. The rule is
//! the one TradingView's open-source Lightweight Charts publishes
//! (`optimalCandlestickWidth` and the candlestick renderer), which the
//! trader's TradingView screenshots match row for row:
//!
//! - the body is the whole pitch up to 4 logical px and drifts toward 80 %
//!   of it as the candles widen; between [`SPECIAL_PITCH_FROM`] and
//!   [`SPECIAL_PITCH_TO`] it is a fixed 3 logical px;
//! - contour and wick are `max(1, floor(scale))` device px, a whole multiple
//!   of that when the style asks for a thicker line;
//! - the body's width and the wick's share a parity, so the wick sits
//!   exactly in the middle;
//! - a body with no room for a fill inside its contour is one solid bar in
//!   the contour colour, never a blend of the two;
//! - a body never overlaps the one before it: its left edge is cut back to
//!   the previous right edge. Touching is allowed.
//!
//! The contour is inside the body's footprint, never added around it.

use std::f64::consts::FRAC_PI_2;

use quantick_engine::Bar;

use crate::geometry::{CandleGeometry, PixelRect, PriceScale, candle_geometry};
use crate::style::{CandleStyle, ResolvedCandlePaint};

/// Narrowest pitch, in logical px, of the fixed-width band.
///
/// Lightweight Charts starts it at 2.5. The trader's captures put
/// TradingView's own bound between 2.56 and 2.85, a pitch measured to a few
/// per cent, so the published value stands.
pub const SPECIAL_PITCH_FROM: f64 = 2.5;
/// Widest pitch, in logical px, of the fixed-width band, and the pitch above
/// which the body starts leaving a gap.
pub const SPECIAL_PITCH_TO: f64 = 4.0;
/// Body width, in logical px, inside the fixed-width band.
const SPECIAL_BODY: f64 = 3.0;
/// How far the body's share of the pitch falls as the candles widen: from
/// the whole pitch toward `1 - WIDE_SHARE_DROP` of it.
const WIDE_SHARE_DROP: f64 = 0.2;
/// The thickest contour or wick a style can ask for, in lines.
const MAX_LINES: i32 = 4;

/// One zoom's candle widths, in whole device px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandleWidths {
    /// The body's full width, contour included.
    pub body: i32,
    /// The contour's thickness on each side; zero with no contour.
    pub border: i32,
    /// The wick's width.
    pub wick: i32,
    /// No fill fits inside the contour: the whole body wears the contour
    /// colour.
    pub solid: bool,
}

/// Body, contour and wick widths for bars `pitch_pt` logical px apart on a
/// display with `pixels_per_point` device px per logical px.
///
/// `outline_pt` and `wick_pt` are the style's line widths: under 1.5 they
/// give TradingView's one line of `max(1, floor(scale))` device px, 2 gives
/// two such lines, and so on. An `outline_pt` of zero means no contour, and
/// the fill takes the whole body.
#[must_use]
pub fn candle_widths(
    pitch_pt: f32,
    pixels_per_point: f32,
    outline_pt: f32,
    wick_pt: f32,
) -> CandleWidths {
    let dpr = scale(pixels_per_point);
    let pitch = if pitch_pt.is_finite() {
        f64::from(pitch_pt).max(0.0)
    } else {
        0.0
    };
    let unit = (dpr.floor() as i32).max(1);
    let body = if (SPECIAL_PITCH_FROM..=SPECIAL_PITCH_TO).contains(&pitch) {
        (SPECIAL_BODY * dpr).floor() as i32
    } else {
        let share = 1.0
            - WIDE_SHARE_DROP * (pitch.max(SPECIAL_PITCH_TO) - SPECIAL_PITCH_TO).atan() / FRAC_PI_2;
        let by_share = (pitch * share * dpr).floor() as i32;
        let whole = (pitch * dpr).floor() as i32;
        by_share.min(whole)
    };
    lines_around(body.max(unit), unit, outline_pt, wick_pt)
}

/// The same lines around a body `body_pt` logical px wide that something
/// else decided: the footprint, which lays its ladder out against the body.
#[must_use]
pub fn fitted_widths(
    body_pt: f32,
    pixels_per_point: f32,
    outline_pt: f32,
    wick_pt: f32,
) -> CandleWidths {
    let dpr = scale(pixels_per_point);
    let body = if body_pt.is_finite() {
        (f64::from(body_pt.max(0.0)) * dpr).floor().min(1e6) as i32
    } else {
        0
    };
    let unit = (dpr.floor() as i32).max(1);
    lines_around(body.max(unit), unit, outline_pt, wick_pt)
}

/// Wick and contour for a body `body` device px wide, `unit` px a line.
fn lines_around(mut body: i32, unit: i32, outline_pt: f32, wick_pt: f32) -> CandleWidths {
    let wick = unit * lines(wick_pt);
    if body >= 2 && body % 2 != wick % 2 {
        body -= 1;
    }
    let wick = wick.min(body).max(1);
    let mut border = if outline_pt == 0.0 {
        0
    } else {
        unit * lines(outline_pt)
    };
    if border > 0 && body <= 2 * border {
        border = unit.max((body - 1) / 2);
    }
    CandleWidths {
        body,
        border,
        wick,
        solid: border > 0 && body <= 2 * border,
    }
}

fn scale(pixels_per_point: f32) -> f64 {
    if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        f64::from(pixels_per_point)
    } else {
        1.0
    }
}

/// A style line width as a whole number of TradingView lines.
fn lines(width_pt: f32) -> i32 {
    if width_pt.is_finite() {
        (width_pt.round() as i32).clamp(1, MAX_LINES)
    } else {
        1
    }
}

/// A rectangle of whole device pixels; `right` and `bottom` are exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl DeviceRect {
    fn new(left: i32, top: i32, right: i32, bottom: i32) -> Option<Self> {
        (right > left && bottom > top).then_some(Self {
            left,
            top,
            right,
            bottom,
        })
    }

    /// The same rectangle in logical px, its edges on device-pixel
    /// boundaries.
    #[must_use]
    pub fn in_points(self, pixels_per_point: f32) -> PixelRect {
        let dpr = scale(pixels_per_point);
        let at = |px: i32| (f64::from(px) / dpr) as f32;
        PixelRect {
            left: at(self.left),
            right: at(self.right),
            top: at(self.top),
            bottom: at(self.bottom),
        }
    }
}

/// One candle's vertical extent in logical px: high, body top, body bottom
/// and low, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleYs {
    pub high: f32,
    pub body_top: f32,
    pub body_bottom: f32,
    pub low: f32,
}

impl CandleYs {
    /// The extent of a candle laid out in logical px.
    #[must_use]
    pub fn of(geometry: &CandleGeometry) -> Self {
        let body = geometry.body;
        Self {
            high: geometry.upper_wick.map_or(body.top, |wick| wick.top),
            body_top: body.top,
            body_bottom: body.bottom,
            low: geometry.lower_wick.map_or(body.bottom, |wick| wick.bottom),
        }
    }
}

/// Where one candle's pixels go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandlePixels {
    /// The body's footprint, contour included.
    pub body: DeviceRect,
    /// The whole body wears the contour colour.
    pub solid: bool,
    /// The fill inside the contour; `None` when solid.
    pub fill: Option<DeviceRect>,
    /// Top, bottom, left and right contour bands; empty bands are `None`.
    pub bands: [Option<DeviceRect>; 4],
    pub upper_wick: Option<DeviceRect>,
    pub lower_wick: Option<DeviceRect>,
}

/// Which width whole-pixel candles take, or `None` for candles drawn where
/// their geometry falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelBodies {
    /// TradingView's width for the zoom ([`candle_widths`]).
    Zoom,
    /// The width the footprint is laid out against ([`fitted_widths`]).
    Fitted,
}

/// How a frame's candles draw. The time chart takes TradingView's widths in
/// either theme. The flow pane's candles, which the heatmap, the bubbles and
/// the tape are laid against, stay where their geometry falls, and so do
/// Dark's while a footprint dresses them (fades their body for its ladder,
/// or moves them into its lane); Light's keep the footprint's width on whole
/// pixels. A footprint layer that is on but leaves the candle alone at this
/// zoom does not count.
#[must_use]
pub fn pixel_bodies(flow_pane: bool, footprint_dresses: bool, dark: bool) -> Option<PixelBodies> {
    match (flow_pane, footprint_dresses, dark) {
        (true, ..) | (false, true, true) => None,
        (false, false, _) => Some(PixelBodies::Zoom),
        (false, true, false) => Some(PixelBodies::Fitted),
    }
}

impl CandlePixels {
    /// What to paint, in order, each rectangle with its colour: the wicks,
    /// then a solid body in the contour colour, or the fill and the contour
    /// bands. Invisible parts are left out.
    #[must_use]
    pub fn paints(&self, paint: &ResolvedCandlePaint) -> [Option<(DeviceRect, [u8; 4])>; 7] {
        let with = |rect: Option<DeviceRect>, color: Option<[u8; 4]>| {
            rect.zip(color.filter(|color| color[3] > 0))
        };
        let (wick, outline) = (paint.wick, Some(paint.outline));
        let [top, bottom, left, right] = self.bands;
        let body = if self.solid {
            [Some(self.body), None, None, None, None]
        } else {
            [self.fill, top, bottom, left, right]
        };
        let fill_color = if self.solid { outline } else { paint.fill };
        [
            with(self.upper_wick, wick),
            with(self.lower_wick, wick),
            with(body[0], fill_color),
            with(body[1], outline),
            with(body[2], outline),
            with(body[3], outline),
            with(body[4], outline),
        ]
    }
}

/// Lays one zoom's candles out left to right so none overlaps the one
/// before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleRun {
    widths: CandleWidths,
    dpr: f64,
    body_right: Option<i32>,
    wick_right: Option<i32>,
}

impl CandleRun {
    /// A run of candles `widths` wide on a display with `pixels_per_point`
    /// device px per logical px.
    #[must_use]
    pub fn new(widths: CandleWidths, pixels_per_point: f32) -> Self {
        Self {
            widths,
            dpr: scale(pixels_per_point),
            body_right: None,
            wick_right: None,
        }
    }

    /// The run one frame's candles are laid out in: bars `pitch_pt` apart,
    /// bodies `body_pt` wide when `bodies` is [`PixelBodies::Fitted`], and
    /// the lines `style` asks for, without a contour it cannot see.
    #[must_use]
    pub fn for_frame(
        bodies: PixelBodies,
        pitch_pt: f32,
        body_pt: f32,
        style: &CandleStyle,
        pixels_per_point: f32,
    ) -> Self {
        let paint = style.resolved(true, false);
        let outline = if paint.outline[3] > 0 {
            paint.outline_width
        } else {
            0.0
        };
        let widths = match bodies {
            PixelBodies::Zoom => {
                candle_widths(pitch_pt, pixels_per_point, outline, paint.wick_width)
            }
            PixelBodies::Fitted => {
                fitted_widths(body_pt, pixels_per_point, outline, paint.wick_width)
            }
        };
        Self::new(widths, pixels_per_point)
    }

    /// The widths every candle of the run is laid out with.
    #[must_use]
    pub fn widths(&self) -> CandleWidths {
        self.widths
    }

    /// Place `bar`, centred at `centre_pt` logical px on `scale`, its body
    /// at least `min_body_height` logical px tall. Bars must come in
    /// left-to-right order.
    pub fn place_bar(
        &mut self,
        scale: &PriceScale,
        bar: &Bar,
        centre_pt: f32,
        min_body_height: f32,
    ) -> CandlePixels {
        let geometry = candle_geometry(scale, bar, centre_pt, 1.0, min_body_height);
        self.place(centre_pt, CandleYs::of(&geometry))
    }

    /// Place the next candle, centred at `centre_pt` logical px. Candles
    /// must come in left-to-right order.
    pub fn place(&mut self, centre_pt: f32, ys: CandleYs) -> CandlePixels {
        let (widths, dpr) = (self.widths, self.dpr);
        let px = |pt: f32| {
            let pt = if pt.is_finite() { f64::from(pt) } else { 0.0 };
            (pt * dpr).round().clamp(-1e9, 1e9) as i32
        };
        let centre = px(centre_pt);
        let body_width = widths.body.max(1);
        let wanted_left = centre - body_width / 2;
        let right = wanted_left + body_width;
        let left = self
            .body_right
            .map_or(wanted_left, |prev| wanted_left.max(prev))
            .min(right - 1);
        let wick_width = widths.wick.max(1);
        let wanted_wick = centre - wick_width / 2;
        let wick_right = wanted_wick + wick_width;
        let wick_left = self
            .wick_right
            .map_or(wanted_wick, |prev| wanted_wick.max(prev))
            .min(wick_right - 1);
        self.body_right = Some(right);
        self.wick_right = Some(wick_right);

        let border = widths.border.max(0);
        let top = px(ys.body_top);
        let bottom = px(ys.body_bottom).max(top + border.max(1));
        let body = DeviceRect {
            left,
            top,
            right,
            bottom,
        };
        let solid = widths.solid || (border > 0 && bottom - top <= 2 * border);
        let (fill, bands) = if solid {
            (None, [None; 4])
        } else {
            let fill_left = (wanted_left + border).max(left);
            let fill_right = right - border;
            let (inner_top, inner_bottom) = (top + border, bottom - border);
            (
                DeviceRect::new(fill_left, inner_top, fill_right, inner_bottom),
                [
                    DeviceRect::new(left, top, right, inner_top),
                    DeviceRect::new(left, inner_bottom, right, bottom),
                    DeviceRect::new(left, inner_top, fill_left, inner_bottom),
                    DeviceRect::new(fill_right, inner_top, right, inner_bottom),
                ],
            )
        };
        CandlePixels {
            body,
            solid,
            fill,
            bands,
            upper_wick: DeviceRect::new(wick_left, px(ys.high), wick_right, top),
            lower_wick: DeviceRect::new(wick_left, bottom, wick_right, px(ys.low)),
        }
    }
}

#[cfg(test)]
#[path = "candle_pixels_tests.rs"]
mod candle_pixels_tests;
