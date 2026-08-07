//! The scrollable/zoomable viewport over the bar series (TradingView-style).
//!
//! Candles have a **fixed pixel width** (the zoom); the newest bar sits near the
//! right edge, and the view is a window that can pan freely — through history and
//! into empty space past the newest bar — so dragging always moves the chart,
//! even when there are only a handful of bars. This is the pure state behind
//! that (candle width + the fractional bar index at the right edge + a follow
//! flag), unit-tested in CI with no egui or input handling.
//!
//! It owns the candles' pane only. The live lane is a band of screen beside it
//! with a clock of its own, so nothing here reaches the tape.

/// Narrowest a candle slot can be, in pixels (max zoom-out).
pub const MIN_CANDLE_WIDTH: f32 = 2.0;
/// Widest a candle slot can be, in pixels (max zoom-in).
///
/// Sized for the footprint's Detailed level: a `sell × buy` ladder cell is
/// only a number from ~72 px of candle, so the old 64 px ceiling kept the
/// most detailed view of the tape permanently out of reach. 160 px shows a
/// handful of bars with full ladders — the "read these five candles" zoom.
pub const MAX_CANDLE_WIDTH: f32 = 160.0;
/// How many empty bar-slots past the newest bar you may pan into.
const FUTURE_MARGIN_BARS: f32 = 40.0;

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
        // Panning into the empty future keeps that margin instead of snapping
        // back to live.
        self.follow = (self.right_bar - newest).abs() <= 0.5;
    }

    /// Pin the right edge back to the newest bar.
    pub fn snap_to_live(&mut self) {
        self.follow = true;
    }

    /// Bring `bar` to the middle of a window `width_px` wide — the object
    /// manager's "select and centre". Clamped to the newest bar (no future
    /// margin: nothing to centre on out there), so centring an object near
    /// the live edge lands on it and resumes following.
    pub fn center_on_bar(&mut self, bar: f32, width_px: f32, total: usize) {
        if total == 0 || self.candle_width <= 0.0 {
            return;
        }
        let newest = (total - 1) as f32;
        let half_window = 0.5 * width_px / self.candle_width;
        self.right_bar = (bar + half_window).clamp(0.0, newest);
        self.follow = (self.right_bar - newest).abs() <= 0.5;
    }

    /// Account for bars appearing at — or leaving — the front of the series:
    /// shift the right-edge bar index by the same amount so the visible window
    /// keeps showing the same bars instead of jumping.
    ///
    /// Signed, because the front can shrink as well as grow. Prepending older
    /// trades adds bars; re-trimming a venue prefix against a series that has
    /// just been re-cut further back *removes* them, and a window that only
    /// knew how to move one way would jump on the other.
    ///
    /// A no-op while following live — the newest bar, and thus the right edge,
    /// is unchanged whatever happened in front of it.
    pub fn shift_right_edge(&mut self, delta: isize) {
        if self.follow || delta == 0 {
            return;
        }
        self.right_bar = (self.right_bar + delta as f32).max(0.0);
    }

    /// Put the right edge back on `bar` of a series that was rebuilt under it.
    ///
    /// A rebuild (a new bar type or threshold) re-aligns every index: bar 2000
    /// of a tick series and bar 2000 of a volume series are not the same
    /// market moment, and the new series may not even be that long — a stale
    /// index leaves the window past the end of the data, showing nothing. The
    /// caller re-derives which bar the edge belongs on (by time, the one thing
    /// a rebuild preserves) and hands it over here; `None` means there is
    /// nothing left to anchor to, so the view returns to the live edge.
    ///
    /// A view already following live is left alone: its right edge is the
    /// newest bar by definition, whatever the rebuild did.
    ///
    /// The sibling of [`Self::shift_right_edge`], for the case that one cannot
    /// serve: prepending older history moves every index by a known count, so
    /// that one shifts by it; a re-cut series has no such count, so this one
    /// takes the destination outright.
    pub fn reanchor(&mut self, bar: Option<usize>, total: usize) {
        if self.follow {
            return;
        }
        match bar {
            Some(index) if total > 0 => {
                let newest = (total - 1) as f32;
                self.right_bar = (index as f32).clamp(0.0, newest);
                self.follow = (self.right_bar - newest).abs() <= 0.5;
            }
            _ => self.snap_to_live(),
        }
    }

    /// The x-pixel centre of bar `index`, given the chart's right edge x and the
    /// series length. The newest bar sits half a candle in from `chart_right`.
    #[must_use]
    pub fn x_center(&self, index: usize, chart_right: f32, total: usize) -> f32 {
        self.x_at_bar_position(index as f32, chart_right, total)
    }

    /// Map a fractional bar-centre coordinate to x pixels.
    ///
    /// Integer positions are candle centres, `index - 0.5` is the left edge of
    /// a slot and `index + 0.5` is its right edge. Order-book timestamps use
    /// these fractional positions so several depth updates can be shown inside
    /// one activity-sampled bar without changing candle spacing.
    #[must_use]
    pub fn x_at_bar_position(&self, bar_position: f32, chart_right: f32, total: usize) -> f32 {
        let right_bar = self.right_edge_bar(total);
        chart_right - (right_bar - bar_position + 0.5) * self.candle_width
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
    fn center_on_bar_puts_the_target_mid_window() {
        let mut v = Viewport::new();
        // 100 bars visible in an 800 px window at 8 px per candle.
        v.center_on_bar(200.0, 800.0, 500);
        assert!(!v.follows_live());
        assert_eq!(v.right_edge_bar(500), 250.0);

        // Centring near the newest bar lands on the live edge and follows.
        v.center_on_bar(499.0, 800.0, 500);
        assert!(v.follows_live());
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
    fn fractional_positions_map_to_candle_edges() {
        let v = Viewport::new();
        let right = 1000.0;
        let center = v.x_center(5, right, 10);
        let left = v.x_at_bar_position(4.5, right, 10);
        let right_edge = v.x_at_bar_position(5.5, right, 10);
        assert!((center - left - v.candle_width() / 2.0).abs() < 0.001);
        assert!((right_edge - center - v.candle_width() / 2.0).abs() < 0.001);
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
    fn shift_right_edge_keeps_the_history_view_steady() {
        let mut v = Viewport::new();
        v.pan_pixels(24.0, 10); // leave follow; right edge at bar 6 of 10
        let before = v.right_edge_bar(10);
        v.shift_right_edge(100); // 100 older bars prepended (total now 110)
        assert!(!v.follows_live());
        assert!((v.right_edge_bar(110) - (before + 100.0)).abs() < 0.001);
    }

    #[test]
    fn shift_right_edge_is_a_noop_while_following() {
        let mut v = Viewport::new(); // follows the newest
        v.shift_right_edge(100);
        assert!(v.follows_live());
        assert_eq!(v.right_edge_bar(110), 109.0);
    }

    /// The regression: a rebuild that shortens the series must not leave the
    /// window past the end of the data, where nothing is drawn at all.
    #[test]
    fn reanchoring_a_shortened_series_keeps_bars_on_screen() {
        let mut v = Viewport::new();
        v.pan_pixels(8_000.0, 3_000); // deep into the history of a long series
        assert!(!v.follows_live());
        assert!(v.visible_range(800.0, 40).0 >= v.visible_range(800.0, 40).1);

        // The rebuild left 40 bars, and bar 12 is the same market time.
        v.reanchor(Some(12), 40);
        let (start, end) = v.visible_range(800.0, 40);
        assert!(start < end, "the window must hold bars again");
        assert!((v.right_edge_bar(40) - 12.0).abs() < 0.001);
    }

    #[test]
    fn reanchoring_without_an_anchor_returns_to_the_live_edge() {
        let mut v = Viewport::new();
        v.pan_pixels(8_000.0, 3_000);
        v.reanchor(None, 40);
        assert!(v.follows_live());
        assert_eq!(v.right_edge_bar(40), 39.0);
    }

    #[test]
    fn reanchoring_onto_the_newest_bar_resumes_following() {
        let mut v = Viewport::new();
        v.pan_pixels(80.0, 3_000);
        v.reanchor(Some(39), 40);
        assert!(v.follows_live(), "the edge is the live edge again");
    }

    #[test]
    fn reanchoring_never_lands_outside_the_new_series() {
        let mut v = Viewport::new();
        v.pan_pixels(80.0, 3_000);
        // An anchor past the end (a rebuild that shrank the series harder than
        // the caller's index knew) still lands on a bar.
        v.reanchor(Some(9_999), 40);
        assert!(v.right_edge_bar(40) <= 39.0);
        // And an empty series has nothing to anchor to.
        v.pan_pixels(80.0, 40);
        v.reanchor(Some(3), 0);
        assert!(v.follows_live());
    }

    #[test]
    fn reanchoring_leaves_a_following_view_alone() {
        let mut v = Viewport::new();
        v.reanchor(Some(3), 40);
        assert!(v.follows_live(), "following already means the newest bar");
        assert_eq!(v.right_edge_bar(40), 39.0);
    }

    #[test]
    fn snap_to_live_resumes_following() {
        let mut v = Viewport::new();
        v.pan_pixels(50.0, 500);
        assert!(!v.follows_live());
        v.snap_to_live();
        assert!(v.follows_live());
    }

    /// The viewport is the candles' pane and nothing else: the live lane is a
    /// band beside it, so no pan or zoom here can move, shrink or empty it.
    #[test]
    fn the_viewport_reserves_nothing_for_the_live_lane() {
        let mut v = Viewport::new();
        assert_eq!(v.right_edge_bar(10), 9.0);
        v.pan_pixels(24.0, 10); // three candles into history
        assert!(!v.follows_live());
        v.pan_pixels(-24.0, 10);
        assert!(v.follows_live());
        assert_eq!(v.right_edge_bar(10), 9.0, "the newest bar is the edge");
    }
}
