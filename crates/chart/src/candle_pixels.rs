//! Candle geometry in whole device pixels, by TradingView's rule.
//!
//! The width of a body, its contour and its wick are decided once per zoom
//! from the bar pitch and the display scale, in device pixels, so every
//! edge lands on a pixel boundary and nothing is antialiased. The rule is
//! the one TradingView's open-source Lightweight Charts publishes
//! (`optimalCandlestickWidth` and the candlestick renderer), which the
//! trader's TradingView screenshots match row for row:
//!
//! - the body is the whole pitch up to 4 lines and drifts toward 80 % of
//!   it as the candles widen; between [`SPECIAL_PITCH_FROM`] and
//!   [`SPECIAL_PITCH_TO`] lines it is a fixed 3 lines. TradingView counts
//!   these in logical px, one of which is its line; here they are counted
//!   in lines, so the candles keep its proportions whatever the line
//!   rounds to (below);
//! - contour and wick are the style's line width in logical px, rounded to
//!   whole device px and never under one. TradingView floors its one-px
//!   line to `floor(scale)`, which at 1.5 px/pt is a single device px: half
//!   the thickness its own chart has on the trader's screen, where it runs at
//!   scale 2 and draws two. Rounding keeps a 1 px style at two device px
//!   there, the same physical line, and a 0.5 px style is the thin single
//!   pixel. Counting the layout in lines then draws, at 1.5 px/pt, the very
//!   candles TradingView draws at scale 2 for the same pitch on screen;
//! - the body's width and the wick's share a parity, so the wick sits
//!   exactly in the middle;
//! - a body with no room for a fill inside its contour (no wider than two
//!   contours) is one solid bar in the contour colour, never a ring around a
//!   sliver of fill;
//! - a body is never narrower than its wick;
//! - a body never overlaps the one before it: its left edge is cut back to
//!   the previous right edge. Touching is allowed.
//!
//! The contour is inside the body's footprint, never added around it.

use std::f64::consts::FRAC_PI_2;

use quantick_engine::Bar;

use crate::geometry::{CandleGeometry, PixelRect, PriceScale, candle_geometry};
use crate::style::{CandleStyle, ResolvedCandlePaint};

/// Narrowest pitch, in lines, of the fixed-width band.
///
/// Lightweight Charts starts it at 2.5. The trader's captures put
/// TradingView's own bound between 2.56 and 2.85, a pitch measured to a few
/// per cent, so the published value stands.
pub const SPECIAL_PITCH_FROM: f64 = 2.5;
/// Widest pitch, in lines, of the fixed-width band, and the pitch above
/// which the body starts leaving a gap.
pub const SPECIAL_PITCH_TO: f64 = 4.0;
/// Body width, in lines, inside the fixed-width band.
const SPECIAL_BODY: f64 = 3.0;
/// How far the body's share of the pitch falls as the candles widen: from
/// the whole pitch toward `1 - WIDE_SHARE_DROP` of it.
const WIDE_SHARE_DROP: f64 = 0.2;
/// The thickest contour or wick drawn, in device px.
const MAX_LINE_PX: i32 = 16;

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
/// `outline_pt` and `wick_pt` are the style's line widths in logical px,
/// each rounded to whole device px. An `outline_pt` of zero means no
/// contour, and the fill takes the whole body. The body is laid out in
/// lines of the contour (of the wick without one), TradingView's rule with
/// its logical px replaced by that line.
#[must_use]
pub fn candle_widths(
    pitch_pt: f32,
    pixels_per_point: f32,
    outline_pt: f32,
    wick_pt: f32,
) -> CandleWidths {
    let dpr = scale(pixels_per_point);
    let line = if outline_pt == 0.0 {
        line_px(wick_pt, dpr)
    } else {
        line_px(outline_pt, dpr)
    };
    let unit = f64::from(line);
    let pitch = if pitch_pt.is_finite() {
        f64::from(pitch_pt).max(0.0) * dpr / unit
    } else {
        0.0
    };
    let body = if (SPECIAL_PITCH_FROM..=SPECIAL_PITCH_TO).contains(&pitch) {
        (SPECIAL_BODY * unit).floor() as i32
    } else {
        let share = 1.0
            - WIDE_SHARE_DROP * (pitch.max(SPECIAL_PITCH_TO) - SPECIAL_PITCH_TO).atan() / FRAC_PI_2;
        let by_share = (pitch * share * unit).floor() as i32;
        let whole = (pitch * unit).floor() as i32;
        by_share.min(whole)
    };
    lines_around(body.max(line), dpr, outline_pt, wick_pt)
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
    lines_around(body, dpr, outline_pt, wick_pt)
}

