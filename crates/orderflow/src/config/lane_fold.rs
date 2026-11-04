//! Closing a side of the flow pane with its divider.
//!
//! The divider between the candles and the tape drags across the whole chart.
//! Let go within [`LANE_FOLD_AT_PX`] of the left edge and the view is tape only;
//! of the right edge and it is candles only: the very states the Trail and Tape
//! switches write ([`LaneView`]), with nothing kept apart from them. A pane
//! showing one side keeps a rail [`LANE_FOLD_RAIL_PX`] wide at the closed edge,
//! however it got there, and that rail reopens it: a click at the default width,
//! a drag where the hand lets go.
//!
//! The numbers are the canvas pane fold's (the app's `canvas_layout` collapse
//! constants, which assert they agree with these): the same rail, the same
//! threshold and the same hit area, so the two feel alike.
//!
//! Mid-drag the close is only shown ([`LiveLaneStyle::drag_close`]): nothing is
//! written until the hand lets go past the threshold, so crossing it and coming
//! back leaves the tape as it was.
//!
//! [`LaneFold`] survives only as a word: what a file written while a closed side
//! was stored apart still says, read once and never written, and what
//! `orderflow.tape.fold.set` calls a view.

use serde::{Deserialize, Serialize};

use super::HeatmapConfig;
use super::lane::{
    DEFAULT_LIVE_LANE_SHARE, LiveLaneStyle, MAX_LIVE_LANE_SHARE, MIN_LIVE_LANE_SHARE,
    MIN_LIVE_LANE_WIDTH_PX,
};
use super::lane_view::{LaneSide, LaneView};

/// Width of a closed side's rail, in pixels: the canvas pane's rail.
pub const LANE_FOLD_RAIL_PX: f32 = 8.0;
/// How close to an edge the divider has to come before that side closes
/// rather than narrows: the canvas pane's collapse threshold.
pub const LANE_FOLD_AT_PX: f32 = 120.0;
/// How wide a rail is to the pointer, reaching into the open side: the
/// canvas rail's hit area, the floor a pointer target is held to.
pub const LANE_FOLD_HIT_PX: f32 = 24.0;

/// A view by the side it closes: an old file's word, and the wire's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LaneFold {
    /// Neither side closed: [`LaneView::Beside`].
    #[default]
    Open,
    /// The candles closed: [`LaneView::TapeOnly`].
    Candles,
    /// The tape closed: [`LaneView::CandlesOnly`].
    Tape,
}

impl LaneFold {
    /// Whether neither side is closed.
    #[must_use]
    pub fn is_open(&self) -> bool {
        *self == Self::Open
    }

    /// The view this word names.
    #[must_use]
    pub fn view(self) -> LaneView {
        match self {
            Self::Open => LaneView::Beside,
            Self::Candles => LaneView::TapeOnly,
            Self::Tape => LaneView::CandlesOnly,
        }
    }
}

/// The market-closed hold's memory: runtime only, never in a file, and it writes
/// neither the view nor the share: it is shown over them. Decided by the chart
/// crate's `market_fold`.
#[derive(Debug, Clone, Copy, Default)]
pub struct MarketHold {
    /// The tape is away because the market is closed.
    pub held: bool,
    /// A hand opened the tape: no hold until the next entry into a closed spell.
    pub stood_down: bool,
    /// The link has said closed, and nothing has answered it yet.
    pub closed_spell: bool,
    /// The last print's date as of the last step.
    pub seen_print_at: Option<i64>,
}

/// Only whether it is held changes what a lane looks like; the rest is memory.
impl PartialEq for MarketHold {
    fn eq(&self, other: &Self) -> bool {
        self.held == other.held
    }
}

impl Eq for MarketHold {}

impl MarketHold {
    /// A hand moved the lane: release, and hold off until the next entry.
    pub fn stand_down(&mut self) {
        self.held = false;
        self.stood_down = true;
    }
}

