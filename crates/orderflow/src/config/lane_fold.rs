//! Folding the tape lane against either edge of its pane.
//!
//! The divider between the candles and the tape drags across the whole
//! chart. Let go within [`LANE_FOLD_AT_PX`] of the left edge and the candles
//! fold to a rail, so the tape takes the pane; of the right edge and the tape
//! folds instead. The folded side keeps a rail [`LANE_FOLD_RAIL_PX`] wide to
//! click, and clicking it unfolds the lane at the share it had before.
//!
//! The numbers are the canvas pane fold's (the app's `canvas_layout`
//! collapse constants, which assert they agree with these): the same rail,
//! the same threshold and the same hit area, so the two folds feel alike.
//!
//! A fold is where the lane sits, not what the tape draws: it leaves the
//! share alone (that is what unfolding returns to), never regroups the tape
//! and never touches `tape_only`, which keeps its own meaning and wins over
//! the fold while it is on.

use serde::{Deserialize, Serialize};

use super::HeatmapConfig;
use super::lane::{
    DEFAULT_LIVE_LANE_SHARE, LiveLaneStyle, MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE,
    MIN_LIVE_LANE_WIDTH_PX,
};

/// Width of a folded side's rail, in pixels: the canvas pane's rail.
pub const LANE_FOLD_RAIL_PX: f32 = 8.0;
/// How close to an edge the divider has to come before that side folds
/// rather than narrows: the canvas pane's collapse threshold.
pub const LANE_FOLD_AT_PX: f32 = 120.0;
/// How wide a rail is to the pointer, reaching into the open side: the
/// canvas rail's hit area, the floor a pointer target is held to.
pub const LANE_FOLD_HIT_PX: f32 = 24.0;

/// Which side of the pane the lane's divider folded away.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneFold {
    /// Candles and tape side by side, split at the lane's share.
    #[default]
    Open,
    /// The candles folded to a rail on the left; the tape takes the rest.
    Candles,
    /// The tape folded to a rail on the right; the candles take the rest.
    Tape,
}

impl LaneFold {
    /// Whether neither side is folded.
    #[must_use]
    pub fn is_open(&self) -> bool {
        *self == Self::Open
    }
}

/// Where a folded side's rail is, in pixels from the chart's left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldRail {
    /// The side that folded: never [`LaneFold::Open`].
    pub side: LaneFold,
    pub paint_from_px: f32,
    pub paint_to_px: f32,
    /// The pointer target: the rail, widened into the open side.
    pub hit_from_px: f32,
    pub hit_to_px: f32,
}

impl FoldRail {
    /// The rail's inner edge, where it meets the open side.
    #[must_use]
    pub fn edge_px(&self) -> f32 {
        if self.side == LaneFold::Tape {
            self.paint_from_px
        } else {
            self.paint_to_px
        }
    }
}

/// The rail on a chart this wide: never more than half of it.
fn rail_px(chart_width: f32) -> f32 {
    LANE_FOLD_RAIL_PX.min(chart_width / 2.0)
}

/// The fold threshold on a chart this wide: a quarter of a chart too narrow
/// for the full one, so a small pane still has open widths to drag through.
fn fold_at_px(chart_width: f32) -> f32 {
    LANE_FOLD_AT_PX.min(chart_width / 4.0)
}

fn measurable(chart_width: f32) -> bool {
    chart_width.is_finite() && chart_width > 0.0
}

impl LiveLaneStyle {
    /// Fold the lane, or open it at the share it kept: the one write every
    /// fold goes through. Answers whether the fold changed.
    pub fn set_fold(&mut self, fold: LaneFold) -> bool {
        let changed = self.fold != fold;
        self.fold = fold;
        changed
    }

    /// Whether the tape sits beside the candles, switched on and not alone
    /// on the pane: the one rule that says a divider exists to fold, read by
    /// the rail, the divider and the control plane alike.
    #[must_use]
    pub fn beside_candles(&self) -> bool {
        self.enabled && !self.tape_only
    }

    /// The fold the pane shows: the kept one beside the candles, open
    /// otherwise, since the tape's own switches win over it.
    #[must_use]
    pub fn shown_fold(&self) -> LaneFold {
        if self.beside_candles() {
            self.fold
        } else {
            LaneFold::Open
        }
    }

