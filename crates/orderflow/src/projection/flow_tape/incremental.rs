//! The spheres of an unchanged view, merged and finished again only in the
//! region windows a print joined since the last projection.
//!
//! FLOW merges inside region windows of [`FLOW_REGION_WINDOW_MS`], never
//! across them. Under one view and one opening selection a new print changes
//! only its window's groups. Fixed references retain their radii; a visible
//! maximum with a floor resizes the cloned output at its common reference.
//! The live edge merges one window rather than every candle on screen, and
//! walks only the cells from that window's first slot.
//! The frame is exactly the one
//! [`FlowTapeSource::project`] builds.
use super::super::{
    Admission, FLOW_REGION_WINDOW_MS, FlowScaleBasis, FlowTapeDot, FlowTapeFrame, FlowTapeView,
    Group, admit, finish_group, merge_window,
};
use super::{FlowOpeningSelection, FlowTapeSource, cell_group, valid};
use rust_decimal::Decimal;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// A native cell's key: slot, native window, price.
type CellKey = (usize, i64, Decimal);

/// One region window's finished spheres.
#[derive(Default)]
struct WindowSpheres {
    /// The lowest slot holding one of the window's cells.
    first_slot: usize,
    merged: Vec<FlowTapeDot>,
    /// Oversized native cells, drawn after every merged sphere in key order.
    standalone: Vec<(CellKey, FlowTapeDot)>,
    off_axis: usize,
    offscreen: usize,
    opening: bool,
}

/// The spheres of the last cached projection, and what they were built for.
pub(super) struct FlowMemo {
    view: FlowTapeView,
    opening_windows: Vec<i64>,
    opening_ordinals: Vec<usize>,
    windows: BTreeMap<i64, WindowSpheres>,
}

impl FlowMemo {
    fn holds(&self, view: FlowTapeView, openings: FlowOpeningSelection<'_>) -> bool {
        self.view == view
            && self.opening_windows == openings.windows
            && self.opening_ordinals == openings.ordinals
    }
}

/// Merge and finish one region window's groups, given in cell key order.
fn finish_window(
    cells: Vec<(CellKey, Group)>,
    view: FlowTapeView,
    reference: Decimal,
) -> WindowSpheres {
    let mut spheres = WindowSpheres {
        first_slot: cells.first().map_or(0, |((slot, ..), _)| *slot),
        ..WindowSpheres::default()
    };
    let mut mergeable = Vec::new();
    let mut standalone = Vec::new();
    for (key, group) in cells {
        match admit(&group, view) {
            Admission::OffAxis(executions) => spheres.off_axis += executions,
            Admission::Offscreen(executions) => spheres.offscreen += executions,
            Admission::Mergeable => mergeable.push(group),
            Admission::Standalone => standalone.push((key, group)),
        }
    }
    let merged = if mergeable.is_empty() {
        Vec::new()
    } else {
        merge_window(mergeable, view)
    };
    spheres.opening = merged
        .iter()
        .chain(standalone.iter().map(|(_, group)| group))
        .any(|group| group.opening_anchor);
    // A sphere is oversized only with an opening anchor of its own, which
    // makes its window's opening the frame's: deciding it per window is exact.
    let exclusion = view.exclude_opening && spheres.opening;
    spheres.merged = merged
        .into_iter()
        .map(|group| finish_group(group, view, reference, exclusion))
        .collect();
    spheres.standalone = standalone
        .into_iter()
        .map(|(key, group)| (key, finish_group(group, view, reference, exclusion)))
        .collect();
    spheres
}

impl FlowTapeSource {
    /// The spheres of `view`'s region windows, of `only` those when given,
    /// whose cells all lie at or after its slot.
    fn window_spheres(
        &self,
        view: FlowTapeView,
        openings: FlowOpeningSelection<'_>,
        reference: Decimal,
        only: Option<(&BTreeSet<i64>, usize)>,
    ) -> BTreeMap<i64, WindowSpheres> {
        let mut windows: BTreeMap<i64, Vec<(CellKey, Group)>> = BTreeMap::new();
        let from = only.map_or(view.first_slot, |(_, from)| from);
        let from = from.clamp(view.first_slot, view.end_slot);
        let cells = (from, i64::MIN, Decimal::MIN)..(view.end_slot, i64::MIN, Decimal::MIN);
        for (&key, cell) in self.cells.range(cells) {
            #[cfg(test)]
            self.cells_walked
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let window = cell
                .mark
                .first_timestamp_ms
                .div_euclid(FLOW_REGION_WINDOW_MS);
            if only.is_some_and(|(only, _)| !only.contains(&window)) {
                continue;
            }
            windows
                .entry(window)
                .or_default()
                .push((key, cell_group(key.0, key.1, cell, view, openings)));
        }
        windows
            .into_iter()
            .map(|(window, cells)| (window, finish_window(cells, view, reference)))
            .collect()
    }

    /// [`Self::project`] for a worker that keeps its projections. Under a
    /// configured reference, with the view and openings of the last call, only
    /// the region windows a print joined since then are merged again. A visible
    /// floor resizes the output without regrouping; the frame is the one
    /// [`Self::project`] builds. Every other
    /// projection is [`Self::project`] itself.
    pub fn project_cached(
        &mut self,
        epoch: u64,
        layout_revision: u64,
        source_count: usize,
        requested: Range<usize>,
        view: FlowTapeView,
        openings: FlowOpeningSelection<'_>,
    ) -> FlowTapeFrame {
        let dirty = std::mem::take(&mut self.dirty);
        let dirty_from = self.dirty_from.take();
        let Some(reference) = view.reference.typed().filter(|_| valid(view)) else {
            self.memo = None;
            return self.project(
                epoch,
                layout_revision,
                source_count,
                requested,
                view,
                openings,
            );
        };
        let memo = match self.memo.take() {
            Some(mut memo) if memo.holds(view, openings) => {
                // A dirty window's cells start at its first slot, or at a
                // print that joined it lower; a new window's are all new.
                let mut from = dirty_from.unwrap_or(view.end_slot);
                for window in &dirty {
                    let held = memo.windows.remove(window);
                    from = held.map_or(from, |held| from.min(held.first_slot));
                }
                let only = Some((&dirty, from));
                memo.windows
                    .extend(self.window_spheres(view, openings, reference, only));
                memo
            }
            _ => FlowMemo {
                view,
                opening_windows: openings.windows.to_vec(),
                opening_ordinals: openings.ordinals.to_vec(),
                windows: self.window_spheres(view, openings, reference, None),
            },
        };
        let mut frame = self.frame_shell(epoch, layout_revision, source_count, requested, view);
        let mut standalone: Vec<&(CellKey, FlowTapeDot)> = Vec::new();
        let mut opening = false;
        for spheres in memo.windows.values() {
            frame.dots.extend(spheres.merged.iter().cloned());
            standalone.extend(&spheres.standalone);
            frame.off_axis_executions += spheres.off_axis;
            frame.offscreen_executions += spheres.offscreen;
            opening |= spheres.opening;
        }
        standalone.sort_by_key(|(key, _)| *key);
        frame
            .dots
            .extend(standalone.into_iter().map(|(_, dot)| dot.clone()));
        if !frame.dots.is_empty() {
            frame.effective_reference = Some(reference);
            frame.scale_basis = FlowScaleBasis::TypedReference;
            frame.opening_exclusion_effective = view.exclude_opening && opening;
        }
        frame.rescale_visible_floor();
        frame.order_for_paint();
        self.memo = Some(memo);
        frame
    }
}
