//! The plate a candle sits on where its surface cases candles: the lit flow
//! pane lays each one on a white plate so the heat under it never touches the
//! body or the wick. A surface without a casing width draws no plate.
use crate::geometry::{CandleGeometry, PixelRect};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::VerticalSegment;

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
}
