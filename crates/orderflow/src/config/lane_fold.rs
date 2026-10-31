//! The flow pane's three views, and the divider drag that lands on them.
//!
//! The pane shows the candles and the tape side by side, the candles alone or
//! the tape alone. [`LaneView`] is not a third switch: it is the lane's
//! `enabled` and `tape_only` read together, so nothing can disagree with them.
//!
//! The divider between the candles and the tape drags across the whole chart.
//! Let go within [`LANE_FOLD_AT_PX`] of the left edge and the tape takes the
//! pane; of the right edge and the candles do. The share is left where the drag
//! found it, which is the band coming back to [`LaneView::Beside`] returns to.
//!
//! The threshold is the canvas pane fold's (the app's `canvas_layout` collapse
//! threshold asserts it agrees), so the two edges feel alike.

use super::HeatmapConfig;
use super::lane::{
    DEFAULT_LIVE_LANE_SHARE, LiveLaneStyle, MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE,
    MIN_LIVE_LANE_WIDTH_PX,
};

/// How close to an edge the divider has to be let go before that edge's side
/// leaves the pane rather than narrows: the canvas pane's collapse threshold.
pub const LANE_FOLD_AT_PX: f32 = 120.0;

/// What the flow pane shows: never stored, always [`LiveLaneStyle::view`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneView {
    /// Candles and tape side by side, split at the lane's share.
    Beside,
    /// The candles alone: the tape is switched off.
    CandlesOnly,
    /// The tape alone: no candle and no candle layer is drawn.
    TapeOnly,
}

impl LaneView {
    /// The view after this one on the pane's view button.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Beside => Self::CandlesOnly,
            Self::CandlesOnly => Self::TapeOnly,
            Self::TapeOnly => Self::Beside,
        }
    }

    /// The view the tape key leads to: tape only, and from there back beside
    /// the candles. Nothing remembers the view it left.
    #[must_use]
    pub fn tape_key(self) -> Self {
        if self == Self::TapeOnly {
            Self::Beside
        } else {
            Self::TapeOnly
        }
    }
}

/// The threshold on a chart this wide: a quarter of a chart too narrow for the
/// full one, so a small pane still has open widths to drag through.
fn fold_at_px(chart_width: f32) -> f32 {
    LANE_FOLD_AT_PX.min(chart_width / 4.0)
}

fn measurable(chart_width: f32) -> bool {
    chart_width.is_finite() && chart_width > 0.0
}

impl LiveLaneStyle {
    /// What the pane shows: the two switches read together. A tape switched
    /// off is candles only whatever `tape_only` says.
    #[must_use]
    pub fn view(&self) -> LaneView {
        match (self.enabled, self.tape_only) {
            (true, false) => LaneView::Beside,
            (true, true) => LaneView::TapeOnly,
            (false, _) => LaneView::CandlesOnly,
        }
    }

    /// Show `view`, the one write every view goes through; answers whether
    /// either switch moved, so a stale `tape_only` cleared here is saved. The
    /// share stays. Candles only clears `tape_only` too, so the tape's own
    /// switch brings back a tape beside the candles.
    pub fn set_view(&mut self, view: LaneView) -> bool {
        let before = (self.enabled, self.tape_only);
        self.enabled = view != LaneView::CandlesOnly;
        self.tape_only = view == LaneView::TapeOnly;
        before != (self.enabled, self.tape_only)
    }

    /// Whether the tape sits beside the candles, so a divider exists.
    #[must_use]
    pub fn beside_candles(&self) -> bool {
        self.view() == LaneView::Beside
    }

    /// Switch the tape on or off; answers whether it changed. A tape switched
    /// on lands beside the candles, whatever `tape_only` a preset or the layer
    /// setter left behind while it was off.
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        let changed = self.enabled != enabled;
        if changed && enabled {
            self.tape_only = false;
        }
        self.enabled = enabled;
        changed
    }

    /// The share the lane's band takes, clamped as the layout clamps it.
    #[must_use]
    pub(crate) fn open_share(&self) -> f32 {
        if self.width_share.is_finite() {
            self.width_share
                .clamp(MIN_LIVE_LANE_SHARE, MAX_LIVE_LANE_SHARE)
        } else {
            DEFAULT_LIVE_LANE_SHARE
        }
    }

    /// Where the divider takes the pointer, in pixels from the chart's left:
    /// `half_px` either side of it. `None` without a divider: the tape off or
    /// alone on the pane.
    #[must_use]
    pub fn divider_hit(&self, chart_width: f32, half_px: f32) -> Option<(f32, f32)> {
        let width = self.resolved_width_px(chart_width);
        let x = chart_width - width;
        (width > 0.0 && self.beside_candles()).then_some((x - half_px, x + half_px))
    }
}

/// The tape widths the divider can leave open on a chart this wide: from
/// the wider of the fold threshold and the share's floor (and the pixel
/// floor) up to the narrower of the threshold and the share's ceiling. Past
/// either end a release changes the view, so an open lane is always drawn where
/// the hand put it right up to where letting go would take a side away.
fn open_range_px(chart_width: f32) -> (f32, f32) {
    let at = fold_at_px(chart_width);
    let floor_px = MIN_LIVE_LANE_WIDTH_PX.min(chart_width / 2.0);
    let low = at.max(chart_width * MIN_LIVE_LANE_SHARE).max(floor_px);
    let high = (chart_width - at).min(chart_width * MAX_LIVE_LANE_SHARE);
    (low, high)
}