/// Wick and contour for a body `body` device px wide.
fn lines_around(body: i32, dpr: f64, outline_pt: f32, wick_pt: f32) -> CandleWidths {
    let wick = line_px(wick_pt, dpr);
    let mut body = body.max(wick);
    if body >= 2 && body % 2 != wick % 2 {
        body -= 1;
    }
    let wick = wick.min(body).max(1);
    let border = if outline_pt == 0.0 {
        0
    } else {
        line_px(outline_pt, dpr)
    };
    CandleWidths {
        body,
        border,
        wick,
        solid: border > 0 && body <= 2 * border,
    }
}

/// A style line `width_pt` logical px wide, in whole device px: the nearest
/// whole number, at least one. At 1.5 px/pt a 1 px line is two device px and
/// a 0.5 px line one.
fn line_px(width_pt: f32, pixels_per_point: f64) -> i32 {
    let width = if width_pt.is_finite() {
        f64::from(width_pt)
    } else {
        1.0
    };
    ((width * pixels_per_point).round() as i32).clamp(1, MAX_LINE_PX)
}

/// A straight line `width_pt` logical px thick at `at_pt`, on whole device
/// pixels: its centre and width in logical px. The width rounds as a candle
/// contour's does ([`candle_widths`]: a 1 px line is two device px at
/// 1.5 px/pt), and the centre moves to the nearest place where both edges
/// fall on pixel boundaries, so nothing is antialiased: on a pixel's centre
/// for an odd width, between two pixels for an even one.
#[must_use]
pub fn crisp_line(at_pt: f32, width_pt: f32, pixels_per_point: f32) -> (f32, f32) {
    let dpr = scale(pixels_per_point);
    let width = line_px(width_pt, dpr);
    let at = if at_pt.is_finite() {
        f64::from(at_pt) * dpr
    } else {
        0.0
    };
    let left = (at + 0.5 - f64::from(width) / 2.0).floor();
    let centre = left + f64::from(width) / 2.0;
    ((centre / dpr) as f32, (f64::from(width) / dpr) as f32)
}

/// [`crisp_line`] for a one-point line when `crisp`, else the one-point
/// stroke where it falls: Light rules on whole pixels, Dark as it always has.
#[must_use]
pub fn crisp_or_point(crisp: bool, at_pt: f32, pixels_per_point: f32) -> (f32, f32) {
    if crisp {
        crisp_line(at_pt, 1.0, pixels_per_point)
    } else {
        (at_pt, 1.0)
    }
}

