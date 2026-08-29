//! The scrollable/zoomable viewport over the bar series (TradingView-style).
//!
//! Bars have a **fixed pixel width** (the zoom); the newest bar sits near the
//! right edge, and the view is a window that can pan freely — through history and
//! into empty space past the newest bar — so dragging always moves the chart,
//! even when there are only a handful of bars. This is the pure state behind
//! that (pixels per bar + the fractional bar index at the right edge + a follow
//! flag), unit-tested in CI with no egui or input handling.
//!
//! One thing it owns that a plain window would not: **room to project into.**
//! How far past the newest bar the view may go is derived from the window
//! ([`Viewport::projection_margin_bars`]), so pushing the chart left always
//! clears about a screen of empty canvas — enough for a channel or a
//! Fibonacci extension drawn out in front of the market.
//!
//! One law it enforces by construction: **one bar, one candle, at every
//! zoom.** A trader who entered on a single bar of their rule — the elephant
//! bar an imbalance rule cut — must be able to trust that every candle on
//! screen *is* one bar of that rule. There is no level of detail here, no
//! aggregation, nothing that changes what a candle means when the zoom moves.
//!
//! It owns the candles' pane only. The live lane is a band of screen beside it
//! with a clock of its own, so nothing here reaches the tape.

/// Narrowest a bar can be drawn, in pixels — the zoom-out floor.
///
/// One pixel is the honest floor: each bar still owns its own pixel column,
/// so every mark on screen is attributable to exactly one bar of the
/// configured rule. Below it neighbouring bars would share pixels and the
/// chart would stop being a drawing of the bars — squeezing may make bars
/// thin, never make them lie. (The old floor was 2 px; this doubles the
/// history a window holds, with nothing merged to pay for it.)
pub const MIN_PX_PER_BAR: f32 = 1.0;
/// Pixels per bar a chart opens on.
pub const DEFAULT_PX_PER_BAR: f32 = 8.0;
/// Widest a candle slot can be, in pixels (max zoom-in).
///
/// Sized for the footprint's widest style at the latest the trader can ask
/// detail to arrive. The ceiling has moved twice for the same reason: 64 px
/// kept the `sell × buy` ladder permanently out of reach, and 160 did the same
/// to the three-column cluster once `detail_scale` was turned up — the style
/// existed, the registry offered it, and no zoom could draw it.
///
/// The number is that promise, stated: the widest floor any style declares,
/// multiplied by the top of the detail-scale range, rounded up. It is guarded
/// by `every_style_is_reachable_at_some_zoom_and_every_detail_scale`, so a
/// style added with a wider floor fails loudly here rather than becoming an
/// entry nothing can select into view. At this width a 1900 px pane holds
/// seven bars — the "read these few candles" zoom the cluster is for.
pub const MAX_CANDLE_WIDTH: f32 = 256.0;
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
    /// Pixels per **bar** — the zoom.
    px_per_bar: f32,
    /// `Some(index)` when the trader has taken the right edge somewhere;
    /// `None` pins it to the newest bar and lets new bars carry it along.
    ///
    /// A latch, not a threshold — the same shape [`crate::price_view::PriceView`]
    /// uses for the vertical axis, and for the same reason. Any real drag takes
    /// it, so there is no distance below which a gesture is discarded: with
    /// `follow` re-derived from "is the edge near enough to the newest bar"
    /// instead, every per-frame delta smaller than that distance was thrown
    /// away on the next read, and a slow drag moved nothing at all. Whichever
    /// unit that distance was written in, some zoom made it wide enough to eat
    /// a whole gesture.
    ///
    /// The value may exceed `total - 1`: past the newest bar is empty canvas
    /// to project into, and a deliberate place to be.
    right_bar: Option<f32>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            px_per_bar: DEFAULT_PX_PER_BAR,
            right_bar: None,
        }
    }
}

impl Viewport {
    /// A viewport following the live edge at the default zoom.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pixels per bar — how far apart two neighbouring bars sit. This is the
    /// zoom, and the number every pixel↔bar conversion uses.
    #[must_use]
    pub fn px_per_bar(&self) -> f32 {
        self.px_per_bar
    }

