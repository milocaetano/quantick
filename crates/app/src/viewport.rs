//! The scrollable/zoomable viewport over the bar series (TradingView-style).
//!
//! Bars have a **fixed pixel width** (the zoom); the newest bar sits near the
//! right edge, and the view is a window that can pan freely — through history and
//! into empty space past the newest bar — so dragging always moves the chart,
//! even when there are only a handful of bars. This is the pure state behind
//! that (pixels per bar + the fractional bar index at the right edge + a follow
//! flag), unit-tested in CI with no egui or input handling.
//!
//! Two things it owns that a plain "one bar, one candle" window would not:
//!
//! - **Room to project into.** How far past the newest bar the view may go is
//!   derived from the window ([`Viewport::projection_margin_bars`]), so pushing
//!   the chart left always clears about a screen of empty canvas — enough for a
//!   channel or a Fibonacci extension drawn out in front of the market.
//! - **Grouping.** Squeezed past the point where a bar can be drawn on its own,
//!   several bars share one candle ([`Viewport::bars_per_slot`]) instead of the
//!   zoom simply stopping. Bars keep getting narrower; what is drawn for them
//!   does not, so more history arrives without the chart turning into a band.
//!
//! It owns the candles' pane only. The live lane is a band of screen beside it
//! with a clock of its own, so nothing here reaches the tape.

/// Narrowest a single **bar** can be, in pixels — the zoom-out floor.
///
/// A hundredth of a pixel: a 1600 px chart holds 160 000 bars there, which is
/// more than any pane in this app holds — the whole loaded series, whatever it
/// is. That is the point. The floor before this one was a floor on *drawing*
/// (two pixels is about the least a candle can be and still be one), and using
/// it as the zoom limit tied how much history a trader could see to how small a
/// candle can be drawn. Grouping ([`Viewport::bars_per_slot`]) separates the
/// two: bars keep shrinking, the thing drawn for them does not, and the cost of
/// a frame follows the candles rather than the bars behind them
/// ([`crate::bar_groups`]).
pub const MIN_PX_PER_BAR: f32 = 0.01;
/// The slot width grouping aims for, in pixels.
///
/// Four pixels is a two-pixel body with the default two-pixel gap around it —
/// the narrowest candle that still reads as a candle rather than a smear. The
/// multiple is the first one on [`SLOT_SNAP`] that reaches it, so a slot is
/// always at least this wide and at most a step over.
pub const TARGET_SLOT_PX: f32 = 4.0;
/// The zoom above which one bar always owns one candle, in pixels per bar.
///
/// Grouping may only exist in territory that had no chart at all before it:
/// two pixels per bar was the old zoom-out floor, so everything at or above it
/// draws exactly the bars the trader configured, one candle each, forever.
///
/// This is not a tuning constant — it is a promise. A trader reading order
/// flow enters on *one* bar: the elephant, the single print-storm bar that a
/// 5 000-tick imbalance rule cut. A chart that quietly folds that bar into its
/// neighbours while they zoom has taken away the thing they trade on. Above
/// this line the chart is theirs, unchanged; below it there was never a
/// readable bar to lose.
pub const UNGROUPED_MIN_PX_PER_BAR: f32 = 2.0;
/// Pixels per bar a chart opens on, and the zoom-out floor that always
/// remains reachable however short the series is.
pub const DEFAULT_PX_PER_BAR: f32 = 8.0;
/// The least of the window a squeezed series may fill.
///
/// Zooming out past the point where the whole series is on screen buys
/// nothing — there is no more history behind it — and costs everything: the
/// bars huddle into a corner and the rest of the chart is empty canvas. So
/// the floor follows the data, and the series always holds at least half the
/// window. It can only ever *lower* the floor below [`DEFAULT_PX_PER_BAR`],
/// never raise it above: a chart with ten bars on it must still zoom out to
/// its normal candles, and does.
const MIN_SERIES_FILL: f32 = 0.5;
/// The grouping multiples, in order — the only counts of bars a candle is ever
/// drawn from.
///
/// A ladder rather than "whatever number lands nearest the target", for two
/// reasons that agree. It is the [`crate::bar_groups`] cache's rebuild trigger:
/// a multiple free to change on every frame of a zoom gesture would re-fold the
/// whole series on every frame of it, where a ladder re-folds once per step.
/// And it is what a trader reads — a candle standing for 200 bars is a number
/// they can hold, 187 is noise. Steps stay near a quarter apart, so the slot
/// never drifts far past the target between them.
const SLOT_SNAP: [usize; 30] = [
    1, 2, 3, 4, 5, 6, 8, 10, 12, 16, 20, 25, 30, 40, 50, 60, 80, 100, 120, 160, 200, 250, 300, 400,
    500, 600, 800, 1000, 1200, 1600,
];
/// Widest a candle slot can be, in pixels (max zoom-in).
///
/// Sized for the footprint's Detailed level: a `sell × buy` ladder cell is
/// only a number from ~72 px of candle, so the old 64 px ceiling kept the
/// most detailed view of the tape permanently out of reach. 160 px shows a
/// handful of bars with full ladders — the "read these five candles" zoom.
pub const MAX_CANDLE_WIDTH: f32 = 160.0;
/// Most of the window the projection margin may take, as a fraction.
///
/// Four fifths, so a fifth of the screen always still holds candles. Leaving
/// only the newest bar was the obvious reading of "push it all the way left",
/// and it makes the view useless for the thing it exists for: the price axis
/// auto-fits what is visible, so a window holding one bar collapses to that
/// bar's range, and a channel projected into the empty space is drawn against
/// a scale that says nothing about the move it came from. A fifth of a screen
/// of candles keeps the scale meaningful and still clears four fifths to draw
/// into — more room than the old fixed margin gave at any zoom.
const MAX_PROJECTION_FRACTION: f32 = 0.8;
/// Smallest projection margin, in bar slots, when there is no usable window
/// width to derive one from (mid-layout, or a pane a few pixels wide). Small
/// enough to mean nothing on a real chart, large enough that the gesture is
/// never dead.
const MIN_PROJECTION_BARS: f32 = 8.0;

