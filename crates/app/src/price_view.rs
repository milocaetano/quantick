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

/// How far back inside [`FLIP_SPAN_FACTOR`] the span must contract before the
/// drag may flip again.
///
/// A flip parks the window at the threshold, where any expanding pixel would
/// cross it again: without this band a hand tremor at the boundary would
/// strobe the chart's orientation at frame rate. 5% is ~8px of gutter travel
/// (`AXIS_ZOOM_DRAG_PX · ln(1/0.95)`) — beyond any tremor, and invisible
/// inside the ~550px gesture that reaches the threshold at all (the bars are
/// equally flat at 95% and 100% of forty auto-fit spans).
pub const FLIP_REARM_FRACTION: f64 = 0.95;

/// The vertical price view: auto-fit or a manual price range, either way up.
#[derive(Debug, Clone, Copy, Default)]
pub struct PriceView {
    /// `Some((lo, hi))` when the user has taken manual control; `None` auto-fits.
    manual: Option<(f64, f64)>,
    /// Upside down: low prices at the top of the pane.
    inverted: bool,
    /// A drag flip just happened and the span still sits at the threshold:
    /// further expanding drags do nothing until a real contraction re-arms
    /// the flip ([`FLIP_REARM_FRACTION`]).
    flip_parked: bool,
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

    /// [`Self::zoom`] for the gutter drag: expanding to [`FLIP_SPAN_FACTOR`]
    /// auto-fit spans flips the chart upside down instead of shrinking it
    /// further.
    ///
    /// Below the threshold an expanding drag walks *up to* it, never through:
    /// however fast the flick, the chart flattens first and turns over on the
    /// next pull, so the flip always lands where nothing legible is left to
    /// mirror — the documented [`FLIP_SPAN_FACTOR`] contract. After a flip
    /// the boundary is parked until a real contraction re-arms it
    /// ([`FLIP_REARM_FRACTION`]), so a hand tremor at the threshold cannot
    /// strobe the orientation. A window some other gesture already pushed
    /// wider (the wheel zooms without a ceiling) keeps its span and only
    /// turns over: snapping it back to the threshold would jump.
    pub fn drag_zoom(&mut self, factor: f64, auto: (f64, f64)) {
        if factor <= 0.0 || !factor.is_finite() {
            return;
        }
        let auto_span = auto.1 - auto.0;
        // The boundary zone: at 99% of the threshold and beyond the chart is
        // equally flat, so this one band is both where an expanding drag
        // flips and what a contraction must leave to re-arm — and comparing
        // against the zone rather than the exact threshold keeps a span the
        // cap parked one float ulp short of it from missing the flip.
        let flip_zone = auto_span * FLIP_SPAN_FACTOR * FLIP_REARM_FRACTION;
        if factor <= 1.0 || auto_span <= 0.0 {
            // Contracting: a plain zoom — and once the span drops back out
            // of the boundary zone, the next crossing may flip again.
            self.zoom(factor, auto);
            if auto_span > 0.0 {
                let (lo, hi) = self.resolve(auto);
                if hi - lo < flip_zone {
                    self.flip_parked = false;
                }
            }
            return;
        }
        let (lo, hi) = self.resolve(auto);
        let span = hi - lo;
        if span >= flip_zone {
            if !self.flip_parked {
                self.inverted = !self.inverted;
                self.flip_parked = true;
            }
            return;
        }
        let flip_span = auto_span * FLIP_SPAN_FACTOR;
        self.zoom(factor.min(flip_span / span), auto);
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
    fn the_drag_flattens_to_the_threshold_and_flips_on_the_next_pull() {
        let mut v = PriceView::new();
        // One violent flick: capped at the 40-auto-span threshold (400),
        // still upright — the chart flattens first, whatever the speed.
        v.drag_zoom(1000.0, AUTO);
        assert!(!v.is_inverted());
        let (lo, hi) = v.resolve(AUTO);
        assert!(
            ((hi - lo) - 400.0).abs() < 1e-9,
            "capped at the threshold: {lo}..{hi}"
        );
        // The next expanding increment is the flip, keeping the span.
        v.drag_zoom(1.01, AUTO);
        assert!(v.is_inverted());
        assert_eq!(v.resolve(AUTO), (lo, hi));
    }

    #[test]
    fn the_opposite_drag_flips_back() {
        let mut v = PriceView::new();
        v.drag_zoom(1000.0, AUTO);
        v.drag_zoom(1.01, AUTO);
        assert!(v.is_inverted());
        // The gesture carries on: the mirrored sense contracts, growing the
        // upside-down chart and re-arming the flip.
        v.drag_zoom(0.5, AUTO);
        // The way back expands to the threshold again — the first crossing
        // only flattens, the next pull turns it upright.
        v.drag_zoom(1000.0, AUTO);
        assert!(v.is_inverted(), "the first crossing only flattens");
        v.drag_zoom(1.01, AUTO);
        assert!(!v.is_inverted());
    }

    #[test]
    fn a_tremor_at_the_boundary_cannot_strobe_the_orientation() {
        let mut v = PriceView::new();
        v.drag_zoom(1000.0, AUTO);
        v.drag_zoom(1.01, AUTO); // flip, parked
        assert!(v.is_inverted());
        // ±1px of hand tremor at the parked boundary: expanding pixels find
        // the flip parked, and a 1px contraction stays inside the re-arm
        // band — the chart must not turn over again.
        for _ in 0..10 {
            v.drag_zoom((1.0 / 150.0_f64).exp(), AUTO);
            v.drag_zoom((-1.0 / 150.0_f64).exp(), AUTO);
            assert!(v.is_inverted(), "still upside down");
        }
        // A real contraction re-arms; the next crossing flips back.
        v.drag_zoom(0.9, AUTO);
        v.drag_zoom(1000.0, AUTO);
        v.drag_zoom(1.01, AUTO);
        assert!(!v.is_inverted());
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