    /// Width of one drawn candle slot, in pixels — always one bar's width.
    ///
    /// One bar, one candle, is a law of this chart, not a level of detail:
    /// a candle that could stand for several bars would make every candle
    /// untrustworthy at some zoom, and a trader enters on *one* bar.
    #[must_use]
    pub fn candle_width(&self) -> f32 {
        self.px_per_bar
    }

    /// Whether the right edge is pinned to the newest bar.
    #[must_use]
    pub fn follows_live(&self) -> bool {
        self.right_bar.is_none()
    }

    /// Zoom by a multiplicative factor: `> 1` widens the candles (zoom in),
    /// `< 1` narrows them (zoom out). Anchored to the right edge — the newest
    /// bar stays put.
    pub fn zoom(&mut self, factor: f32) {
        if factor > 0.0 && factor.is_finite() {
            self.px_per_bar = (self.px_per_bar * factor).clamp(MIN_PX_PER_BAR, MAX_CANDLE_WIDTH);
        }
    }

    /// Set the zoom directly to `px` per bar, clamped to the same bounds the
    /// gesture obeys. This is the scripted entry (`QUANTICK_CANDLE_WIDTH`) to
    /// the zoom the scroll gesture reaches — one clamp, two doors.
    pub fn set_px_per_bar(&mut self, px: f32) {
        if px.is_finite() && px > 0.0 {
            self.px_per_bar = px.clamp(MIN_PX_PER_BAR, MAX_CANDLE_WIDTH);
        }
    }

    /// The bar index at the right edge for a series of `total` bars.
    #[must_use]
    pub fn right_edge_bar(&self, total: usize) -> f32 {
        self.right_bar
            .unwrap_or_else(|| total.saturating_sub(1) as f32)
    }

    /// Pan by `dx` pixels (a drag delta). Positive `dx` (drag right) reveals
    /// older bars — the right edge moves into the past; negative moves toward
    /// the present. Coming back *to* the newest bar resumes following.
    ///
    /// The past end stops at the oldest bar loaded; the future end is left
    /// open here and bounded once per frame by [`Self::clamp_to_window`],
    /// which is the only place that knows how wide the window is.
    ///
    /// **Every non-zero drag counts.** A drag does not arrive as one delta —
    /// it arrives as a stream of per-frame deltas of a few pixels each — so a
    /// rule that discards deltas under some distance discards the whole
    /// gesture, and any distance is too wide at some zoom. That is why the
    /// edge is a latch rather than a boolean re-derived from how near the
    /// newest bar it landed.
    pub fn pan_pixels(&mut self, dx: f32, total: usize) {
        if total == 0 || self.px_per_bar <= 0.0 || dx == 0.0 || !dx.is_finite() {
            return;
        }
        let newest = (total - 1) as f32;
        let current = self.right_edge_bar(total);
        let next = (current - dx / self.px_per_bar).max(0.0);
        // Walking back to the live edge from the past hands control back;
        // pushing *past* it is the projection gesture, and keeps its margin.
        self.right_bar = (dx >= 0.0 || current >= newest || next < newest).then_some(next);
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
        let (Some(right_bar), true) = (self.right_bar, total > 0) else {
            return;
        };
        let newest = (total - 1) as f32;
        let max_right = newest + self.projection_margin_bars(window_px);
        // Clamping never hands control back: a view pushed into the margin and
        // then squeezed by a narrower window is still a view the trader placed.
        self.right_bar = Some(right_bar.clamp(0.0, max_right));
    }

