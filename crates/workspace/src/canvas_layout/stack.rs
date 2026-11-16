//! The context stack beside the flow pane: the heights the trader gave its
//! charts, the column it was last drawn in, and the one operation a divider
//! drag and a remote caller share, with its floors.
//!
//! Height choices survive layouts that hide the stack; the drawn frame only
//! describes a stack on screen, so an action can refuse a stale one. Which
//! chart is folded is the canvas model's: [`on_stack`] runs every change of
//! the stack folded as the model says and gives the model the folds it left.

use super::split::{Span, centre};
use super::{
    CANVAS_DIVIDER_PX, COLLAPSE_AT_PX, COLLAPSED_PANE_WIDTH_PX, MAX_CONTEXT_PANES,
    MIN_PANE_WIDTH_PX, PaneWidth, resolve_shares, split,
};
use crate::canvas::{CanvasRect, CanvasState, Hand, PaneSlot, Seats};

/// One tab's stacked context charts.
#[derive(Debug, Default)]
pub struct ContextStack {
    pub heights: Vec<PaneWidth>,
    pub frame: Option<StackFrame>,
    /// The hand on a divider, while one is held.
    hold: Option<StackHold>,
    /// The model last folded every chart: the column's fold, inside which
    /// the stack keeps its own folds for the column to reopen with.
    whole: bool,
}

/// A hand on the divider below chart `index`: where the divider stood at
/// the press, the pointer's travel since, and the heights then.
#[derive(Debug)]
struct StackHold {
    index: usize,
    at: f32,
    travel: f32,
    heights: Vec<PaneWidth>,
}

