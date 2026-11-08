//! A candle body drawn as whole pixels, the way TradingView's light chart
//! draws it: the fill inside, and the contour as four filled bands a whole
//! number of pixels thick. A stroked rectangle feathers a one-pixel contour
//! into two grey columns; filled bands on pixel boundaries do not.

use crate::geometry::PixelRect;

/// Where a crisp body's fill and contour bands go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrispBody {
    /// The fill, inside the contour; `None` when the contour leaves no room.
    pub inner: Option<PixelRect>,
    /// Top, bottom, left and right bands; empty bands are `None`.
    pub bands: [Option<PixelRect>; 4],
}

/// The body from `min` to `max` (x, y) has its edges on pixel centres; half
/// a pixel out is the boundary the bands are laid on. The contour is
/// `outline_px` rounded to whole pixels, at least one and at most half the body.
#[must_use]
pub fn crisp_body(min: [f32; 2], max: [f32; 2], outline_px: f32) -> CrispBody {
    let outer = PixelRect {
        left: min[0] - 0.5,
        right: max[0] + 0.5,
        top: min[1] - 0.5,
        bottom: max[1] + 0.5,
    };
    let half = (outer.width().min(outer.height()) / 2.0).floor().max(1.0);
    let width = if outline_px.is_finite() {
        outline_px.round().clamp(1.0, half)
    } else {
        1.0
    };
    let rect = |left: f32, right: f32, top: f32, bottom: f32| {
        (right > left && bottom > top).then_some(PixelRect {
            left,
            right,
            top,
            bottom,
        })
    };
    let (top, bottom) = (outer.top + width, outer.bottom - width);
    CrispBody {
        inner: rect(outer.left + width, outer.right - width, top, bottom),
        bands: [
            rect(outer.left, outer.right, outer.top, top),
            rect(outer.left, outer.right, bottom, outer.bottom),
            rect(outer.left, outer.left + width, top, bottom),
            rect(outer.right - width, outer.right, top, bottom),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: f32, right: f32, top: f32, bottom: f32) -> PixelRect {
        PixelRect {
            left,
            right,
            top,
            bottom,
        }
    }

    #[test]
    fn a_one_pixel_contour_fills_the_edge_pixels_exactly() {
        let body = crisp_body([10.5, 20.5], [16.5, 40.5], 1.0);
        assert_eq!(body.inner, Some(rect(11.0, 16.0, 21.0, 40.0)));
        assert_eq!(
            body.bands,
            [
                Some(rect(10.0, 17.0, 20.0, 21.0)),
                Some(rect(10.0, 17.0, 40.0, 41.0)),
                Some(rect(10.0, 11.0, 21.0, 40.0)),
                Some(rect(16.0, 17.0, 21.0, 40.0)),
            ]
        );
    }

    #[test]
    fn the_contour_rounds_to_whole_pixels_and_never_swallows_the_body() {
        assert_eq!(
            crisp_body([0.5, 0.5], [8.5, 8.5], 1.25).bands[0],
            Some(rect(0.0, 9.0, 0.0, 1.0))
        );
        assert_eq!(
            crisp_body([0.5, 0.5], [8.5, 8.5], 2.6).bands[0],
            Some(rect(0.0, 9.0, 0.0, 3.0))
        );
        // A doji two pixels tall: one-pixel bands top and bottom, no fill.
        let doji = crisp_body([0.5, 0.5], [6.5, 1.5], 4.0);
        assert_eq!(doji.inner, None);
        assert_eq!(doji.bands[0], Some(rect(0.0, 7.0, 0.0, 1.0)));
        assert_eq!(doji.bands[2], None);
        assert_eq!(
            crisp_body([0.5, 0.5], [6.5, 6.5], f32::NAN).bands[0],
            Some(rect(0.0, 7.0, 0.0, 1.0))
        );
    }
}