    /// Pin the right edge back to the newest bar.
    pub fn snap_to_live(&mut self) {
        self.right_bar = None;
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
        let placed = (bar + half_window).clamp(0.0, newest);
        // Centring on something at or past the live edge pins the edge to the
        // newest bar, and that *is* following — anything short of it is a
        // place the trader asked to look at, whatever the zoom makes of the
        // remaining distance.
        self.right_bar = (placed < newest).then_some(placed);
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
        let (Some(right_bar), true) = (self.right_bar, delta != 0) else {
            return;
        };
        self.right_bar = Some((right_bar + delta as f32).max(0.0));
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
        if self.right_bar.is_none() {
            return;
        }
        match bar {
            Some(index) if total > 0 => {
                let newest = (total - 1) as f32;
                let anchored = (index as f32).clamp(0.0, newest);
                // The anchor is an exact bar index, so "is it the newest one"
                // is an exact question. Answering it with a distance made a
                // one-bar-old anchor read as live at low zoom, and the view
                // scrolled away from where the trader had parked it.
                self.right_bar = (anchored < newest).then_some(anchored);
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
    /// time the projection changed, and a drift here puts an anchor on the
    /// wrong bar.
    #[must_use]
    pub fn bar_at_x(&self, x: f32, chart_right: f32, total: usize) -> f32 {
        if self.px_per_bar <= 0.0 {
            return self.right_edge_bar(total);
        }
        self.right_edge_bar(total) - (chart_right - x - 0.5 * self.candle_width()) / self.px_per_bar
    }

    /// The bar slot a fractional bar coordinate names: the half-open interval
    /// `(i - 0.5, i + 0.5]` belongs to bar `i`.
    ///
    /// Two producers write these coordinates and they do not use the same
    /// convention, which is why the rule is stated as an interval rather than
    /// as "round" or "truncate":
    ///
    /// * [`Self::bar_at_x`] answers in bar *centres* — bar `i` reads exactly
    ///   `i` at its centre and `i - 0.5` at its left edge. Truncating that
    ///   names the bar to the *left* for the whole left half of every candle,
    ///   which is half the pixels of every candle on the chart.
    /// * `ChartPane::slot_of_time` writes a time-derived anchor at the slot's
    ///   far *edge*, `i + 0.5`, deliberately. Rounding that to nearest sends
    ///   it to `i + 1` — every re-anchored drawing a bar late, silently.
    ///
    /// Closing the interval at the top is what serves both: `i` reads `i`, and
    /// so does `i + 0.5`. Written as `(bar - 0.5).ceil()` because that is the
    /// arithmetic that says it, and `round` is the one that does not.
    ///
    /// `None` for a coordinate that is not a slot at all: not finite, or left
    /// of the first bar. Unbounded on the right, because the empty space past
    /// the newest bar is a real place a trader draws in — the caller decides
    /// what a slot out there means (see `ChartPane::anchor_time`).
    #[must_use]
    pub fn slot_of(bar: f32) -> Option<usize> {
        if !bar.is_finite() {
            return None;
        }
        let slot = (bar - 0.5).ceil();
        (slot >= 0.0).then_some(slot as usize)
    }

    /// The bar under `x` pixels, bounded by a series of `total` bars.
    ///
    /// The one question "which candle am I pointing at?" — asked by the axis
    /// compass, by the control plane's cursor scope and by every hit test that
    /// needs a whole bar rather than a fraction of one.
    #[must_use]
    pub fn slot_at_x(&self, x: f32, chart_right: f32, total: usize) -> Option<usize> {
        Self::slot_of(self.bar_at_x(x, chart_right, total)).filter(|slot| *slot < total)
    }

    /// The `[start, end)` bar indices at least partly visible in a chart `width`
    /// pixels wide over `total` bars. Generous by up to a bar at each edge (the
    /// caller clips), so nothing pops in late.
    #[must_use]
    pub fn visible_range(&self, width: f32, total: usize) -> (usize, usize) {
        if total == 0 || self.px_per_bar <= 0.0 {
            return (0, 0);
        }
        let right_bar = self.right_edge_bar(total);
        let bars_across = width / self.px_per_bar;
        let start = (right_bar - bars_across).floor().max(0.0) as usize;
        let end = (right_bar.floor() as i64 + 2).clamp(0, total as i64) as usize;
        let start = start.min(end);
        (start, end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole reason [`Viewport::slot_of`] rounds.
    ///
    /// A candle occupies its slot centred on [`Viewport::x_center`], so the
    /// left half of every candle reads as `i - something` and truncating names
    /// the candle to its *left*. Half the pixels of every candle on the chart
    /// answered with the wrong bar — which the axis compass would have
    /// inherited, and the candle magnet already had.
    #[test]
    fn every_pixel_of_a_candle_names_that_candle() {
        let mut viewport = Viewport::new();
        viewport.set_px_per_bar(20.0);
        let (right, total) = (1000.0_f32, 300_usize);
        let width = viewport.candle_width();
        for slot in [10_usize, 150, 299] {
            let centre = viewport.x_center(slot, right, total);
            for (where_on_the_candle, x) in [
                ("left edge", centre - width / 2.0 + 0.01),
                ("centre", centre),
                ("right edge", centre + width / 2.0 - 0.01),
            ] {
                assert_eq!(
                    viewport.slot_at_x(x, right, total),
                    Some(slot),
                    "the {where_on_the_candle} of candle {slot} is candle {slot}"
                );
            }
        }
    }

    /// Off either end there is no bar, and the answer says so rather than
    /// clamping onto the nearest one: a compass that named the newest candle
    /// while the pointer sat in the empty margin past it would put a time on
    /// a place where nothing happened.
    #[test]
    fn there_is_no_slot_off_the_ends_of_the_tape() {
        let mut viewport = Viewport::new();
        viewport.set_px_per_bar(20.0);
        let (right, total) = (1000.0_f32, 300_usize);
        // Past the newest bar: the projection margin, where traders draw.
        assert_eq!(viewport.slot_at_x(right + 40.0, right, total), None);
        // And left of the oldest.
        let oldest = viewport.x_center(0, right, total);
        assert_eq!(viewport.slot_at_x(oldest - 40.0, right, total), None);
        assert_eq!(Viewport::slot_of(f32::NAN), None, "not a position at all");
        assert_eq!(Viewport::slot_of(-3.0), None, "before the first bar");
    }

    /// The other producer's convention, and the one a rounding rule silently
    /// breaks: `ChartPane::slot_of_time` writes a time-derived anchor at the
    /// slot's far edge, `i + 0.5`. Nearest-rounding sends it to `i + 1`, so
    /// every drawing re-anchored across a timeframe switch would resolve one
    /// bar late and claim the next candle's instant — with nothing on screen
    /// saying so.
    #[test]
    fn an_anchor_written_at_a_slots_far_edge_still_names_that_slot() {
        for slot in [0_usize, 1, 47, 1_000] {
            #[allow(clippy::cast_precision_loss)]
            let written = slot as f32 + 0.5;
            assert_eq!(
                Viewport::slot_of(written),
                Some(slot),
                "an anchor written at {written} is bar {slot}"
            );
        }
        // And the interval is half-open at the bottom, so the two rules meet
        // without overlapping: one boundary, one bar.
        assert_eq!(Viewport::slot_of(0.5), Some(0));
        assert_eq!(Viewport::slot_of(0.51), Some(1));
    }

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

    /// A drag arrives as a stream of small per-frame deltas, and every one of
    /// them has to count — at every zoom, and at every speed.
    ///
    /// This is the defect the latch exists for. While the right edge was a
    /// float plus a `follow` boolean re-derived from "did it land near the
    /// newest bar", every per-frame delta smaller than that distance was
    /// written and then answered over by the newest bar on the next read: the
    /// gesture could not accumulate and the chart sat welded to the live edge.
    /// Half a bar slot made it bite past ~30 px per candle — the zoom the
    /// footprint ladder is read at, which is why it surfaced when that layer
    /// landed — and a fixed pixel distance merely moved the dead zone to the
    /// slow drags and the zoomed-out end instead.
    ///
    /// So the probes are deliberately hostile: the slowest deliberate drag
    /// anyone performs (one pixel a frame) at both ends of the zoom range.
    #[test]
    fn a_drag_accumulates_at_every_zoom_and_every_speed() {
        for zoom in [MIN_PX_PER_BAR, DEFAULT_PX_PER_BAR, 68.0, MAX_CANDLE_WIDTH] {
            for per_frame in [1.0_f32, 1.5, 2.0, 10.0] {
                // Long enough that the drag never reaches the oldest bar: the
                // past end stops there by design, and a test that hits the
                // wall measures the wall instead of the gesture.
                const TOTAL: usize = 4_000;
                const FRAMES: usize = 30;
                let mut v = Viewport::new();
                v.set_px_per_bar(zoom);
                let newest = (TOTAL - 1) as f32;
                for _ in 0..FRAMES {
                    v.pan_pixels(per_frame, TOTAL);
                    // The real loop clamps between frames; a test that skips
                    // it cannot see a regression living in that interaction.
                    v.clamp_to_window(800.0, TOTAL);
                }
                let want = per_frame * FRAMES as f32;
                let moved = (newest - v.right_edge_bar(TOTAL)) * zoom;
                assert!(
                    !v.follows_live(),
                    "zoom {zoom}, {per_frame} px/frame: the drag left the view following"
                );
                assert!(
                    (moved - want).abs() < 0.5,
                    "zoom {zoom}, {per_frame} px/frame: wanted {want} px, moved {moved}"
                );
            }
        }
    }

    /// Coming back to the live edge hands control back; pushing past it into
    /// the projection margin does not, because that is a place a trader means
    /// to be.
    #[test]
    fn the_live_edge_is_reached_exactly_and_the_margin_is_kept() {
        for zoom in [MIN_PX_PER_BAR, DEFAULT_PX_PER_BAR, MAX_CANDLE_WIDTH] {
            const TOTAL: usize = 200;
            let mut v = Viewport::new();
            v.set_px_per_bar(zoom);
            // Out into the past, then back one pixel short of the edge.
            v.pan_pixels(100.0, TOTAL);
            assert!(
                !v.follows_live(),
                "zoom {zoom}: the drag did not leave live"
            );
            v.pan_pixels(-99.0, TOTAL);
            assert!(
                !v.follows_live(),
                "zoom {zoom}: a pixel short of the edge already counted as live"
            );
            // Closing that pixel hands control back, at any zoom.
            v.pan_pixels(-1.0, TOTAL);
            assert!(
                v.follows_live(),
                "zoom {zoom}: back at the edge, not following"
            );
            // And pushing on from there keeps the margin instead of snapping.
            v.pan_pixels(-50.0, TOTAL);
            assert!(
                !v.follows_live(),
                "zoom {zoom}: the projection margin snapped back to live"
            );
        }
    }

    /// Zoom must not discard where the trader put the edge.
    ///
    /// With `follow` derived from a pixel distance, a small pan at low zoom
    /// counted as live, and zooming in then threw a quarter-screen offset away
    /// in silence — the state was a conclusion about the zoom rather than a
    /// record of the gesture.
    #[test]
    fn the_edge_survives_a_zoom_change() {
        const TOTAL: usize = 200;
        let mut v = Viewport::new();
        v.set_px_per_bar(MIN_PX_PER_BAR);
        v.pan_pixels(2.0, TOTAL);
        let parked = v.right_edge_bar(TOTAL);
        assert!(!v.follows_live(), "a two-pixel drag was discarded");
        v.set_px_per_bar(MAX_CANDLE_WIDTH);
        assert!(!v.follows_live(), "zooming in resumed following on its own");
        assert!(
            (v.right_edge_bar(TOTAL) - parked).abs() < f32::EPSILON,
            "zooming moved the edge"
        );
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

    /// The law the x axis answers to: one bar, one candle, at every zoom.
    /// A trader enters on a single bar of their rule — a candle that could
    /// stand for several would make every candle untrustworthy somewhere.
    #[test]
    fn one_bar_is_one_candle_at_every_zoom() {
        let mut v = Viewport::new();
        let mut px = MAX_CANDLE_WIDTH;
        while px >= MIN_PX_PER_BAR {
            v.set_px_per_bar(px);
            assert!(
                (v.candle_width() - v.px_per_bar()).abs() < f32::EPSILON,
                "at {px} px per bar"
            );
            px *= 0.9;
        }
    }

    /// The zoom-out floor is one pixel per bar — each bar still owns its own
    /// pixel column, so the squeeze doubles what the old 2 px floor showed
    /// without a single bar merged to pay for it.
    #[test]
    fn the_squeeze_reaches_one_pixel_per_bar_and_stops() {
        let mut v = Viewport::new();
        for _ in 0..100 {
            v.zoom(0.9);
        }
        assert!((v.px_per_bar() - MIN_PX_PER_BAR).abs() < 0.001);
        let (start, end) = v.visible_range(1600.0, 100_000);
        assert!(end - start >= 1600, "a 1600 px window holds 1600 bars");
    }

    /// Placing a drawing and drawing it are one projection read in both
    /// directions, at every zoom the chart reaches.
    #[test]
    fn x_and_bar_are_inverses_at_every_zoom() {
        for px in [8.0_f32, 2.0, MIN_PX_PER_BAR] {
            let mut v = Viewport::new();
            v.set_px_per_bar(px);
            for bar in [0.0_f32, 12.5, 99.0] {
                let x = v.x_at_bar_position(bar, 1000.0, 200);
                let back = v.bar_at_x(x, 1000.0, 200);
                assert!((back - bar).abs() < 0.01, "px {px}, bar {bar}: got {back}");
            }
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