/// The tape widths one drag leaves open: [`open_range_px`], reaching down to
/// the share's floor or up to its ceiling when an open lane starts outside
/// it, so a lane kept narrower or wider than the range moves with the hand
/// from where it is rather than changing the view on the first pixel.
fn drag_range_px(lane: &LiveLaneStyle, chart_width: f32) -> (f32, f32) {
    let (low, high) = open_range_px(chart_width);
    let start = lane.resolved_width_px(chart_width);
    let floor =
        (chart_width * MIN_LIVE_LANE_SHARE).max(MIN_LIVE_LANE_WIDTH_PX.min(chart_width / 2.0));
    let ceiling = chart_width * MAX_LIVE_LANE_SHARE;
    let low = if start < low { floor.min(start) } else { low };
    let high = if start > high {
        ceiling.max(start)
    } else {
        high
    };
    (low, high)
}

/// One drag of the lane's divider, from press to release.
///
/// Follows the hand rather than summing clamped steps: the tape is as wide as
/// the distance from the pointer to the chart's right edge, so dragging past
/// an edge and back puts the divider under the pointer again. Only the release
/// decides a view, and one that does gives the lane back the share the drag
/// started from, which is the width returning beside the candles restores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneDividerDrag {
    chart_width: f32,
    wanted_px: f32,
    restore_share: f32,
    open_px: (f32, f32),
}

impl LaneDividerDrag {
    /// A drag starting on the lane as it is now, on a chart this wide.
    #[must_use]
    pub fn begin(lane: &LiveLaneStyle, chart_width: f32) -> Self {
        Self {
            chart_width,
            wanted_px: lane.resolved_width_px(chart_width),
            restore_share: lane.width_share,
            open_px: drag_range_px(lane, chart_width),
        }
    }

    /// Move the divider by `delta_px` (left is negative: a wider tape), under
    /// the hand as far as the open range goes. Unmeasurable input moves nothing.
    pub fn drag(&mut self, lane: &mut LiveLaneStyle, delta_px: f32, chart_width: f32) {
        if !delta_px.is_finite() || !measurable(chart_width) {
            return;
        }
        if chart_width != self.chart_width {
            // The pane resized under the hand: carry on from the band on
            // screen rather than from a width measured on another chart.
            self.chart_width = chart_width;
            self.wanted_px = lane.resolved_width_px(chart_width);
            self.open_px = drag_range_px(lane, chart_width);
        }
        self.wanted_px -= delta_px;
        let (low, high) = self.open_px;
        lane.width_share = self.wanted_px.clamp(low, high.max(low)) / chart_width;
    }

    /// Let go: past either end of the open range that edge's side leaves the
    /// pane and the lane gets its share back. Answers whether the view changed.
    pub fn release(self, lane: &mut LiveLaneStyle) -> bool {
        let (low, high) = self.open_px;
        let view = if self.wanted_px < low {
            LaneView::CandlesOnly
        } else if self.wanted_px > high {
            LaneView::TapeOnly
        } else {
            return false;
        };
        lane.width_share = self.restore_share;
        lane.set_view(view)
    }

    /// The divider left under the hand, so no release will come: the lane
    /// gets its share back and the view stays whatever took the divider away.
    pub fn abandon(self, lane: &mut LiveLaneStyle) {
        lane.width_share = self.restore_share;
    }
}

impl HeatmapConfig {
    /// One step of the divider drag a pane carries in `hand` across frames:
    /// the first step begins it on the lane as it stands, every later one
    /// moves it ([`LaneDividerDrag::drag`]). Answers whether there was a
    /// divider to move: a tape-only pane is all tape and has none.
    pub fn drag_lane_divider(
        &mut self,
        hand: &mut Option<LaneDividerDrag>,
        delta_px: f32,
        chart_width: f32,
    ) -> bool {
        if self.tape_only() {
            return false;
        }
        let lane = &mut self.live_lane;
        let drag = hand.get_or_insert_with(|| LaneDividerDrag::begin(lane, chart_width));
        drag.drag(lane, delta_px, chart_width);
        true
    }

    /// The drag in `hand` let go ([`LaneDividerDrag::release`]); answers
    /// whether the view changed. An empty hand changes nothing.
    pub fn release_lane_divider(&mut self, hand: &mut Option<LaneDividerDrag>) -> bool {
        hand.take()
            .is_some_and(|drag| drag.release(&mut self.live_lane))
    }

    /// The drag in `hand` given up ([`LaneDividerDrag::abandon`]). An empty
    /// hand changes nothing.
    pub fn abandon_lane_divider(&mut self, hand: &mut Option<LaneDividerDrag>) {
        if let Some(drag) = hand.take() {
            drag.abandon(&mut self.live_lane);
        }
    }
}

#[cfg(test)]
#[path = "lane_fold_tests.rs"]
mod lane_fold_tests;
