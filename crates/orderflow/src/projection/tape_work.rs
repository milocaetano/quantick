//! What one frame's tape asks of a [`TapeDotMemory`], priced before any of
//! it runs, and what the memory draws while that work runs elsewhere.
//!
//! A frame reads every native cell it is handed and merges every closed
//! native window among them the memory has not merged yet, each against
//! the frontier. A steady frame is a few windows and the cells after the
//! worker's seal; a zoom, a new lineage, a stale memory or a stalled worker
//! can make it the whole window. [`TapeWork::fits_frame`] tells the two
//! apart, so a painter can run the second where no frame waits for it
//! ([`crate::projection::TapeRebuilds`]) and draw [`TapeDotMemory::draw_retained`]
//! meanwhile.

use super::{
    AggressionCluster, AggressionPrimitive, BubbleStyle, DotSizing, Group, LiveLaneStyle,
    TapeDotFrame, TapeDotMemory, TapeDotView, TapeSource, keys_native_cells, window_start,
};
use crate::projection::constants::{
    COMPLETE_CELL_UNITS, FRAME_WORK_BUDGET, REREAD_FRONTIER, SEALED_CELL_UNITS, WINDOW_UNITS,
};

/// What projecting one frame asks of a [`TapeDotMemory`], measured without
/// doing it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TapeWork {
    /// Every cell in the window is read again, not only the newest.
    pub rereads: bool,
    /// A new display epoch: the memory drops everything it holds first.
    pub starts_over: bool,
    /// The native cells handed to reconciliation; a reread's is the window's.
    pub cells: usize,
    /// The closed native windows among those cells the memory has not
    /// merged yet: each is merged against the frontier.
    pub windows: usize,
    /// The groups each of those windows is merged against.
    pub frontier: usize,
    /// What reading one of the cells costs, in units.
    pub cell_units: usize,
}

impl TapeWork {
    /// The work's price, about a microsecond a unit.
    #[must_use]
    pub fn units(&self) -> usize {
        self.cells.saturating_mul(self.cell_units).saturating_add(
            self.windows
                .saturating_mul(self.frontier.saturating_add(WINDOW_UNITS)),
        )
    }

    /// Whether a frame can do this work itself ([`FRAME_WORK_BUDGET`]).
    #[must_use]
    pub fn fits_frame(&self) -> bool {
        self.units() <= FRAME_WORK_BUDGET
    }
}

impl TapeDotMemory {
    /// The work [`Self::project`] would do at `view` for `marks`: every live
    /// mark in the window is read again on every frame.
    #[must_use]
    pub fn complete_work(
        &self,
        marks: &[AggressionPrimitive],
        view: TapeDotView,
        sizing: DotSizing,
    ) -> TapeWork {
        if !keys_native_cells(sizing, view) {
            return TapeWork::default();
        }
        let from = view.now_ms.saturating_sub(view.window_ms);
        let windows = marks
            .iter()
            .filter(|mark| mark.live)
            .map(|mark| window_start(mark.first_timestamp_ms, view.dot_window_ms))
            .filter(|window| {
                *window >= from
                    && view
                        .evicted_through_ms
                        .is_none_or(|horizon| *window > horizon)
            });
        let mut work = self.priced(true, self.starts_over(view), windows, view);
        work.cell_units = COMPLETE_CELL_UNITS;
        work
    }

    /// The work [`Self::project_sealed`] would do at `view`; `None` where it
    /// would decline for the complete path. A reread is counted as the
    /// window's cells, which are the ones it folds into groups.
    #[must_use]
    pub fn sealed_work(
        &self,
        source: TapeSource<'_>,
        view: TapeDotView,
        sizing: DotSizing,
    ) -> Option<TapeWork> {
        let seal = source.facts.seal.as_ref()?;
        if !keys_native_cells(sizing, view) || seal.window_ms != view.dot_window_ms {
            return None;
        }
        let starts_over = self.starts_over(view);
        let below = (!starts_over)
            .then(|| self.reconciled_below(source, view))
            .flatten();
        let from = below.unwrap_or_else(|| view.now_ms.saturating_sub(view.window_ms));
        let window =
            |cell: &AggressionCluster| window_start(cell.first_timestamp_ms, view.dot_window_ms);
        let cells = &source.facts.clusters;
        let first = cells.partition_point(|cell| window(cell) < from);
        let overlay = source
            .overlay
            .map_or(&[][..], |overlay| overlay.cells.as_slice());
        let windows = cells[first..].iter().chain(overlay).map(window);
        Some(self.priced(below.is_none(), starts_over, windows, view))
    }

    /// The work of reading the cells whose windows `windows` lists, and of
    /// merging each closed window among them the memory has not merged yet.
    fn priced(
        &self,
        rereads: bool,
        starts_over: bool,
        windows: impl Iterator<Item = i64>,
        view: TapeDotView,
    ) -> TapeWork {
        let merged = self.merged_through.filter(|_| !starts_over);
        let closed_before = view.now_ms.saturating_sub(view.dot_window_ms.max(1));
        let mut cells = 0;
        let mut unmerged = Vec::new();
        for window in windows {
            cells += 1;
            // Published cells come in window order: a run of one window is
            // one entry, and what is left to sort is short and nearly sorted.
            if window <= closed_before
                && merged.is_none_or(|merged| window > merged)
                && unmerged.last() != Some(&window)
            {
                unmerged.push(window);
            }
        }
        unmerged.sort_unstable();
        unmerged.dedup();
        let frontier = if starts_over { 0 } else { self.frontier.len() };
        TapeWork {
            rereads,
            starts_over,
            cells,
            windows: unmerged.len(),
            frontier: if rereads {
                frontier.max(REREAD_FRONTIER)
            } else {
                frontier
            },
            cell_units: SEALED_CELL_UNITS,
        }
    }

    /// The frame the retained groups draw at `view`, reconciling nothing and
    /// keeping nothing: what a painter shows while this memory's
    /// reconciliation runs beside the frame. Groups leave the window as they
    /// would; nothing new joins them. `marks` supply only the candle marks,
    /// which pass through.
    #[must_use]
    pub fn draw_retained(
        &self,
        marks: &[AggressionPrimitive],
        view: TapeDotView,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        opening_bursts: &[i64],
    ) -> TapeDotFrame {
        if !keys_native_cells(sizing, view) {
            return TapeDotFrame {
                marks: marks.to_vec(),
                max_radius: bubbles.max_radius,
                full_quantity: sizing.full_quantity(marks, true),
            };
        }
        let kept: Vec<&Group> = self
            .settled
            .iter()
            .chain(&self.frontier)
            .filter(|group| group.retained_at(view.now_ms, view.window_ms))
            .collect();
        let reference_quantities = (!opening_bursts.is_empty() && sizing.typed_full.is_none())
            .then(|| {
                kept.iter()
                    .map(|group| group.reference_quantity(opening_bursts))
                    .collect()
            });
        let shown = kept.iter().map(|group| group.mark.clone()).collect();
        self.draw(
            shown,
            reference_quantities,
            marks,
            view,
            sizing,
            bubbles,
            lane,
        )
    }
}
