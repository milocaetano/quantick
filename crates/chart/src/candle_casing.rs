//! The plate a candle sits on where its surface cases candles: the lit flow
//! pane lays each one on a white plate so the heat under it never touches the
//! body or the wick. A surface without a casing width draws no plate.
use crate::candle_pixels::CandlePixels;
use crate::geometry::{CandleGeometry, PixelRect, VerticalSegment};
use crate::style::ResolvedCandlePaint;

/// The wick's plate reaches this far either side of the wick beyond the width.
const WICK_PLATE_EXTRA_PX: f32 = 0.5;

/// The two plates under a candle of this geometry, centred on `xc`: the body
/// grown by `width` on every side, then the wick's span from high to low,
/// grown by `width` at both ends.
#[must_use]
pub fn candle_casing(geometry: &CandleGeometry, xc: f32, width: f32) -> [PixelRect; 2] {
    let body = geometry.body;
    let top = geometry.upper_wick.map_or(body.top, |wick| wick.top);
    let bottom = geometry.lower_wick.map_or(body.bottom, |wick| wick.bottom);
    let half = width + WICK_PLATE_EXTRA_PX;
    [
        PixelRect {
            left: body.left - width,
            right: body.right + width,
            top: body.top - width,
            bottom: body.bottom + width,
        },
        PixelRect {
            left: xc - half,
            right: xc + half,
            top: top - width,
            bottom: bottom + width,
        },
    ]
}

/// Where a candle was laid out: on its run's whole device pixels, or where
/// its geometry falls.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LaidCandle {
    Pixels(CandlePixels),
    Geometry(CandleGeometry),
}

/// The plates under a candle laid out as `laid`, centred on `xc`, `width`
/// wide: on whole pixels a plate hugs the snapped body and wick. The wick's
/// plate is `None` where `paint` draws no wick.
#[must_use]
pub fn bar_casing(
    laid: &LaidCandle,
    xc: f32,
    paint: &ResolvedCandlePaint,
    (pixels_per_point, width): (f32, f32),
) -> [Option<PixelRect>; 2] {
    let [body, wick] = match laid {
        LaidCandle::Pixels(pixels) => pixel_casing(pixels, pixels_per_point, width),
        LaidCandle::Geometry(geometry) => candle_casing(geometry, xc, width),
    };
    let wick_shows = paint.wick.is_some_and(|[.., alpha]| alpha > 0);
    [Some(body), wick_shows.then_some(wick)]
}

/// The plates under a whole-pixel candle, from the very pixels its run placed.
fn pixel_casing(pixels: &CandlePixels, pixels_per_point: f32, width: f32) -> [PixelRect; 2] {
    let body = pixels.body.in_points(pixels_per_point);
    let [upper, lower] = [pixels.upper_wick, pixels.lower_wick]
        .map(|wick| wick.map(|wick| wick.in_points(pixels_per_point)));
    let column = upper.or(lower).unwrap_or(body);
    let xc = (column.left + column.right) / 2.0;
    let segment = |wick: PixelRect| VerticalSegment {
        x: xc,
        top: wick.top,
        bottom: wick.bottom,
    };
    let geometry = CandleGeometry {
        body,
        upper_wick: upper.map(segment),
        lower_wick: lower.map(segment),
    };
    candle_casing(&geometry, xc, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candle_pixels::DeviceRect;

    #[test]
    fn the_plates_grow_the_body_and_span_the_wick_by_the_width() {
        let geometry = CandleGeometry {
            body: PixelRect {
                left: 10.0,
                right: 16.0,
                top: 40.0,
                bottom: 60.0,
            },
            upper_wick: Some(VerticalSegment {
                x: 13.0,
                top: 20.0,
                bottom: 40.0,
            }),
            lower_wick: None,
        };
        let [body, wick] = candle_casing(&geometry, 13.0, 1.5);
        assert_eq!(
            (body.left, body.right, body.top, body.bottom),
            (8.5, 17.5, 38.5, 61.5)
        );
        assert_eq!(
            (wick.left, wick.right, wick.top, wick.bottom),
            (11.0, 15.0, 18.5, 61.5)
        );
    }

    /// A whole-pixel candle's plates hug the device pixels it was placed on.
    #[test]
    fn whole_pixel_plates_hug_the_placed_pixels() {
        let rect = |left, top, right, bottom| DeviceRect {
            left,
            top,
            right,
            bottom,
        };
        let pixels = CandlePixels {
            body: rect(20, 80, 32, 120),
            solid: false,
            fill: None,
            bands: [None; 4],
            upper_wick: Some(rect(25, 40, 27, 80)),
            lower_wick: None,
        };
        let [body, wick] = pixel_casing(&pixels, 2.0, 1.5);
        assert_eq!(
            (body.left, body.right, body.top, body.bottom),
            (8.5, 17.5, 38.5, 61.5)
        );
        assert_eq!(
            (wick.left, wick.right, wick.top, wick.bottom),
            (11.0, 15.0, 18.5, 61.5)
        );
    }

    /// A hidden wick, or one painted fully transparent, lays no plate.
    #[test]
    fn a_hidden_wick_lays_no_plate() {
        let geometry = CandleGeometry {
            body: PixelRect {
                left: 10.0,
                right: 16.0,
                top: 40.0,
                bottom: 60.0,
            },
            upper_wick: Some(VerticalSegment {
                x: 13.0,
                top: 20.0,
                bottom: 40.0,
            }),
            lower_wick: None,
        };
        let laid = LaidCandle::Geometry(geometry);
        let paint = |wick| ResolvedCandlePaint {
            fill: Some([0, 0, 0, 255]),
            outline: [0; 4],
            wick,
            outline_width: 1.0,
            wick_width: 1.0,
            corner_radius: 0.0,
            min_body_height: 1.0,
        };
        let plates = |wick| bar_casing(&laid, 13.0, &paint(wick), (1.0, 1.5)).map(|p| p.is_some());
        assert_eq!(plates(Some([0, 0, 0, 255])), [true, true]);
        assert_eq!(plates(Some([0, 0, 0, 0])), [true, false]);
        assert_eq!(plates(None), [true, false]);
    }
}