/// A file's old `fold = "tape"`, read and not yet carried out. Runtime only, like
/// the hold, and never written. Compared, so a store entry holding it differs from
/// what the view files and the next filing writes it out; the bubble look spends
/// it, once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OldTapeFold(pub bool);

/// Where a closed side's rail is, in pixels from the chart's left edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldRail {
    /// The side that is closed, whose edge the rail stands on.
    pub side: LaneSide,
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
        if self.side == LaneSide::Tape {
            self.paint_from_px
        } else {
            self.paint_to_px
        }
    }
}

/// The rail on a chart this wide: never more than half of it.
pub(super) fn rail_px(chart_width: f32) -> f32 {
    LANE_FOLD_RAIL_PX.min(chart_width / 2.0)
}

/// The band beside the candles at `share` of a chart this wide. The floor
/// yields on a narrow window: half a small chart beats a fixed band that would
/// eat all of it.
pub(super) fn beside_width_px(chart_width: f32, share: f32) -> f32 {
    let floor = super::lane::MIN_LIVE_LANE_WIDTH_PX.min(chart_width / 2.0);
    (chart_width * share).max(floor)
}

/// The closing threshold on a chart this wide: a quarter of a chart too narrow
/// for the full one, so a small pane still has open widths to drag through.
fn fold_at_px(chart_width: f32) -> f32 {
    LANE_FOLD_AT_PX.min(chart_width / 4.0)
}

fn measurable(chart_width: f32) -> bool {
    chart_width.is_finite() && chart_width > 0.0
}

impl LiveLaneStyle {
    /// What a file written with a closed side apart from its view still says,
    /// where the view was beside, since a view other than beside won on screen:
    /// the candles' side closes; the tape's waits as [`Self::old_tape_fold`], since
    /// the tape's switch is not a look's to carry. The share stays as saved.
    pub fn read_old_fold(&mut self, fold: LaneFold) {
        if self.view() != LaneView::Beside {
            return;
        }
        if fold == LaneFold::Tape {
            self.old_tape_fold = OldTapeFold(true);
        } else {
            self.write_view(fold.view());
        }
    }

    /// An explicit word on the pane, a rail or `orderflow.tape.fold.set`: the
    /// market's hold stands down for it. Open is beside at the default width,
    /// held or not; a side closes keeping the share. Answers whether the pane
    /// moved, so a retry does not.
    pub fn set_fold(&mut self, fold: LaneFold) -> bool {
        match fold.view() {
            LaneView::Beside => self.open_beside(),
            view => self.show_by_hand(view),
        }
    }

    /// Show `view` where a hand left it: no snap, so the share is whatever the
    /// hand set. The market's hold stands down.
    pub fn show_by_hand(&mut self, view: LaneView) -> bool {
        let released = self.market_held();
        self.market_hold.stand_down();
        self.write_view(view) || released
    }

    /// Whether the market is holding the tape away on this pane.
    #[must_use]
    pub fn market_held(&self) -> bool {
        self.view() == LaneView::Beside && self.market_hold.held
    }

