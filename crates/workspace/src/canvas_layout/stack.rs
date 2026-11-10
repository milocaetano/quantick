//! The context stack beside the flow pane: the heights the trader gave its
//! charts, the column it was last drawn in, and the one operation a divider
//! drag and a remote caller share, with its floors.
//!
//! Height choices survive layouts that hide the stack; the drawn frame only
//! describes a stack on screen, so an action can refuse a stale one.

use super::split::{Span, centre};
use super::{
    CANVAS_DIVIDER_PX, COLLAPSE_AT_PX, COLLAPSED_PANE_WIDTH_PX, MAX_CONTEXT_PANES,
    MIN_PANE_WIDTH_PX, PaneWidth, split,
};
use crate::canvas::CanvasRect;

/// One tab's stacked context charts.
#[derive(Debug, Default)]
pub struct ContextStack {
    pub heights: Vec<PaneWidth>,
    pub frame: Option<StackFrame>,
}

/// Where the stack was last drawn and which charts it held, top to bottom.
#[derive(Debug)]
pub struct StackFrame {
    pub column: CanvasRect,
    pub pane_ids: Vec<u64>,
}

/// Move the boundary between two adjacent charts, named by their stable ids
/// so a delayed call never resizes replacement panes.
#[derive(Clone, Copy, Debug)]
pub struct ResizeContextPair {
    pub upper_pane_id: u64,
    pub lower_pane_id: u64,
    pub wanted_y: f32,
}

/// Where the boundary landed, as a share of the column, and whether it moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContextPairResult {
    pub fraction: f32,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContextResizeError {
    NotDrawn,
    NotAdjacent,
    InvalidCoordinate,
}

impl std::fmt::Display for ContextResizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotDrawn => "the current context stack must be visible and drawn before resizing",
            Self::NotAdjacent => "the named panes are not an adjacent upper/lower context pair",
            Self::InvalidCoordinate => "the requested divider coordinate must be finite",
        })
    }
}

impl ContextStack {
    /// The trader's vertical sizing as shares, for the saved document.
    #[must_use]
    pub fn height_shares(&self) -> Vec<f32> {
        let share = |height: &PaneWidth| match *height {
            PaneWidth::Auto => 0.0,
            PaneWidth::Manual(share) | PaneWidth::Collapsed { restore: share } => share,
        };
        self.heights.iter().map(share).collect()
    }

    #[must_use]
    pub fn collapsed_slots(&self) -> Vec<bool> {
        self.heights
            .iter()
            .map(|height| height.is_collapsed())
            .collect()
    }

    /// Reopen every folded chart at the height it had; a folded pair gives
    /// its partner back the rest.
    pub fn expand(&mut self) {
        let folded = (self.heights.iter().enumerate()).find_map(|(slot, height)| match *height {
            PaneWidth::Collapsed { restore } => Some((slot, restore)),
            _ => None,
        });
        if let (2, Some((slot, restore))) = (self.heights.len(), folded) {
            self.heights[slot] = PaneWidth::Manual(restore);
            self.heights[1 - slot] = PaneWidth::Manual(1.0 - restore);
            return;
        }
        for height in &mut self.heights {
            if let PaneWidth::Collapsed { restore } = *height {
                *height = PaneWidth::Manual(restore);
            }
        }
    }

    /// Restore saved `shares` and folds; a share that is not one opens
    /// automatic, and a stack folded whole opens its first chart.
    pub fn restore(&mut self, shares: &[f32], collapsed: &[bool]) {
        self.heights.clear();
        for (slot, share) in shares.iter().copied().take(MAX_CONTEXT_PANES).enumerate() {
            let size = if !share.is_finite() || !(0.0..=1.0).contains(&share) || share <= 0.0 {
                PaneWidth::Auto
            } else if collapsed.get(slot).copied().unwrap_or(false) {
                PaneWidth::Collapsed { restore: share }
            } else {
                PaneWidth::Manual(share)
            };
            self.heights.push(size);
        }
        if self.heights.iter().all(|height| height.is_collapsed()) && !self.heights.is_empty() {
            self.heights[0] = PaneWidth::Auto;
        }
    }