fn scale(pixels_per_point: f32) -> f64 {
    if pixels_per_point.is_finite() && pixels_per_point > 0.0 {
        f64::from(pixels_per_point)
    } else {
        1.0
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
    /// The extent from its four edges, top to bottom.
    #[must_use]
    pub const fn new(high: f32, body_top: f32, body_bottom: f32, low: f32) -> Self {
        Self {
            high,
            body_top,
            body_bottom,
            low,
        }
    }

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
/// either theme, and while a footprint dresses them (fades the bodies it has
/// a ladder for, or moves them into its lane) the footprint's width, still on
/// whole pixels. The flow pane's candles, which the heatmap, the bubbles and
/// the tape are laid against, stay where their geometry falls. A footprint
/// layer that is on but leaves the candle alone at this zoom does not count.
#[must_use]
pub fn pixel_bodies(flow_pane: bool, footprint_dresses: bool) -> Option<PixelBodies> {
    match (flow_pane, footprint_dresses) {
        (true, _) => None,
        (false, false) => Some(PixelBodies::Zoom),
        (false, true) => Some(PixelBodies::Fitted),
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

/// One candle `body_pt` logical px wide on its own, centred at `centre_pt`,
/// in the lines `paint` draws: the time chart's whole-pixel candle, as a
/// style preview shows it.
#[must_use]
pub fn lone_candle(
    centre_pt: f32,
    ys: CandleYs,
    body_pt: f32,
    pixels_per_point: f32,
    paint: &ResolvedCandlePaint,
) -> CandlePixels {
    let outline = if paint.outline[3] > 0 {
        paint.outline_width
    } else {
        0.0
    };
    let widths = fitted_widths(body_pt, pixels_per_point, outline, paint.wick_width);
    CandleRun::new(widths, pixels_per_point).place_painted(centre_pt, ys, paint)
}

/// Lays one zoom's candles out left to right so none overlaps the one
/// before it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CandleRun {
    widths: CandleWidths,
    dpr: f64,
    body_right: Option<i32>,
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
        }
    }

    /// The run one frame's candles are laid out in: bars `pitch_pt` apart,
    /// bodies `body_pt` wide when `bodies` is [`PixelBodies::Fitted`], and
    /// the lines `style` asks for, without a contour it cannot see. At
    /// [`PixelBodies::Zoom`] the body is TradingView's width, narrowed to
    /// `body_pt` only where the trader sized the body
    /// ([`CandleStyle::sizes_its_body`]); it is never widened. The corner
    /// radius has no place on whole pixels and is not drawn.
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
        let fitted = || fitted_widths(body_pt, pixels_per_point, outline, paint.wick_width);
        let widths = match bodies {
            PixelBodies::Zoom => {
                let zoom = candle_widths(pitch_pt, pixels_per_point, outline, paint.wick_width);
                let asked = style.sizes_its_body().then(fitted);
                asked.filter(|asked| asked.body < zoom.body).unwrap_or(zoom)
            }
            PixelBodies::Fitted => fitted(),
        };
        Self::new(widths, pixels_per_point)
    }

    /// The widths every candle of the run is laid out with.
    #[must_use]
    pub fn widths(&self) -> CandleWidths {
        self.widths
    }

    /// Place `bar`, centred at `centre_pt` logical px on `scale`, in the
    /// `paint` it is drawn with: its body at least that paint's minimum
    /// height, and its contour that paint's, so a bar whose own style draws
    /// no contour gets no ring of contour pixels. The width stays the run's.
    /// Bars must come in left-to-right order.
    pub fn place_bar(
        &mut self,
        scale: &PriceScale,
        bar: &Bar,
        centre_pt: f32,
        paint: &ResolvedCandlePaint,
    ) -> CandlePixels {
        let geometry = candle_geometry(scale, bar, centre_pt, 1.0, paint.min_body_height);
        self.place_painted(centre_pt, CandleYs::of(&geometry), paint)
    }

    /// Place a candle laid out in logical px, with the contour `paint`
    /// draws: none when its contour is invisible.
    pub fn place_painted(
        &mut self,
        centre_pt: f32,
        ys: CandleYs,
        paint: &ResolvedCandlePaint,
    ) -> CandlePixels {
        let border = if paint.outline[3] > 0 {
            line_px(paint.outline_width, self.dpr)
        } else {
            0
        };
        self.place_with(centre_pt, ys, border)
    }

    /// Place the next candle, centred at `centre_pt` logical px, with the
    /// run's own contour. Candles must come in left-to-right order.
    pub fn place(&mut self, centre_pt: f32, ys: CandleYs) -> CandlePixels {
        self.place_with(centre_pt, ys, self.widths.border)
    }

    /// Lays out one candle with a contour `border` device px thick; the
    /// body is solid when no fill fits inside it, in either direction.
    fn place_with(&mut self, centre_pt: f32, ys: CandleYs, border: i32) -> CandlePixels {
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
        // The wick keeps to the columns its body was left: never into the
        // body before it. Cut off them entirely, it moves onto them.
        let wick_width = widths.wick.max(1);
        let wanted_wick = centre - wick_width / 2;
        let (wick_left, wick_right) = if wanted_wick + wick_width > left {
            (wanted_wick.max(left), wanted_wick + wick_width)
        } else {
            (left, (left + wick_width).min(right))
        };
        self.body_right = Some(right);

        let border = border.max(0);
        let top = px(ys.body_top);
        let bottom = px(ys.body_bottom).max(top + border.max(1));
        let body = DeviceRect {
            left,
            top,
            right,
            bottom,
        };
        // No fill fits inside the contour across the body as cut, or down it.
        let solid = border > 0 && (right - left <= 2 * border || bottom - top <= 2 * border);
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