    /// Switch the tape on or off; answers whether it changed. Switched on,
    /// a fold it was switched off in opens at the share it kept: the Tape
    /// chip brings back a tape, never a rail.
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        self.enabled = enabled;
        if enabled {
            self.set_fold(LaneFold::Open);
        }
        true
    }

    /// The share the lane's open band takes, clamped as the layout clamps it.
    #[must_use]
    pub(super) fn open_share(&self) -> f32 {
        if self.width_share.is_finite() {
            self.width_share
                .clamp(MIN_LIVE_LANE_SHARE, MAX_LIVE_LANE_SHARE)
        } else {
            DEFAULT_LIVE_LANE_SHARE
        }
    }

    /// The lane's width on a measurable chart when folded; `None` while open.
    #[must_use]
    pub(super) fn folded_width_px(&self, chart_width: f32) -> Option<f32> {
        match self.fold {
            LaneFold::Open => None,
            LaneFold::Candles => Some(chart_width - rail_px(chart_width)),
            LaneFold::Tape => Some(rail_px(chart_width)),
        }
    }

    /// The share of the frame's mark budget the tape spends: its open share,
    /// all of it with the candles folded, the least with itself folded. A
    /// fold the pane does not show ([`Self::shown_fold`]) moves nothing.
    #[must_use]
    pub fn budget_share(&self) -> f32 {
        match self.shown_fold() {
            LaneFold::Open => self.open_share(),
            LaneFold::Candles => 1.0,
            LaneFold::Tape => 0.0,
        }
    }

    /// Where the divider takes the pointer, in pixels from the chart's left:
    /// `half_px` either side of an open lane's divider, or a folded side's
    /// rail. `None` without a divider: the tape off or alone on the pane.
    #[must_use]
    pub fn divider_hit(&self, chart_width: f32, half_px: f32) -> Option<(f32, f32)> {
        if let Some(rail) = self.fold_rail(chart_width) {
            return Some((rail.hit_from_px, rail.hit_to_px));
        }
        let width = self.resolved_width_px(chart_width);
        let x = chart_width - width;
        (width > 0.0 && self.beside_candles()).then_some((x - half_px, x + half_px))
    }

    /// The folded side's rail on a chart this wide, or `None` when there is
    /// no rail to draw: the lane open, the tape off or alone on the pane.
    #[must_use]
    pub fn fold_rail(&self, chart_width: f32) -> Option<FoldRail> {
        if !measurable(chart_width) {
            return None;
        }
        let rail = rail_px(chart_width);
        let hit = LANE_FOLD_HIT_PX.max(rail).min(chart_width);
        match self.shown_fold() {
            LaneFold::Open => None,
            LaneFold::Candles => Some(FoldRail {
                side: LaneFold::Candles,
                paint_from_px: 0.0,
                paint_to_px: rail,
                hit_from_px: 0.0,
                hit_to_px: hit,
            }),
            LaneFold::Tape => Some(FoldRail {
                side: LaneFold::Tape,
                paint_from_px: chart_width - rail,
                paint_to_px: chart_width,
                hit_from_px: chart_width - hit,
                hit_to_px: chart_width,
            }),
        }
    }
}

/// The tape widths the divider can leave open on a chart this wide: from
/// the wider of the fold threshold and the share's floor (and the pixel
/// floor) up to the narrower of the threshold and the share's ceiling. Past
/// either end a side folds, so an open lane is always drawn where the hand
/// put it rather than stopping at a clamp and then jumping to a rail.
fn open_range_px(chart_width: f32) -> (f32, f32) {
    let at = fold_at_px(chart_width);
    let floor_px = MIN_LIVE_LANE_WIDTH_PX.min(chart_width / 2.0);
    let low = at.max(chart_width * MIN_LIVE_LANE_SHARE).max(floor_px);
    let high = (chart_width - at).min(chart_width * MAX_LIVE_LANE_SHARE);
    (low, high)
}

/// The tape widths one drag leaves open: [`open_range_px`], reaching down to
/// the share's floor or up to its ceiling when an open lane starts outside
/// it, so a lane kept narrower or wider than the fold range moves with the
/// hand from where it is rather than folding on the first pixel.
fn drag_range_px(lane: &LiveLaneStyle, chart_width: f32) -> (f32, f32) {
    let (low, high) = open_range_px(chart_width);
    if !lane.fold.is_open() {
        return (low, high);
    }
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
/// an edge and back puts the divider under the pointer again. Folding gives
/// the lane back the share the drag started from, which is the width a click
/// on the rail restores.
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

    /// Move the divider by `delta_px` (left is negative: a wider tape) and
    /// decide the lane: folded past either threshold, open under the hand
    /// otherwise. Answers whether the fold changed; unmeasurable input moves
    /// nothing.
    pub fn drag(&mut self, lane: &mut LiveLaneStyle, delta_px: f32, chart_width: f32) -> bool {
        if !delta_px.is_finite() || !measurable(chart_width) {
            return false;
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
        let (fold, share) = if self.wanted_px < low {
            (LaneFold::Tape, self.restore_share)
        } else if self.wanted_px > high {
            (LaneFold::Candles, self.restore_share)
        } else {
            (LaneFold::Open, self.wanted_px / chart_width)
        };
        lane.width_share = share;
        lane.set_fold(fold)
    }
}

impl HeatmapConfig {
    /// One step of the divider drag a pane carries in `hand` across frames:
    /// the first step begins it on the lane as it stands, every later one
    /// moves it ([`LaneDividerDrag::drag`]). `None` on a tape-only pane, which
    /// is all tape and has no divider to move; otherwise whether the fold
    /// changed.
    pub fn drag_lane_divider(
        &mut self,
        hand: &mut Option<LaneDividerDrag>,
        delta_px: f32,
        chart_width: f32,
    ) -> Option<bool> {
        if self.tape_only() {
            return None;
        }
        let lane = &mut self.live_lane;
        let drag = hand.get_or_insert_with(|| LaneDividerDrag::begin(lane, chart_width));
        Some(drag.drag(lane, delta_px, chart_width))
    }
}

#[cfg(test)]
#[path = "lane_fold_tests.rs"]
mod lane_fold_tests;