    /// Switch the tape on or off; answers whether it changed. Back on beside
    /// the candles is at the default width ([`Self::open_beside`]); a `tape_only`
    /// a preset saved with the tape off brings back the tape alone.
    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        if enabled && !self.tape_only {
            return self.open_beside();
        }
        self.enabled = enabled;
        true
    }

    /// A saved answer put back on the tape's switch, and nothing else: restoring
    /// is no hand, so the saved share and the market's hold stay. Answers whether
    /// it changed.
    pub fn restore_enabled(&mut self, enabled: bool) -> bool {
        let changed = self.enabled != enabled;
        self.enabled = enabled;
        changed
    }

    /// Ask for the tape alone, or not; answers whether it changed. Back beside
    /// the candles is at the default width ([`Self::open_beside`]).
    pub fn set_tape_only(&mut self, tape_only: bool) -> bool {
        if self.tape_only == tape_only {
            return false;
        }
        if self.enabled && !tape_only {
            return self.open_beside();
        }
        self.tape_only = tape_only;
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

    /// The share of the frame's mark budget the tape spends: its share, or the
    /// least while the market holds it away. Its share even alone on the pane,
    /// since the live strip still reads the forming bar from the candles' marks.
    #[must_use]
    pub fn budget_share(&self) -> f32 {
        if self.market_held() {
            0.0
        } else {
            self.open_share()
        }
    }

    /// Where the divider takes the pointer, in pixels from the chart's left:
    /// `half_px` either side of an open lane's divider, or a closed side's rail.
    /// `None` on a chart that cannot be measured.
    #[must_use]
    pub fn divider_hit(&self, chart_width: f32, half_px: f32) -> Option<(f32, f32)> {
        if let Some(rail) = self.fold_rail(chart_width) {
            return Some((rail.hit_from_px, rail.hit_to_px));
        }
        let width = self.resolved_width_px(chart_width);
        let x = chart_width - width;
        (width > 0.0).then_some((x - half_px, x + half_px))
    }

    /// The closed side's rail on a chart this wide, or `None` beside the candles.
    #[must_use]
    pub fn fold_rail(&self, chart_width: f32) -> Option<FoldRail> {
        if !measurable(chart_width) {
            return None;
        }
        let rail = rail_px(chart_width);
        let hit = LANE_FOLD_HIT_PX.max(rail).min(chart_width);
        match self.shown_view() {
            LaneView::Beside => None,
            LaneView::TapeOnly => Some(FoldRail {
                side: LaneSide::Trail,
                paint_from_px: 0.0,
                paint_to_px: rail,
                hit_from_px: 0.0,
                hit_to_px: hit,
            }),
            LaneView::CandlesOnly => Some(FoldRail {
                side: LaneSide::Tape,
                paint_from_px: chart_width - rail,
                paint_to_px: chart_width,
                hit_from_px: chart_width - hit,
                hit_to_px: chart_width,
            }),
        }
    }

    /// The band a frame keeps clear of the candles: the lane's, or a closed tape's
    /// rail, which no lane reserves and no candle may stand under.
    #[must_use]
    pub fn candles_clear_px(&self, chart_width: f32) -> f32 {
        match self.fold_rail(chart_width) {
            Some(rail) if rail.side == LaneSide::Tape => rail_px(chart_width),
            _ => self.resolved_width_px(chart_width),
        }
    }

    /// The width a hand on the divider starts from: the distance from the
    /// divider, or the rail's inner edge, to the chart's right.
    fn drag_start_px(&self, chart_width: f32) -> f32 {
        self.fold_rail(chart_width).map_or_else(
            || self.resolved_width_px(chart_width),
            |rail| chart_width - rail.edge_px(),
        )
    }

    /// The hand let go: a close still shown is written now, the one write it
    /// makes. Answers whether the view changed.
    pub fn release_drag_close(&mut self) -> bool {
        self.drag_close
            .take()
            .is_some_and(|view| self.show_by_hand(view))
    }
}

/// The tape widths the divider can leave open on a chart this wide: from
/// the wider of the threshold and the share's floor (and the pixel floor) up to
/// the narrower of the threshold and the share's ceiling. Past either end a side
/// closes, so an open lane is always drawn where the hand put it rather than
/// stopping at a clamp and then jumping to a rail.
fn open_range_px(chart_width: f32) -> (f32, f32) {
    let at = fold_at_px(chart_width);
    let floor_px = MIN_LIVE_LANE_WIDTH_PX.min(chart_width / 2.0);
    let low = at.max(chart_width * MIN_LIVE_LANE_SHARE).max(floor_px);
    let high = (chart_width - at).min(chart_width * MAX_LIVE_LANE_SHARE);
    (low, high)
}

