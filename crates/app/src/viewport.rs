//! The scrollable/zoomable viewport over the bar series (TradingView-style).
//!
//! Candles have a **fixed pixel width** (the zoom); the newest bar sits near the
//! right edge, and the view is a window that can pan freely — through history and
//! into empty space past the newest bar — so dragging always moves the chart,
//! even when there are only a handful of bars. This is the pure state behind
//! that (candle width + the fractional bar index at the right edge + a follow
//! flag), unit-tested in CI with no egui or input handling.

/// Narrowest a candle slot can be, in pixels (max zoom-out).
pub const MIN_CANDLE_WIDTH: f32 = 2.0;
/// Widest a candle slot can be, in pixels (max zoom-in).
pub const MAX_CANDLE_WIDTH: f32 = 64.0;
/// How many empty bar-slots past the newest bar you may pan into.
const FUTURE_MARGIN_BARS: f32 = 20.0;

/// The visible window over a bar series.
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    /// Pixels per bar slot — the zoom.
    candle_width: f32,
    /// Fractional bar index at the right edge (used when not following). May
    /// exceed `total - 1` to show empty space past the newest bar.
    right_bar: f32,
    /// Whether the right edge is pinned to the newest bar.
    follow: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            candle_width: 8.0,
            right_bar: 0.0,
            follow: true,
        }
    }
}

impl Viewport {
    /// A viewport following the live edge at the default zoom.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pixels per bar slot (drives candle width and the pan/zoom maths).
    #[must_use]
    pub fn candle_width(&self) -> f32 {
        self.candle_width
    }

    /// Whether the right edge is pinned to the newest bar.
    #[must_use]
    pub fn follows_live(&self) -> bool {
        self.follow
    }

    /// Zoom by a multiplicative factor on the candle width: `> 1` widens the
    /// candles (zoom in), `< 1` narrows them (zoom out). Anchored to the right
    /// edge — the newest bar stays put.
    pub fn zoom(&mut self, factor: f32) {
        if factor > 0.0 && factor.is_finite() {
            self.candle_width =
                (self.candle_width * factor).clamp(MIN_CANDLE_WIDTH, MAX_CANDLE_WIDTH);
        }
    }

    /// The bar index at the right edge for a series of `total` bars.
    #[must_use]
    pub fn right_edge_bar(&self, total: usize) -> f32 {
        if self.follow {
            total.saturating_sub(1) as f32
        } else {
            self.right_bar
        }
    }

    /// Pan by `dx` pixels (a drag delta). Positive `dx` (drag right) reveals
    /// older bars — the right edge moves into the past; negative moves toward
    /// the present. Reaching the newest bar resumes following.
    pub fn pan_pixels(&mut self, dx: f32, total: usize) {
        if total == 0 || self.candle_width <= 0.0 || dx == 0.0 {
            return;
        }
        let newest = (total - 1) as f32;
        let current = self.right_edge_bar(total);
        let next = current - dx / self.candle_width;
        let max_right = newest + FUTURE_MARGIN_BARS;
        self.right_bar = next.clamp(0.0, max_right);
        // Follow only when the right edge is essentially *at* the newest bar.
        // Panning into the empty future (right_bar past newest) keeps that
        // margin instead of snapping back to live.
        self.follow = (self.right_bar - newest).abs() <= 0.5;
    }

    /// Pin the right edge back to the newest bar.
    pub fn snap_to_live(&mut self) {
        self.follow = true;
    }

    /// The x-pixel centre of bar `index`, given the chart's right edge x and the
    /// series length. The newest bar sits half a candle in from `chart_right`.
    #[must_use]
    pub fn x_center(&self, index: usize, chart_right: f32, total: usize) -> f32 {
        let right_bar = self.right_edge_bar(total);
        chart_right - (right_bar - index as f32 + 0.5) * self.candle_width
    }

    /// The `[start, end)` bar indices at least partly visible in a chart `width`
    /// pixels wide over `total` bars. Generous by up to a bar at each edge (the
    /// caller clips), so nothing pops in late.
    #[must_use]
    pub fn visible_range(&self, width: f32, total: usize) -> (usize, usize) {
        if total == 0 || self.candle_width <= 0.0 {
            return (0, 0);
        }
        let right_bar = self.right_edge_bar(total);
        let bars_across = width / self.candle_width;
        let start = (right_bar - bars_across).floor().max(0.0) as usize;
        let end = (right_bar.floor() as i64 + 2).clamp(0, total as i64) as usize;
        let start = start.min(end);
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_follows_live() {
        let v = Viewport::new();
        assert!(v.follows_live());
        assert_eq!(v.right_edge_bar(500), 499.0);
    }

    #[test]
    fn zoom_clamps_candle_width() {
        let mut v = Viewport::new();
        for _ in 0..100 {
            v.zoom(2.0);
        }
        assert!((v.candle_width() - MAX_CANDLE_WIDTH).abs() < 0.001);
        for _ in 0..100 {
            v.zoom(0.5);
        }
        assert!((v.candle_width() - MIN_CANDLE_WIDTH).abs() < 0.001);
    }

    #[test]
    fn x_centres_are_one_candle_apart_and_newest_is_near_the_right() {
        let v = Viewport::new(); // candle_width 8, following
        let right = 1000.0;
        // Newest bar (index 9 of 10) sits half a candle in from the right edge.
        assert!((v.x_center(9, right, 10) - (right - 4.0)).abs() < 0.001);
        // Adjacent bars are one candle_width apart.
        let a = v.x_center(5, right, 10);
        let b = v.x_center(6, right, 10);
        assert!((b - a - v.candle_width()).abs() < 0.001);
    }

    #[test]
    fn dragging_right_reveals_the_past_even_with_few_bars() {
        let mut v = Viewport::new();
        // 10 bars. Drag right by 24px (= 3 candles at width 8): right edge moves
        // back 3 bars, so the newest is no longer at the edge.
        v.pan_pixels(24.0, 10);
        assert!(!v.follows_live());
        assert!((v.right_edge_bar(10) - (9.0 - 3.0)).abs() < 0.001);
    }

    #[test]
    fn dragging_back_to_the_edge_resumes_following() {
        let mut v = Viewport::new();
        v.pan_pixels(24.0, 10);
        assert!(!v.follows_live());
        v.pan_pixels(-24.0, 10); // drag left back toward the present
        assert!(v.follows_live());
    }

    #[test]
    fn can_pan_into_empty_space_past_the_newest() {
        let mut v = Viewport::new();
        // Drag left hard (toward the future) — the right edge can move a bounded
        // margin past the newest bar.
        v.pan_pixels(-10_000.0, 10);
        let edge = v.right_edge_bar(10);
        assert!(edge > 9.0, "panned into empty future: {edge}");
        assert!(edge <= 9.0 + FUTURE_MARGIN_BARS + 0.001);
    }

    #[test]
    fn visible_range_follows_the_newest() {
        let v = Viewport::new(); // width 8
        // 1000 bars, chart 800px wide => ~100 bars across, ending at the newest.
        let (start, end) = v.visible_range(800.0, 1000);
        assert_eq!(end, 1000);
        // ~100 bars across (800px / 8px), generous by about a bar at the left.
        assert!(
            start < end && (898..=900).contains(&start),
            "start = {start}"
        );
    }

    #[test]
    fn snap_to_live_resumes_following() {
        let mut v = Viewport::new();
        v.pan_pixels(50.0, 500);
        assert!(!v.follows_live());
        v.snap_to_live();
        assert!(v.follows_live());
    }
}
