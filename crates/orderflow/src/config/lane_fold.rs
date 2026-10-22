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

use super::lane::{
    DEFAULT_LIVE_LANE_SHARE, LiveLaneStyle, MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE,
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
    /// Fold the lane, or open it at the share it kept. Answers whether the
    /// fold changed.
    pub fn set_fold(&mut self, fold: LaneFold) -> bool {
        let changed = self.fold != fold;
        self.fold = fold;
        changed
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
    /// all of it with the candles folded, the least with itself folded.
    #[must_use]
    pub fn budget_share(&self) -> f32 {
        match self.fold {
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
        (width > 0.0 && !self.tape_only).then_some((x - half_px, x + half_px))
    }

    /// The folded side's rail on a chart this wide, or `None` when there is
    /// no rail to draw: the lane open, the tape off or alone on the pane.
    #[must_use]
    pub fn fold_rail(&self, chart_width: f32) -> Option<FoldRail> {
        if !self.enabled || self.tape_only || !measurable(chart_width) {
            return None;
        }
        let rail = rail_px(chart_width);
        let hit = LANE_FOLD_HIT_PX.max(rail).min(chart_width);
        match self.fold {
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
}

impl LaneDividerDrag {
    /// A drag starting on the lane as it is now, on a chart this wide.
    #[must_use]
    pub fn begin(lane: &LiveLaneStyle, chart_width: f32) -> Self {
        Self {
            chart_width,
            wanted_px: lane.resolved_width_px(chart_width),
            restore_share: lane.width_share,
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
        let was = lane.fold;
        if chart_width != self.chart_width {
            // The pane resized under the hand: carry on from the band on
            // screen rather than from a width measured on another chart.
            self.chart_width = chart_width;
            self.wanted_px = lane.resolved_width_px(chart_width);
        }
        self.wanted_px -= delta_px;
        let at = fold_at_px(chart_width);
        if self.wanted_px < at {
            lane.fold = LaneFold::Tape;
            lane.width_share = self.restore_share;
        } else if chart_width - self.wanted_px < at {
            lane.fold = LaneFold::Candles;
            lane.width_share = self.restore_share;
        } else {
            lane.fold = LaneFold::Open;
            lane.width_share = self.wanted_px / chart_width;
        }
        lane.fold != was
    }
}

#[cfg(test)]
#[path = "lane_fold_tests.rs"]
mod lane_fold_tests;
