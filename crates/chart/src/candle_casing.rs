//! The plate a candle sits on where its surface cases candles: the lit flow
//! pane lays each one on a white plate so the heat under it never touches the
//! body or the wick. A surface without a casing width draws no plate.
use crate::candle_pixels::{CandlePixels, CandleRun};
use crate::geometry::{CandleGeometry, PixelRect, PriceScale, VerticalSegment, candle_geometry};
use crate::style::ResolvedCandlePaint;
use quantick_engine::Bar;

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

/// The plates under `bar`'s candle, `width` wide, laid out as the candle is
/// painted: on `run`'s whole pixels where the candles snap, so a plate hugs
/// the snapped body and wick, else where `paint` puts its geometry. `run`
/// must see the bars in the order the candles' own run does.
#[must_use]
pub fn bar_casing(
    run: Option<&mut CandleRun>,
    (scale, bar): (&PriceScale, &Bar),
    (xc, half_width): (f32, f32),
    paint: &ResolvedCandlePaint,
    (pixels_per_point, width): (f32, f32),
) -> [PixelRect; 2] {
    match run {
        Some(run) => pixel_casing(
            &run.place_bar(scale, bar, xc, paint),
            pixels_per_point,
            width,
        ),
        None => {
            let geometry = candle_geometry(scale, bar, xc, half_width, paint.min_body_height);
            candle_casing(&geometry, xc, width)
        }
    }
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
}