    /// Fold the upper or the lower of two charts, the other keeping its
    /// share (an even split where none was given).
    pub fn fold_one(&mut self, upper: bool) {
        let shares = self.height_shares();
        let share = |slot: usize| shares.get(slot).copied().filter(|share| *share > 0.0);
        let shares = [share(0).unwrap_or(0.5), share(1).unwrap_or(0.5)];
        self.restore(&shares, &[upper, !upper]);
    }

    /// Which boundary of the drawn stack `request` names.
    ///
    /// # Errors
    /// Nothing drawn, or the two charts are not adjacent in it.
    pub fn pair_index(&self, request: &ResizeContextPair) -> Result<usize, ContextResizeError> {
        let frame = self.frame.as_ref().ok_or(ContextResizeError::NotDrawn)?;
        let pair = [request.upper_pane_id, request.lower_pane_id];
        let index = frame.pane_ids.windows(2).position(|held| held == pair);
        index.ok_or(ContextResizeError::NotAdjacent)
    }

    /// Move the boundary below chart `index` of the stack drawn in `column`
    /// towards `wanted_y`, each chart keeping its floor (`footer_px` of
    /// footer on top of a pane's); dragged within reach of an edge, a chart
    /// folds, and dragged back out it reopens.
    ///
    /// # Errors
    /// `wanted_y` is not finite.
    pub fn resize(
        &mut self,
        index: usize,
        wanted_y: f32,
        column: CanvasRect,
        dividers: &[Span],
        footer_px: f32,
    ) -> Result<ContextPairResult, ContextResizeError> {
        if !wanted_y.is_finite() {
            return Err(ContextResizeError::InvalidCoordinate);
        }
        let pair_top = index
            .checked_sub(1)
            .map_or(column.y, |above| centre(dividers[above]));
        let pair_bottom = dividers
            .get(index + 1)
            .map_or(column.bottom(), |at| centre(*at));
        let pair_height = pair_bottom - pair_top;
        // Height buys the pane-local footer too, so it cannot eat the chart's floor.
        let floor = MIN_PANE_WIDTH_PX + footer_px;
        let previous_y = centre(dividers[index]);
        let fraction = |y: f32| (y - column.y) / column.h;
        if pair_height < floor + COLLAPSED_PANE_WIDTH_PX {
            return Ok(ContextPairResult {
                fraction: fraction(previous_y),
                changed: false,
            });
        }
        let (upper, lower) = (self.heights[index], self.heights[index + 1]);
        let pair_share = pair_height / column.h;
        let count = self.heights.len();
        let collapsed_share = |slot: usize| {
            let dividers = usize::from(slot > 0) + usize::from(slot + 1 < count);
            (COLLAPSED_PANE_WIDTH_PX + dividers as f32 * CANVAS_DIVIDER_PX / 2.0) / column.h
        };
        let collapse_upper = wanted_y - pair_top < COLLAPSE_AT_PX;
        let collapse_lower = pair_bottom - wanted_y < COLLAPSE_AT_PX;
        let open = !upper.is_collapsed() && !lower.is_collapsed();
        if collapse_upper && open {
            let restore = (previous_y - pair_top) / column.h;
            self.heights[index] = PaneWidth::Collapsed { restore };
            self.heights[index + 1] = PaneWidth::Manual(pair_share - collapsed_share(index));
        } else if collapse_lower && open {
            let restore = (pair_bottom - previous_y) / column.h;
            self.heights[index] = PaneWidth::Manual(pair_share - collapsed_share(index + 1));
            self.heights[index + 1] = PaneWidth::Collapsed { restore };
        } else if !collapse_upper && !collapse_lower {
            // A short column can hold one open pane and a rail but cannot
            // satisfy two full floors. Reopening must still be reachable.
            let open_floor = if pair_height >= floor * 2.0 {
                floor
            } else {
                COLLAPSE_AT_PX
            };
            let applied = wanted_y.clamp(pair_top + open_floor, pair_bottom - open_floor);
            self.heights[index] = PaneWidth::Manual((applied - pair_top) / column.h);
            self.heights[index + 1] = PaneWidth::Manual((pair_bottom - applied) / column.h);
        }
        let changed = self.heights[index] != upper || self.heights[index + 1] != lower;
        let applied = if changed {
            centre(split(column.y, column.h, &self.heights).dividers[index])
        } else {
            previous_y
        };
        Ok(ContextPairResult {
            fraction: fraction(applied),
            changed,
        })
    }
}
