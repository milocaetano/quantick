//! Candle direction and horizontal allocation, shared with chart consumers.
use quantick_engine::Bar;

/// A flat bar is bullish, including a newly opened forming bar.
#[must_use]
pub fn is_bullish(bar: &Bar) -> bool {
    bar.close >= bar.open
}

/// Centre and half-width of a candle's on-screen column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BarSlot {
    pub xc: f32,
    pub half_width: f32,
}

/// Canvas clearance on each side of an existing candle contour, in points.
pub const CONTOUR_CLEARANCE_PX: f32 = 0.5;

/// Small bodies do not acquire overlapping clearance bands through their centre.
#[must_use]
pub fn contour_backing_width(stroke: f32, minimum_span: f32) -> f32 {
    stroke + 2.0 * CONTOUR_CLEARANCE_PX.min((minimum_span - stroke).max(0.0) * 0.25)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doji_clearance_never_closes_the_remaining_body_interior() {
        for span in [0.5, 1.0, 2.0, 4.0, 20.0] {
            let stroke = 1.25_f32.min(span);
            let backed = contour_backing_width(stroke, span);
            assert!(backed >= stroke && backed <= stroke + 1.0);
            if stroke < span {
                assert!(backed < span);
            } else {
                assert_eq!(backed, stroke);
            }
        }
    }
}