/// The tape widths one drag leaves open: [`open_range_px`], reaching down to
/// the share's floor or up to its ceiling when an open lane starts outside
/// it, so a lane kept narrower or wider than the range moves with the hand from
/// where it is rather than closing on the first pixel.
fn drag_range_px(lane: &LiveLaneStyle, chart_width: f32) -> (f32, f32) {
    let (low, high) = open_range_px(chart_width);
    if lane.shown_view() != LaneView::Beside {
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

/// One drag of the lane's divider or rail, from press to release.
///
/// Follows the hand rather than summing clamped steps: the tape is as wide as
/// the distance from the pointer to the chart's right edge, so dragging past
/// an edge and back puts the divider under the pointer again. Closing a side
/// gives the lane back the share the drag started from, and is only shown until
/// the release ([`LiveLaneStyle::release_drag_close`]); opening one is written as
/// the hand crosses, since a closed side has nothing to show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneDividerDrag {
    chart_width: f32,
    wanted_px: f32,
    restore_share: f32,
    open_px: (f32, f32),
    /// It began on the market's rail and has not left the rail's range: nothing
    /// it does here is the trader's, even after a print ends the hold.
    on_market_rail: bool,
}

impl LaneDividerDrag {
    /// A drag starting on the lane as it is now, on a chart this wide.
    #[must_use]
    pub fn begin(lane: &LiveLaneStyle, chart_width: f32) -> Self {
        Self {
            chart_width,
            wanted_px: lane.drag_start_px(chart_width),
            restore_share: lane.width_share,
            open_px: drag_range_px(lane, chart_width),
            on_market_rail: lane.market_held(),
        }
    }

    /// Move the divider by `delta_px` (left is negative: a wider tape) and
    /// decide the lane: one side alone shown past either threshold, beside under
    /// the hand otherwise. Answers whether the stored view changed; unmeasurable
    /// input moves nothing.
    pub fn drag(&mut self, lane: &mut LiveLaneStyle, delta_px: f32, chart_width: f32) -> bool {
        if !delta_px.is_finite() || !measurable(chart_width) {
            return false;
        }
        if chart_width != self.chart_width {
            // The pane resized under the hand: carry on from the band on
            // screen rather than from a width measured on another chart.
            self.chart_width = chart_width;
            self.wanted_px = lane.drag_start_px(chart_width);
            self.open_px = drag_range_px(lane, chart_width);
        }
        self.wanted_px -= delta_px;
        let (low, high) = self.open_px;
        let (view, share) = if self.wanted_px < low {
            (LaneView::CandlesOnly, self.restore_share)
        } else if self.wanted_px > high {
            (LaneView::TapeOnly, self.restore_share)
        } else {
            (LaneView::Beside, self.wanted_px / chart_width)
        };
        lane.width_share = share;
        self.on_market_rail &= view == LaneView::CandlesOnly;
        lane.drag_close = None;
        // A wiggle on the market's rail is not the trader's. Nor is a step still in
        // the rail's range once a print has released the hold under the hand: the
        // view saved must never be the held one.
        if self.on_market_rail {
            return false;
        }
        if view == LaneView::Beside {
            return lane.shown_view() != view && lane.show_by_hand(view);
        }
        if view == lane.view() {
            return false;
        }
        // One step from a closed side past the other's threshold still opens first:
        // a close is only ever shown over a lane beside the candles.
        let opened = lane.view() != LaneView::Beside && lane.show_by_hand(LaneView::Beside);
        lane.drag_close = Some(view);
        opened
    }
}

impl HeatmapConfig {
    /// One step of the divider drag a pane carries in `hand` across frames:
    /// the first step begins it on the lane as it stands, every later one
    /// moves it ([`LaneDividerDrag::drag`]). Answers whether the view changed.
    pub fn drag_lane_divider(
        &mut self,
        hand: &mut Option<LaneDividerDrag>,
        delta_px: f32,
        chart_width: f32,
    ) -> bool {
        let lane = &mut self.live_lane;
        let drag = hand.get_or_insert_with(|| LaneDividerDrag::begin(lane, chart_width));
        drag.drag(lane, delta_px, chart_width)
    }
}

#[cfg(test)]
#[path = "lane_fold_tests.rs"]
mod lane_fold_tests;
