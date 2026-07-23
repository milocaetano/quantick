//! The scrollable/zoomable viewport over the bar series.
//!
//! Exocharts-style navigation: drag to pan through history, scroll to zoom.
//! This is the pure state behind that — how many bars are visible (zoom) and
//! which absolute bar sits at the right edge (pan) — with no egui or input
//! handling, so it's unit-tested in CI.
//!
//! When the viewport is *following* the live edge, new bars keep it pinned to
//! the newest bar; once you pan back into history it holds an absolute right
//! edge, so incoming bars don't drift the view.

/// Fewest bars that can fill the width (max zoom-in).
pub const MIN_VISIBLE: f32 = 8.0;
/// Most bars that can fill the width (max zoom-out).
pub const MAX_VISIBLE: f32 = 5000.0;

/// The visible window over a bar series.
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    visible_bars: f32,
    /// Absolute index of the rightmost visible bar (used only when not
    /// following). Stored as `f32` so panning is smooth sub-bar.
    right_index: f32,
    follow: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            visible_bars: 150.0,
            right_index: 0.0,
            follow: true,
        }
    }
}

impl Viewport {
    /// A viewport following the live edge, showing ~150 bars.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// How many bars currently fill the width (fractional; drives candle width).
    #[must_use]
    pub fn visible_bars(&self) -> f32 {
        self.visible_bars
    }

    /// Whether the right edge is pinned to the newest bar.
    #[must_use]
    pub fn follows_live(&self) -> bool {
        self.follow
    }

    /// Zoom by a multiplicative factor: `< 1` zooms in (fewer, larger bars),
    /// `> 1` zooms out. Anchored to the right edge.
    pub fn zoom(&mut self, factor: f32) {
        if factor > 0.0 && factor.is_finite() {
            self.visible_bars = (self.visible_bars * factor).clamp(MIN_VISIBLE, MAX_VISIBLE);
        }
    }

    /// Pan by `drag_bars`: positive drags the content right, revealing older
    /// bars (the right edge moves into the past); negative moves toward the
    /// present. Reaching the newest bar resumes following.
    pub fn pan(&mut self, drag_bars: f32, total: usize) {
        if total == 0 {
            return;
        }
        let newest = (total - 1) as f32;
        let current = if self.follow {
            newest
        } else {
            self.right_index
        };
        let next = (current - drag_bars).clamp(0.0, newest);
        self.right_index = next;
        self.follow = next >= newest - 0.5;
    }

    /// Pin the right edge back to the newest bar.
    pub fn snap_to_live(&mut self) {
        self.follow = true;
    }

    /// The `[start, end)` absolute bar indices visible for a series of `total`
    /// bars. `end` is exclusive.
    #[must_use]
    pub fn visible_range(&self, total: usize) -> (usize, usize) {
        if total == 0 {
            return (0, 0);
        }
        let vis = (self.visible_bars.round() as usize).max(1);
        let end = if self.follow {
            total
        } else {
            (self.right_index.round() as usize + 1).min(total)
        };
        let start = end.saturating_sub(vis);
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_follows_live_and_shows_the_newest() {
        let v = Viewport::new();
        assert!(v.follows_live());
        let (start, end) = v.visible_range(1000);
        assert_eq!(end, 1000, "right edge at the newest");
        assert_eq!(end - start, 150, "default ~150 visible");
    }

    #[test]
    fn fewer_bars_than_the_window_shows_them_all() {
        let v = Viewport::new();
        assert_eq!(v.visible_range(10), (0, 10));
    }

    #[test]
    fn zoom_clamps_between_min_and_max() {
        let mut v = Viewport::new();
        for _ in 0..100 {
            v.zoom(0.5);
        }
        assert!((v.visible_bars() - MIN_VISIBLE).abs() < 0.001);
        for _ in 0..100 {
            v.zoom(2.0);
        }
        assert!((v.visible_bars() - MAX_VISIBLE).abs() < 0.001);
    }

    #[test]
    fn panning_into_the_past_stops_following_and_holds() {
        let mut v = Viewport::new();
        // 500 bars, drag right by 100 bars -> right edge at index 399.
        v.pan(100.0, 500);
        assert!(!v.follows_live());
        let (_, end) = v.visible_range(500);
        assert_eq!(end, 400, "right edge index 399, exclusive end 400");

        // New bars arrive (total 520) while not following: the view holds.
        let (_, end2) = v.visible_range(520);
        assert_eq!(end2, 400, "held view does not drift with new bars");
    }

    #[test]
    fn panning_back_to_the_edge_resumes_following() {
        let mut v = Viewport::new();
        v.pan(100.0, 500);
        assert!(!v.follows_live());
        v.pan(-100.0, 500); // drag left back to the present
        assert!(v.follows_live());
    }

    #[test]
    fn snap_to_live_resumes_following() {
        let mut v = Viewport::new();
        v.pan(50.0, 500);
        assert!(!v.follows_live());
        v.snap_to_live();
        assert!(v.follows_live());
        assert_eq!(v.visible_range(500).1, 500);
    }

    #[test]
    fn pan_cannot_go_before_the_first_bar() {
        let mut v = Viewport::new();
        v.pan(10_000.0, 500); // absurd drag into the past
        let (start, end) = v.visible_range(500);
        assert_eq!(end, 1, "right edge clamped to the first bar");
        assert_eq!(start, 0);
    }
}