/// The visible window over a bar series.
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    /// Pixels per **bar** — the zoom. Below [`TARGET_SLOT_PX`] several bars
    /// share one drawn slot; see [`Viewport::bars_per_slot`].
    px_per_bar: f32,
    /// How far the zoom may go out, in pixels per bar, given how much series
    /// there is to show. Refreshed every frame by [`Viewport::clamp_to_window`],
    /// which is the only place that knows both the window and the bar count;
    /// see [`MIN_SERIES_FILL`].
    data_floor: f32,
    /// Fractional bar index at the right edge (used when not following). May
    /// exceed `total - 1` to show empty space past the newest bar.
    right_bar: f32,
    /// Whether the right edge is pinned to the newest bar.
    follow: bool,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            px_per_bar: DEFAULT_PX_PER_BAR,
            data_floor: MIN_PX_PER_BAR,
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

    /// Pixels per bar — how far apart two neighbouring bars sit.
    ///
    /// This is the zoom, and the number every pixel↔bar conversion uses. It is
    /// [`Self::candle_width`] only while one bar owns one slot; zoomed out past
    /// that, a bar is a fraction of the candle drawn over it.
    #[must_use]
    pub fn px_per_bar(&self) -> f32 {
        self.px_per_bar
    }

    /// How many bars are merged into one drawn candle.
    ///
    /// **One, at every zoom the chart could already reach** — see
    /// [`UNGROUPED_MIN_PX_PER_BAR`]. Below that line, where a bar used to be
    /// too narrow to draw at all, it is the whole number of bars that brings a
    /// slot back to [`TARGET_SLOT_PX`]: squeezing further then shows *more
    /// history at the same legibility* instead of a solid band.
    ///
    /// The multiple is the first entry of [`SLOT_SNAP`] wide enough — a
    /// ladder, not a free number, for the reasons written there. Bars group
    /// from index 0 up, so a group is the same group from frame to frame
    /// however far the view pans, and prepended history shifts every index by
    /// a count the cache already watches for.
    #[must_use]
    pub fn bars_per_slot(&self) -> usize {
        if !self.px_per_bar.is_finite()
            || self.px_per_bar <= 0.0
            || self.px_per_bar >= UNGROUPED_MIN_PX_PER_BAR
        {
            return 1;
        }
        SLOT_SNAP
            .into_iter()
            .find(|multiple| *multiple as f32 * self.px_per_bar >= TARGET_SLOT_PX)
            .unwrap_or(SLOT_SNAP[SLOT_SNAP.len() - 1])
    }

    /// Whether several bars share one drawn candle at this zoom.
    #[must_use]
    pub fn grouped(&self) -> bool {
        self.bars_per_slot() > 1
    }

    /// Width of one drawn candle slot, in pixels.
    #[must_use]
    pub fn candle_width(&self) -> f32 {
        self.px_per_bar * self.bars_per_slot() as f32
    }

    /// Whether the right edge is pinned to the newest bar.
    #[must_use]
    pub fn follows_live(&self) -> bool {
        self.follow
    }

    /// Zoom by a multiplicative factor: `> 1` widens the candles (zoom in),
    /// `< 1` narrows them (zoom out). Anchored to the right edge — the newest
    /// bar stays put.
    pub fn zoom(&mut self, factor: f32) {
        if factor > 0.0 && factor.is_finite() {
            self.px_per_bar = (self.px_per_bar * factor).clamp(self.floor(), MAX_CANDLE_WIDTH);
        }
    }

    /// How far out this view may zoom right now, in pixels per bar.
    ///
    /// The hard floor, unless the series is short enough that showing all of
    /// it takes wider bars — then it is whatever keeps [`MIN_SERIES_FILL`] of
    /// the window covered. Never above [`DEFAULT_PX_PER_BAR`]: however few
    /// bars there are, a chart still zooms out to its ordinary candles.
    #[must_use]
    fn floor(&self) -> f32 {
        self.data_floor.clamp(MIN_PX_PER_BAR, DEFAULT_PX_PER_BAR)
    }

    /// Set the zoom directly to `px` per bar, clamped to the same bounds the
    /// gesture obeys. This is the scripted entry (`QUANTICK_CANDLE_WIDTH`) to
    /// the zoom the scroll gesture reaches — one clamp, two doors.
    pub fn set_px_per_bar(&mut self, px: f32) {
        if px.is_finite() && px > 0.0 {
            self.px_per_bar = px.clamp(self.floor(), MAX_CANDLE_WIDTH);
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
    ///
    /// The past end stops at the oldest bar loaded; the future end is left
    /// open here and bounded once per frame by [`Self::clamp_to_window`],
    /// which is the only place that knows how wide the window is.
    pub fn pan_pixels(&mut self, dx: f32, total: usize) {
        if total == 0 || self.px_per_bar <= 0.0 || dx == 0.0 {
            return;
        }
        let newest = (total - 1) as f32;
        let current = self.right_edge_bar(total);
        let next = current - dx / self.px_per_bar;
        self.right_bar = next.max(0.0);
        // Follow only when the right edge is essentially *at* the newest bar.
        // Panning into the empty future keeps that margin instead of snapping
        // back to live.
        self.follow = (self.right_bar - newest).abs() <= 0.5;
    }

    /// How far past the newest bar the right edge may sit, in bar slots, for a
    /// candle area `window_px` wide.
    ///
    /// The margin *is* the window, less the bars that stay on screen: pushed
    /// all the way left, the newest bar sits at the left edge and the rest of
    /// the window is empty canvas — a full screen to project a channel or a
    /// Fibonacci extension into, which is what the gesture is for.
    ///
    /// A fixed slot count cannot do that job, and the old one (40 slots) is
    /// why the margin read as short: it is two thirds of a screen at 2 px per
    /// candle and a fifth of one at 8 px, so the same drag bought a different
    /// amount of room at every zoom, and least of all where a trader is most
    /// likely to be projecting.
    #[must_use]
    pub fn projection_margin_bars(&self, window_px: f32) -> f32 {
        if !window_px.is_finite() || window_px <= 0.0 || self.px_per_bar <= 0.0 {
            return MIN_PROJECTION_BARS;
        }
        (window_px * MAX_PROJECTION_FRACTION / self.px_per_bar).max(MIN_PROJECTION_BARS)
    }

    /// Hold the view inside what a `window_px`-wide candle area may show.
    ///
    /// Called once per frame after the gestures, because two of them leave the
    /// view outside its bounds by construction: [`Self::pan_pixels`] lets the
    /// future end run free, and [`Self::zoom`] does not know the window at all
    /// — zooming in while pushed fully left would otherwise multiply the empty
    /// space and walk the candles off the screen.
    pub fn clamp_to_window(&mut self, window_px: f32, total: usize) {
        // How far out this series is worth zooming, refreshed here because
        // this is the one call that sees both the window and the bar count.
        // It only ever *lowers* the floor (see `Self::floor`), so a chart with
        // three bars on it is not pinned to giant candles.
        if window_px.is_finite() && window_px > 0.0 && total > 0 {
            self.data_floor = window_px * MIN_SERIES_FILL / total as f32;
            // The zoom may already be past it — the series just grew shorter,
            // or the window just grew wider. Bring it back to what there is.
            self.px_per_bar = self.px_per_bar.max(self.floor());
        }
        if self.follow || total == 0 {
            return;
        }
        let newest = (total - 1) as f32;
        let max_right = newest + self.projection_margin_bars(window_px);
        self.right_bar = self.right_bar.clamp(0.0, max_right);
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
        if total == 0 || self.px_per_bar <= 0.0 {
            return;
        }
        let newest = (total - 1) as f32;
        let half_window = 0.5 * width_px / self.px_per_bar;
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
    /// Integer positions are bar centres, and half a slot of clearance keeps
    /// the bar at the right edge fully on screen. Order-book timestamps use
    /// fractional positions so several depth updates can be shown inside one
    /// activity-sampled bar without changing candle spacing.
    ///
    /// Grouped, neighbouring bars land a fraction of a pixel apart — the
    /// projection stays continuous in *bars*, and it is only what is drawn
    /// (candles, one per slot) that steps.
    #[must_use]
    pub fn x_at_bar_position(&self, bar_position: f32, chart_right: f32, total: usize) -> f32 {
        let right_bar = self.right_edge_bar(total);
        chart_right - (right_bar - bar_position) * self.px_per_bar - 0.5 * self.candle_width()
    }

    /// The fractional bar index under `x` pixels — the inverse of
    /// [`Self::x_at_bar_position`], and the only one.
    ///
    /// Placing a drawing and drawing it are the same projection read in
    /// opposite directions; written out twice they would disagree the first
    /// time the projection changed, and grouping is exactly such a change.
    #[must_use]
    pub fn bar_at_x(&self, x: f32, chart_right: f32, total: usize) -> f32 {
        if self.px_per_bar <= 0.0 {
            return self.right_edge_bar(total);
        }
        self.right_edge_bar(total) - (chart_right - x - 0.5 * self.candle_width()) / self.px_per_bar
    }

    /// The x-pixel centre of the slot holding bar `index` — where the candle
    /// drawn for that bar actually sits.
    ///
    /// The same point as [`Self::x_center`] while one bar owns one slot. Once
    /// grouped, it is the centre of the whole group, so a candle and anything
    /// anchored to a bar inside it agree about where that bar is on screen.
    #[must_use]
    pub fn slot_center_x(&self, index: usize, chart_right: f32, total: usize) -> f32 {
        let per_slot = self.bars_per_slot();
        let first = (index / per_slot * per_slot) as f32;
        let middle = first + (per_slot as f32 - 1.0) / 2.0;
        self.x_at_bar_position(middle, chart_right, total)
    }

    /// The `[start, end)` bar indices at least partly visible in a chart `width`
    /// pixels wide over `total` bars. Generous by up to a bar at each edge (the
    /// caller clips), so nothing pops in late.
    ///
    /// Grouped, the start is pulled back to the first bar of its slot: a slot
    /// is drawn from every bar in it, and starting mid-group would draw a
    /// candle from part of one.
    #[must_use]
    pub fn visible_range(&self, width: f32, total: usize) -> (usize, usize) {
        if total == 0 || self.px_per_bar <= 0.0 {
            return (0, 0);
        }
        let right_bar = self.right_edge_bar(total);
        let bars_across = width / self.px_per_bar;
        let start = (right_bar - bars_across).floor().max(0.0) as usize;
        let per_slot = self.bars_per_slot();
        let start = start / per_slot * per_slot;
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

    /// The scripted width obeys the gesture's own clamp, and the ceiling it
    /// reaches is the footprint's Detailed budget — the reason it rose to 160.
    #[test]
    #[allow(clippy::assertions_on_constants)] // the ceiling itself is the claim
    fn scripted_candle_width_shares_the_gestures_clamp() {
        let mut v = Viewport::new();
        v.set_px_per_bar(1000.0);
        assert!((v.px_per_bar() - MAX_CANDLE_WIDTH).abs() < 0.001);
        v.set_px_per_bar(0.01);
        assert!((v.px_per_bar() - MIN_PX_PER_BAR).abs() < 0.001);
        v.set_px_per_bar(f32::NAN);
        assert!((v.px_per_bar() - MIN_PX_PER_BAR).abs() < 0.001);
        assert!(MAX_CANDLE_WIDTH >= 160.0);
    }

    #[test]
    fn zoom_clamps_candle_width() {
        let mut v = Viewport::new();
        for _ in 0..100 {
            v.zoom(2.0);
        }
        assert!((v.px_per_bar() - MAX_CANDLE_WIDTH).abs() < 0.001);
        for _ in 0..100 {
            v.zoom(0.5);
        }
        assert!((v.px_per_bar() - MIN_PX_PER_BAR).abs() < 0.001);
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

    /// Pushed as far left as it goes, four fifths of the window is empty canvas
    /// to project a channel or a Fibonacci extension into — and the last fifth
    /// still holds candles, so the price axis is still scaled to the move being
    /// projected from.
    #[test]
    fn pushing_left_clears_most_of_the_window_and_keeps_the_rest_readable() {
        let mut v = Viewport::new(); // 8 px per candle
        let window = 800.0; // 100 bars across
        v.pan_pixels(-10_000.0, 100);
        v.clamp_to_window(window, 100);
        assert!(!v.follows_live());
        let edge = v.right_edge_bar(100);
        assert!((edge - (99.0 + 80.0)).abs() < 0.001, "edge = {edge}");
        // The same statement in pixels: the newest bar sits a fifth of the way
        // in from the left, and everything right of it is empty.
        let x = v.x_center(99, window, 100);
        assert!(
            (x - window * 0.2).abs() < 8.0,
            "newest bar a fifth in: {x} of {window}"
        );
        // Which means bars are still on screen to scale the price axis by.
        let (start, end) = v.visible_range(window, 100);
        assert!(end - start >= 15, "candles left in view: {}", end - start);
    }

    /// The margin is derived from the window, so it is worth the same *screen*
    /// at every zoom. The fixed 40-slot margin it replaces was worth two thirds
    /// of a screen zoomed out and a fifth of one zoomed in.
    #[test]
    fn the_projection_margin_scales_with_the_window() {
        let mut v = Viewport::new();
        let zoomed_out = v.projection_margin_bars(1600.0);
        v.set_px_per_bar(32.0);
        let zoomed_in = v.projection_margin_bars(1600.0);
        // Both come to the same four fifths of a screen of empty canvas.
        assert!((zoomed_out * 8.0 - 1600.0 * 0.8).abs() < 0.001);
        assert!((zoomed_in * 32.0 - 1600.0 * 0.8).abs() < 0.001);
    }

    /// The regression the per-frame clamp exists for: the same margin in *bars*
    /// is a wider margin in pixels once the candles grow, so zooming in while
    /// pushed fully left used to walk the series off the left of the screen.
    #[test]
    fn zooming_in_while_pushed_left_keeps_the_candles_on_screen() {
        let mut v = Viewport::new();
        v.pan_pixels(-10_000.0, 10);
        v.clamp_to_window(800.0, 10);
        v.zoom(4.0); // 32 px per candle: 25 bars across, not 100
        v.clamp_to_window(800.0, 10);
        let edge = v.right_edge_bar(10);
        assert!((edge - (9.0 + 20.0)).abs() < 0.001, "edge = {edge}");
        let x = v.x_center(9, 800.0, 10);
        assert!(
            (0.0..=800.0).contains(&x),
            "newest bar still on screen: {x}"
        );
    }

    #[test]
    fn clamping_leaves_a_following_view_alone() {
        let mut v = Viewport::new();
        v.clamp_to_window(800.0, 10);
        assert!(v.follows_live());
        assert_eq!(v.right_edge_bar(10), 9.0);
    }

    /// The promise grouping is bounded by: every zoom the chart could already
    /// reach draws the bars the trader configured, one candle each. A trader
    /// entering on a single print-storm bar — the elephant a 5 000-tick
    /// imbalance rule cut — must never watch it fold into its neighbours
    /// because they moved the zoom.
    #[test]
    fn nothing_groups_at_any_zoom_the_chart_could_already_reach() {
        let mut v = Viewport::new();
        for px in [
            MAX_CANDLE_WIDTH,
            64.0,
            DEFAULT_PX_PER_BAR,
            4.0,
            3.0,
            UNGROUPED_MIN_PX_PER_BAR,
        ] {
            v.set_px_per_bar(px);
            assert_eq!(v.bars_per_slot(), 1, "{px} px per bar must stay ungrouped");
            assert!(!v.grouped());
            assert!((v.candle_width() - px).abs() < 0.001);
        }
    }

    /// Below that line — territory the old floor made unreachable — squeezing
    /// groups instead of shrinking, which is what keeps the extra history
    /// readable rather than a solid band.
    #[test]
    fn squeezing_past_a_drawable_candle_groups_bars_instead() {
        let mut v = Viewport::new();
        v.set_px_per_bar(UNGROUPED_MIN_PX_PER_BAR - 0.1);
        assert!(
            v.grouped(),
            "just past the line, bars start sharing candles"
        );
        assert!(
            v.candle_width() >= TARGET_SLOT_PX - 0.001,
            "and the candle drawn for them is drawable: {}",
            v.candle_width()
        );

        v.set_px_per_bar(MIN_PX_PER_BAR); // as far out as it goes
        assert_eq!(v.bars_per_slot(), 400);
        assert!(
            (v.candle_width() - TARGET_SLOT_PX).abs() < 0.001,
            "still a drawable slot: {}",
            v.candle_width()
        );
    }

    /// Whatever the zoom, a candle is drawn from a whole number of bars off the
    /// ladder and is never narrower than the target. The first claim is what
    /// keeps the grouping cache from rebuilding every frame of a gesture; the
    /// second is what keeps the chart readable while it does.
    #[test]
    fn every_zoom_lands_on_the_ladder_with_a_drawable_slot() {
        let mut v = Viewport::new();
        let mut steps = 0;
        let mut previous = v.bars_per_slot();
        // Walk the whole zoom range in fine increments, as a gesture does.
        let mut px = MAX_CANDLE_WIDTH;
        while px > MIN_PX_PER_BAR {
            v.set_px_per_bar(px);
            let per_slot = v.bars_per_slot();
            assert!(
                SLOT_SNAP.contains(&per_slot),
                "{per_slot} bars a candle is not on the ladder"
            );
            assert!(
                per_slot == 1 || v.candle_width() >= TARGET_SLOT_PX - 0.001,
                "slot {} px at {px} px per bar",
                v.candle_width()
            );
            assert!(per_slot >= previous, "the multiple only grows on zoom-out");
            if per_slot != previous {
                steps += 1;
                previous = per_slot;
            }
            px *= 0.99;
        }
        // A whole zoom-out is a couple of dozen rebuilds, not the thousand
        // frames it takes to perform.
        assert!(steps <= SLOT_SNAP.len(), "{steps} multiples used");
    }

    /// What the floor buys, stated as the trader feels it: ten times the
    /// history on the same screen.
    #[test]
    fn the_zoom_out_floor_holds_ten_times_the_bars_the_old_one_did() {
        let mut v = Viewport::new();
        v.set_px_per_bar(MIN_PX_PER_BAR);
        let (start, end) = v.visible_range(1600.0, 100_000);
        let bars = (end - start) as f32;
        // The old floor was 2 px per bar: 800 bars across a 1600 px chart.
        assert!(bars >= 10.0 * (1600.0 / 2.0), "bars across = {bars}");
    }

    /// A candle is drawn from a whole group, so the window starts on a group
    /// boundary — starting mid-group would draw a candle from part of one.
    #[test]
    fn a_grouped_window_starts_on_a_group_boundary() {
        let mut v = Viewport::new();
        v.set_px_per_bar(0.5);
        assert_eq!(v.bars_per_slot(), 8);
        let (start, _) = v.visible_range(800.0, 100_000);
        assert_eq!(start % 8, 0, "start = {start}");
    }

    /// Grouped, the candle for a group is drawn at the middle of it and every
    /// bar in the group reports that same slot — which is what keeps a drawing
    /// anchored to a bar sitting on the candle that stands for it.
    #[test]
    fn every_bar_of_a_group_shares_the_slot_its_candle_is_drawn_at() {
        let mut v = Viewport::new();
        v.set_px_per_bar(1.0);
        assert_eq!(v.bars_per_slot(), 4);
        let right = 1000.0;
        let slot = v.slot_center_x(40, right, 100);
        for bar in 40..44 {
            assert!(
                (v.slot_center_x(bar, right, 100) - slot).abs() < 0.001,
                "bar {bar} belongs to the same candle"
            );
            // And its own position never leaves the candle drawn over it.
            let x = v.x_center(bar, right, 100);
            assert!(
                (x - slot).abs() <= v.candle_width() / 2.0 + 0.001,
                "bar {bar}"
            );
        }
        let next = v.slot_center_x(44, right, 100);
        assert!(
            (next - slot - v.candle_width()).abs() < 0.001,
            "neighbouring candles are one slot apart"
        );
    }

    /// Placing a drawing and drawing it are one projection read in both
    /// directions — including grouped, where the two easily drift apart.
    #[test]
    fn x_and_bar_are_inverses_at_every_zoom() {
        for px in [8.0_f32, 2.0, 0.5, MIN_PX_PER_BAR] {
            let mut v = Viewport::new();
            v.set_px_per_bar(px);
            for bar in [0.0_f32, 12.5, 99.0] {
                let x = v.x_at_bar_position(bar, 1000.0, 200);
                let back = v.bar_at_x(x, 1000.0, 200);
                assert!((back - bar).abs() < 0.01, "px {px}, bar {bar}: got {back}");
            }
        }
    }

    /// Ungrouped — every zoom a trader spends their day at — nothing about the
    /// projection moved: one bar, one slot, the same pixels as before.
    #[test]
    fn an_ungrouped_projection_is_unchanged() {
        let v = Viewport::new();
        assert_eq!(v.bars_per_slot(), 1);
        assert!((v.candle_width() - v.px_per_bar()).abs() < f32::EPSILON);
        for bar in [0_usize, 5, 9] {
            assert!((v.slot_center_x(bar, 1000.0, 10) - v.x_center(bar, 1000.0, 10)).abs() < 0.001);
        }
    }

    /// A window with no usable width is mid-layout, not a decision to take the
    /// gesture away: the margin falls back to a floor rather than to zero.
    #[test]
    fn a_window_with_no_width_still_leaves_room_to_pan_into() {
        for window in [0.0, -50.0, f32::NAN, f32::INFINITY] {
            let mut v = Viewport::new();
            v.pan_pixels(-10_000.0, 10);
            v.clamp_to_window(window, 10);
            let edge = v.right_edge_bar(10);
            assert!(
                (edge - (9.0 + MIN_PROJECTION_BARS)).abs() < 0.001,
                "window {window}: edge = {edge}"
            );
        }
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
