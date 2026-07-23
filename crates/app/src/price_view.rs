//! Manual price-axis control (vertical pan and zoom).
//!
//! By default the price axis auto-fits the visible bars. Once the user drags the
//! chart vertically (pan) or drags the price gutter (zoom), the axis switches to
//! an explicit `(lo, hi)` range that holds as new bars arrive — TradingView
//! behaviour — until reset back to auto-fit. This is the pure state behind that,
//! unit-tested in CI.

/// The vertical price view: auto-fit, or a manual price range.
#[derive(Debug, Clone, Copy, Default)]
pub struct PriceView {
    /// `Some((lo, hi))` when the user has taken manual control; `None` auto-fits.
    manual: Option<(f64, f64)>,
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

    /// The `(lo, hi)` range to display: the manual range if set, else `auto`.
    #[must_use]
    pub fn resolve(&self, auto: (f64, f64)) -> (f64, f64) {
        self.manual.unwrap_or(auto)
    }

    /// Return to auto-fitting the visible bars.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: (f64, f64) = (100.0, 110.0);

    #[test]
    fn auto_by_default() {
        let v = PriceView::new();
        assert!(v.is_auto());
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
        assert!(v.is_auto(), "no-op operations don't take manual control");
    }
}
