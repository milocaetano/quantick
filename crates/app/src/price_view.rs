//! Manual price-axis control (vertical pan, zoom and orientation).
//!
//! By default the price axis auto-fits the visible bars. Once the user drags the
//! chart vertically (pan) or drags the price gutter (zoom), the axis switches to
//! an explicit `(lo, hi)` range that holds as new bars arrive — TradingView
//! behaviour — until reset back to auto-fit. The view also owns which way up
//! the chart is: an expanding drag that flattens the bars past
//! [`FLIP_SPAN_FACTOR`] flips it upside down, and the axis menu's
//! "Inverted chart" toggle flips it outright. This is the pure state behind
//! all of that, unit-tested in CI.

use crate::chart::PriceScale;

/// How many auto-fit spans wide the price window can be stretched before an
/// expanding drag flips the chart upside down instead of shrinking it further.
///
/// At 40× the visible bars occupy 1/40 — under 3% — of the pane: flat to the
/// eye. Flipping there mirrors nothing legible, so the drag reads as one
/// continuous motion — shrink, flatten, grow again upside down. Only the drag
/// flips ([`PriceView::drag_zoom`]); the wheel zooms without a ceiling
/// ([`PriceView::zoom`]), because zooming far out to read a wide range is a
/// legitimate ask that must not turn the chart over.
pub const FLIP_SPAN_FACTOR: f64 = 40.0;

/// The vertical price view: auto-fit or a manual price range, either way up.
#[derive(Debug, Clone, Copy, Default)]
pub struct PriceView {
    /// `Some((lo, hi))` when the user has taken manual control; `None` auto-fits.
    manual: Option<(f64, f64)>,
    /// Upside down: low prices at the top of the pane.
    inverted: bool,
}

impl PriceView {
    /// A view that auto-fits the visible bars.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the axis is auto-fitting (not under manual control).
    #[must_use]
    pub fn is_auto(&self) -> bool {
        self.manual.is_none()
    }

    /// Whether the chart is upside down.
    #[must_use]
    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// Turn the chart upside down, or back. The one named door to the
    /// orientation: the axis menu's checkbox, the drag crossing the flip
    /// threshold ([`Self::drag_zoom`]) and the `QUANTICK_INVERTED` hook all
    /// pass through here.
    pub fn set_inverted(&mut self, inverted: bool) {
        self.inverted = inverted;
    }

    /// The `(lo, hi)` range to display: the manual range if set, else `auto`.
    #[must_use]
    pub fn resolve(&self, auto: (f64, f64)) -> (f64, f64) {
        self.manual.unwrap_or(auto)
    }

    /// The frame's price→pixel mapping in one call: the resolved range over
    /// `[top, bottom]`, carrying this view's orientation — so no caller can
    /// rebuild the scale and forget the chart is upside down.
    #[must_use]
    pub fn scale(&self, auto: (f64, f64), top: f32, bottom: f32) -> PriceScale {
        let (lo, hi) = self.resolve(auto);
        PriceScale::from_range(lo, hi, top, bottom).with_inverted(self.inverted)
    }

    /// Return to auto-fitting the visible bars.
    ///
    /// Framing only: an upside-down chart double-clicked back to auto-fit
    /// stays upside down. Orientation is a standing choice with doors of its
    /// own — the axis menu's toggle and the opposite drag — and a reset that
    /// also turned the chart over would make the panic gesture mean two
    /// things at once.
    pub fn reset(&mut self) {
        self.manual = None;
    }

    /// Pan the price window by `delta` price units (shifts both bounds), taking
    /// manual control from the current resolved range.
    pub fn pan(&mut self, delta: f64, auto: (f64, f64)) {
        if delta == 0.0 || !delta.is_finite() {
            return;
        }
        let (lo, hi) = self.resolve(auto);
        self.manual = Some((lo + delta, hi + delta));
    }

    /// Pan by a screen drag instead of a price delta: positive `delta_px`
    /// drags the picture down. The pixel→price conversion flips with the
    /// orientation, so the candles follow the pointer whichever way up the
    /// chart is.
    pub fn pan_screen(&mut self, delta_px: f64, price_per_px: f64, auto: (f64, f64)) {
        let sign = if self.inverted { -1.0 } else { 1.0 };
        self.pan(delta_px * price_per_px * sign, auto);
    }

    /// Zoom the price span by `factor` around its centre: `> 1` expands the span
    /// (smaller candles), `< 1` compresses it (bigger candles).
    pub fn zoom(&mut self, factor: f64, auto: (f64, f64)) {
        if factor <= 0.0 || !factor.is_finite() {
            return;
        }
        let (lo, hi) = self.resolve(auto);
        let center = f64::midpoint(lo, hi);
        let half = ((hi - lo) / 2.0 * factor).max(1e-9);
        self.manual = Some((center - half, center + half));
    }