/// How far from its press a hand may be let go and still count as back
/// where it started.
const BACK_PX: f32 = 0.5;

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

    /// Reopen every folded chart at the height it had ([`Self::open`]).
    pub fn expand(&mut self) {
        (0..self.heights.len()).for_each(|slot| self.open(slot));
    }

    /// Reopen chart `slot` at the height it had, if folded; a folded pair
    /// gives its partner back the rest.
    fn open(&mut self, slot: usize) {
        let Some(PaneWidth::Collapsed { restore }) = self.heights.get(slot).copied() else {
            return;
        };
        self.heights[slot] = PaneWidth::Manual(restore);
        if self.heights.len() == 2 {
            self.heights[1 - slot] = PaneWidth::Manual(1.0 - restore);
        }
    }

    /// Hold at least `count` charts, a new one sized automatically.
    pub fn at_least(&mut self, count: usize) {
        if self.heights.len() < count {
            self.heights.resize(count, PaneWidth::Auto);
        }
    }

    /// Move chart `from` to `to`, its height going with it.
    pub fn move_chart(&mut self, from: usize, to: usize) {
        if from < self.heights.len() && to < self.heights.len() {
            let height = self.heights.remove(from);
            self.heights.insert(to, height);
        }
    }

    /// Fold the charts as the canvas model folds them, `folded[i]` for
    /// chart `i`: one the model rails folds at the share it holds, a pair's
    /// partner taking the rest; one the model opened reopens at the height it
    /// had ([`Self::open`]). The model folding every chart is the column's
    /// fold, which leaves the stack as it is, and the column reopening
    /// keeps the stack's own folds: [`on_stack`] gives them back.
    pub fn follow(&mut self, folded: &[bool]) {
        let count = folded.len().min(self.heights.len());
        let whole = folded[..count].iter().all(|folded| *folded);
        if std::mem::replace(&mut self.whole, whole) || whole {
            return;
        }
        let px = self.frame.as_ref().map_or(0.0, |frame| frame.column.h);
        let mut shares = [0.0_f32; MAX_CONTEXT_PANES];
        resolve_shares(&self.heights[..count], px, &mut shares[..count]);
        for (slot, folded) in folded[..count].iter().enumerate() {
            match (*folded, self.heights[slot]) {
                (true, PaneWidth::Collapsed { .. })
                | (false, PaneWidth::Auto | PaneWidth::Manual(_)) => {}
                (true, _) => {
                    self.heights[slot] = PaneWidth::Collapsed {
                        restore: shares[slot],
                    };
                    if count == 2 {
                        self.heights[1 - slot] = PaneWidth::Auto;
                    }
                }
                (false, PaneWidth::Collapsed { .. }) => self.open(slot),
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

    /// The hand on the divider below chart `index`, drawn centred at `at`,
    /// one frame: the boundary it asks for, the divider's place at the press
    /// plus the pointer's travel since, so a grab off the divider's centre
    /// never jumps it; `None` on a frame it did not move. Let go back where
    /// it was pressed, the heights are as they were at the press.
    pub fn hand(&mut self, index: usize, hand: Hand, at: f32) -> Option<f32> {
        let ours = |hold: &StackHold| hold.index == index;
        if hand.pressed || (hand.held && !self.hold.as_ref().is_some_and(ours)) {
            let heights = self.heights.clone();
            let travel = 0.0;
            self.hold = Some(StackHold {
                index,
                at,
                travel,
                heights,
            });
        }
        if hand.released
            && let Some(hold) = self.hold.take_if(|hold| ours(hold))
            && hold.travel.abs() < BACK_PX
        {
            self.heights = hold.heights;
        }
        let hold = self.hold.as_mut().filter(|hold| ours(hold) && hand.held)?;
        (hand.delta.abs() > f32::EPSILON && hand.delta.is_finite()).then(|| {
            hold.travel += hand.delta;
            hold.at + hold.travel
        })
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
        // A fold by the hand keeps the boundary its press found on an open
        // pair, not the floor the drag passed.
        let open_at_press = |hold: &&StackHold| {
            let open = |slot: usize| hold.heights.get(slot).is_some_and(|at| !at.is_collapsed());
            hold.index == index && open(index) && open(index + 1)
        };
        let kept_y = (self.hold.as_ref().filter(open_at_press)).map_or(previous_y, |hold| hold.at);
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
            let restore = (kept_y - pair_top) / column.h;
            self.heights[index] = PaneWidth::Collapsed { restore };
            self.heights[index + 1] = PaneWidth::Manual(pair_share - collapsed_share(index));
        } else if collapse_lower && open {
            let restore = (pair_bottom - kept_y) / column.h;
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

/// The column painter's stack and the canvas model as one: `op` runs on
/// `stack` folded as `state` folds the charts `seats` seats in it, top to
/// bottom, and the folds it leaves are `state`'s. The model holds which
/// chart is folded, the stack the heights. A layout the column painter does
/// not draw (`columns` false) keeps its folds on its tree.
pub fn on_stack<R>(
    stack: &mut ContextStack,
    (state, seats, columns): (&mut CanvasState, &mut Seats, bool),
    op: impl FnOnce(&mut ContextStack) -> R,
) -> R {
    seats.refresh(state);
    let slots = stack_slots(state, seats);
    if !columns || slots.len() < 2 {
        return op(stack);
    }
    stack.at_least(slots.len());
    let folded: Vec<bool> = (slots.iter())
        .map(|slot| state.railed().contains(slot))
        .collect();
    stack.follow(&folded);
    let out = op(stack);
    let mut folded = stack.collapsed_slots();
    folded.resize(slots.len(), false);
    state.rail_stack(&slots, &folded);
    out
}

/// The slots of `state`'s stack by the built chart `seats` seats each on:
/// the stack's charts top to bottom as the column painter draws them.
fn stack_slots(state: &CanvasState, seats: &Seats) -> Vec<PaneSlot> {
    let stack = state.stack_slots();
    let mut held: Vec<(usize, PaneSlot)> = (seats.held().iter())
        .filter(|(slot, _)| stack.contains(slot))
        .map(|(slot, seat)| (*seat, *slot))
        .collect();
    held.sort_unstable();
    let seated = held.iter().enumerate();
    let seated = seated.take_while(|(at, (seat, _))| at == seat);
    seated.map(|(_, (_, slot))| *slot).collect()
}