    /// [`Self::zoom`] for the gutter drag: expanding past [`FLIP_SPAN_FACTOR`]
    /// auto-fit spans flips the chart upside down instead of shrinking it
    /// further.
    ///
    /// Crossing the threshold parks the span *at* the threshold — the
    /// increment that crossed is consumed by the flip, so the size the eye
    /// reads is continuous through it. A window some other gesture already
    /// pushed wider (the wheel zooms without flipping) keeps its span and only
    /// turns over: snapping it back to the threshold would jump.
    pub fn drag_zoom(&mut self, factor: f64, auto: (f64, f64)) {
        if factor <= 0.0 || !factor.is_finite() {
            return;
        }
        let auto_span = auto.1 - auto.0;
        if factor > 1.0 && auto_span > 0.0 {
            let (lo, hi) = self.resolve(auto);
            let flip_span = auto_span * FLIP_SPAN_FACTOR;
            if (hi - lo) * factor >= flip_span {
                self.inverted = !self.inverted;
                let center = f64::midpoint(lo, hi);
                let half = (hi - lo).max(flip_span) / 2.0;
                self.manual = Some((center - half, center + half));
                return;
            }
        }
        self.zoom(factor, auto);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: (f64, f64) = (100.0, 110.0);

    #[test]
    fn auto_by_default() {
        let v = PriceView::new();
        assert!(v.is_auto());
        assert!(!v.is_inverted());
        assert_eq!(v.resolve(AUTO), AUTO);
    }

    #[test]
    fn pan_shifts_both_bounds_and_takes_manual_control() {
        let mut v = PriceView::new();
        v.pan(5.0, AUTO); // shift up by 5
        assert!(!v.is_auto());
        assert_eq!(v.resolve(AUTO), (105.0, 115.0));

        // A different auto range is now ignored — the manual range holds.
        assert_eq!(v.resolve((200.0, 210.0)), (105.0, 115.0));
    }

    #[test]
    fn zoom_scales_span_around_center() {
        let mut v = PriceView::new();
        // center 105, span 10. factor 2 -> span 20 -> (95, 115).
        v.zoom(2.0, AUTO);
        assert_eq!(v.resolve(AUTO), (95.0, 115.0));

        // factor 0.5 from the current (95,115): center 105, span 20 -> 10 -> (100,110).
        v.zoom(0.5, AUTO);
        let (lo, hi) = v.resolve(AUTO);
        assert!((lo - 100.0).abs() < 1e-9 && (hi - 110.0).abs() < 1e-9);
    }

    #[test]
    fn reset_returns_to_auto() {
        let mut v = PriceView::new();
        v.pan(5.0, AUTO);
        v.reset();
        assert!(v.is_auto());
        assert_eq!(v.resolve(AUTO), AUTO);
    }

    #[test]
    fn degenerate_inputs_are_ignored() {
        let mut v = PriceView::new();
        v.pan(f64::NAN, AUTO);
        v.zoom(0.0, AUTO);
        v.zoom(-1.0, AUTO);
        v.drag_zoom(0.0, AUTO);
        v.drag_zoom(f64::NAN, AUTO);
        assert!(v.is_auto(), "no-op operations don't take manual control");
        assert!(!v.is_inverted(), "no-op operations don't flip");
    }

    #[test]
    fn drag_zoom_below_the_threshold_is_a_zoom() {
        let mut v = PriceView::new();
        v.drag_zoom(2.0, AUTO);
        assert!(!v.is_inverted());
        assert_eq!(v.resolve(AUTO), (95.0, 115.0));
    }

    #[test]
    fn an_expanding_drag_past_the_threshold_flips_and_parks_at_it() {
        let mut v = PriceView::new();
        // span 10 × 41 crosses 40 auto-spans (400): flip, span parked at 400.
        v.drag_zoom(41.0, AUTO);
        assert!(v.is_inverted());
        let (lo, hi) = v.resolve(AUTO);
        assert!((lo - -95.0).abs() < 1e-9 && (hi - 305.0).abs() < 1e-9);
    }

    #[test]
    fn the_opposite_drag_flips_back() {
        let mut v = PriceView::new();
        v.drag_zoom(41.0, AUTO);
        assert!(v.is_inverted());
        // Inverted, the gutter feeds the mirrored sense: the drag back toward
        // normal expands the span again, crossing the same threshold.
        v.drag_zoom(1.1, AUTO);
        assert!(!v.is_inverted(), "the second crossing turns it back over");
    }

    #[test]
    fn a_span_the_wheel_pushed_past_the_threshold_flips_without_jumping() {
        let mut v = PriceView::new();
        // The wheel zooms without flipping — even far past the threshold.
        v.zoom(60.0, AUTO);
        assert!(!v.is_inverted());
        let before = v.resolve(AUTO);
        // The next expanding drag flips, keeping the span it found.
        v.drag_zoom(1.01, AUTO);
        assert!(v.is_inverted());
        assert_eq!(v.resolve(AUTO), before, "no snap back to the threshold");
    }

    #[test]
    fn a_contracting_drag_never_flips() {
        let mut v = PriceView::new();
        v.zoom(60.0, AUTO); // wider than the threshold already
        v.drag_zoom(0.5, AUTO);
        assert!(!v.is_inverted());
    }

    #[test]
    fn reset_keeps_orientation() {
        let mut v = PriceView::new();
        v.set_inverted(true);
        v.pan(5.0, AUTO);
        v.reset();
        assert!(v.is_auto());
        assert!(v.is_inverted(), "auto-fit is framing, not orientation");
    }

    #[test]
    fn pan_screen_follows_the_pointer_either_way_up() {
        let mut v = PriceView::new();
        // Normal: dragging the picture down means higher prices enter from
        // the top — the window shifts up.
        v.pan_screen(10.0, 1.0, AUTO);
        assert_eq!(v.resolve(AUTO), (110.0, 120.0));

        let mut v = PriceView::new();
        v.set_inverted(true);
        // Inverted, the same downward drag shifts the window the other way.
        v.pan_screen(10.0, 1.0, AUTO);
        assert_eq!(v.resolve(AUTO), (90.0, 100.0));
    }

    #[test]
    fn scale_carries_the_orientation() {
        let mut v = PriceView::new();
        v.set_inverted(true);
        let scale = v.scale(AUTO, 0.0, 100.0);
        assert!(scale.is_inverted());
        assert!(
            (scale.y(100.0) - 0.0).abs() < f32::EPSILON,
            "lo maps to the top when inverted"
        );
        assert!((scale.y(110.0) - 100.0).abs() < f32::EPSILON);
    }
}
