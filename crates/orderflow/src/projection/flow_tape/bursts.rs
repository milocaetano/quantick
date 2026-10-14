//! Causal tape grouping in declared market-time geometry, then exact candle placement.
use super::*;
use crate::constants::NATIVE_TAPE_WINDOW_MS;
use crate::projection::{TapeDotMemory, TapeDotView, tape_group::NativeKey};
use rust_decimal::prelude::FromPrimitive as _;

#[cfg(test)]
#[path = "tests/bursts.rs"]
mod tests;

#[derive(Default)]
pub(super) struct BurstCache {
    calibration: Option<crate::TickBurstGrouping>,
    openings: (Vec<i64>, Vec<usize>),
    blocks: BTreeMap<i64, CachedBlock>,
    #[cfg(test)]
    pub(super) passes: usize,
}

#[derive(Default, Clone)]
struct CachedBlock {
    // Source edits invalidate by native window; these bounds also guard a
    // caller changing admission without appending any new retained records.
    admitted: Option<(usize, usize, usize, usize)>,
    through_ms: i64,
    memory: TapeDotMemory,
    dirty: bool,
    groups: Vec<Group>,
}

impl BurstCache {
    pub(super) fn changed(&mut self, window: i64) {
        if let Some(calibration) = self.calibration
            && let Some(block) = self
                .blocks
                .get_mut(&window.div_euclid(calibration.window_ms))
        {
            block.dirty = true;
        }
    }

    pub(super) fn evicted(&mut self, window: i64) {
        if let Some(calibration) = self.calibration {
            self.blocks
                .remove(&window.div_euclid(calibration.window_ms));
        }
    }

    pub(super) fn project(
        &mut self,
        cells: &BTreeMap<(usize, i64, Decimal), NativeCell>,
        burst: FlowBurstView,
        view: FlowTapeView,
        openings: FlowOpeningSelection<'_>,
        complete_admission: bool,
    ) -> (Vec<Group>, usize) {
        if self.calibration != Some(burst.calibration) {
            self.blocks.clear();
            self.calibration = Some(burst.calibration);
        }
        if self.openings.0 != openings.windows || self.openings.1 != openings.ordinals {
            for held in self.blocks.values_mut() {
                held.dirty = true;
            }
            self.openings = (openings.windows.to_vec(), openings.ordinals.to_vec());
        }
        let mut blocks = BTreeMap::<i64, Vec<_>>::new();
        let mut outside = 0;
        for (&(slot, window, _), cell) in cells.range(
            (view.first_slot, i64::MIN, Decimal::MIN)..(view.end_slot, i64::MIN, Decimal::MIN),
        ) {
            if !(burst.from_ms..burst.until_ms).contains(&window) {
                outside += cell.members.len();
                continue;
            }
            let index = window.div_euclid(burst.calibration.window_ms);
            blocks.entry(index).or_default().push((slot, window, cell));
        }
        let mut shown = Vec::new();
        for (index, cells) in blocks {
            let admitted = (
                cells.len(),
                cells.iter().map(|(_, _, cell)| cell.members.len()).sum(),
                cells
                    .iter()
                    .map(|(_, _, cell)| cell.first_ordinal)
                    .min()
                    .unwrap_or(0),
                cells
                    .iter()
                    .filter_map(|(_, _, cell)| cell.members.last().map(|member| member.ordinal))
                    .max()
                    .unwrap_or(0),
            );
            // A partial source page is a preview, never a watermark for retained groups.
            let mut preview = if complete_admission {
                CachedBlock::default()
            } else {
                self.blocks.get(&index).cloned().unwrap_or_default()
            };
            let held = if complete_admission {
                self.blocks.entry(index).or_default()
            } else {
                &mut preview
            };
            if held.admitted.is_some_and(|old| {
                admitted.0 < old.0
                    || admitted.1 < old.1
                    || admitted.2 != old.2
                    || admitted.3 < old.3
            }) {
                *held = CachedBlock::default();
            }
            let through_ms = burst.settled_through_ms.min(
                index
                    .saturating_add(1)
                    .saturating_mul(burst.calibration.window_ms),
            );
            if held.dirty || held.admitted != Some(admitted) || held.through_ms != through_ms {
                #[cfg(test)]
                {
                    self.passes += 1;
                }
                held.groups = settle(
                    &mut held.memory,
                    &cells,
                    burst.calibration,
                    view,
                    openings,
                    through_ms,
                );
                held.admitted = Some(admitted);
                held.through_ms = through_ms;
                held.dirty = false;
            }
            for mut group in held.groups.iter().cloned() {
                let mut mark = group.moment.finish(Decimal::ONE);
                let position = group.position_quantity / mark.quantity;
                mark.x = ((position - Decimal::from(view.first_slot))
                    / Decimal::from(view.end_slot - view.first_slot))
                .to_f64()
                .unwrap_or_default();
                mark.y = view.prices.y_unclamped(mark.price).unwrap_or_default();
                group.moment = TapeMoment::new(mark, f64::INFINITY);
                shown.push(group);
            }
        }
        (shown, outside)
    }
}

fn settle(
    memory: &mut TapeDotMemory,
    cells: &[(usize, i64, &NativeCell)],
    calibration: crate::TickBurstGrouping,
    view: FlowTapeView,
    openings: FlowOpeningSelection<'_>,
    through_ms: i64,
) -> Vec<Group> {
    // A native key may straddle candle boundaries. Consolidate it before passing
    // facts to the tape's unique-key map; the sidecar keeps every source ordinal.
    let mut native = BTreeMap::<NativeKey, Group>::new();
    for &(slot, window, cell) in cells {
        let group = cell_group(slot, window, cell, view, openings);
        let key = NativeKey(window, cell.mark.price_bucket);
        match native.entry(key) {
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.get_mut().absorb(group, f64::INFINITY);
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(group);
            }
        }
    }
    let facts: Vec<_> = native
        .values()
        .map(|group| {
            let mut mark = group.moment.clone().finish(Decimal::ONE);
            mark.live = true;
            mark
        })
        .collect();
    if facts.is_empty() {
        return Vec::new();
    }
    // A constant two-point coordinate frame fixes the physical price scale.
    // The shared merger expands it around actual prices without changing that scale.
    let unit = Decimal::from_f64(calibration.price_units_per_pixel).expect("validated price unit");
    let prices = PriceWindow::new(-unit, unit).expect("validated positive price unit");
    let tape_view = TapeDotView {
        now_ms: through_ms,
        window_ms: calibration.window_ms,
        dot_window_ms: NATIVE_TAPE_WINDOW_MS,
        evicted_through_ms: None,
        prices,
        geometry: TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: calibration.width_px,
            height_px: 2.0,
        },
    };
    let sizing = DotSizing {
        native_tape: true,
        tape_column_px: 0.0,
        candle_column_px: 0.0,
        px_per_price: 0.0,
        typed_full: None,
    };
    let bubbles = BubbleStyle {
        max_radius: calibration.max_radius,
        ..Default::default()
    };
    let closed: Vec<_> = facts
        .iter()
        .filter(|mark| {
            RecordedOpenings::window_start(mark.last_timestamp_ms)
                .saturating_add(NATIVE_TAPE_WINDOW_MS)
                <= through_ms
        })
        .cloned()
        .collect();
    let mut groups = memory.project_native_block(
        &closed,
        tape_view,
        sizing,
        &bubbles,
        &LiveLaneStyle::default(),
    );
    if closed.len() != facts.len() {
        let mut preview = memory.clone();
        let now_ms = facts
            .iter()
            .map(|mark| RecordedOpenings::window_start(mark.last_timestamp_ms))
            .max()
            .unwrap_or(through_ms)
            .saturating_add(NATIVE_TAPE_WINDOW_MS);
        groups = preview.project_native_block(
            &facts,
            TapeDotView {
                now_ms,
                ..tape_view
            },
            sizing,
            &bubbles,
            &LiveLaneStyle::default(),
        );
    }
    groups
        .into_iter()
        .map(|group| {
            let mut parts = group
                .sources
                .keys()
                .map(|key| native.remove(key).expect("native sidecar"));
            let mut sidecar = parts.next().expect("nonempty native group");
            for part in parts {
                sidecar.absorb(part, f64::INFINITY);
            }
            let mut mark = group.mark;
            mark.live = false;
            sidecar.moment = TapeMoment::new(mark, f64::INFINITY);
            sidecar
        })
        .collect()
}

pub(super) fn finish(
    groups: Vec<Group>,
    view: FlowTapeView,
) -> (
    Vec<FlowTapeDot>,
    Option<Decimal>,
    usize,
    usize,
    FlowScaleBasis,
    bool,
) {
    let mut off_axis = 0;
    let mut offscreen = 0;
    let groups = groups
        .into_iter()
        .filter(|group| {
            if !(0.0..=1.0).contains(&group.y()) {
                off_axis += group.members.len();
                return false;
            }
            let position = group.position_quantity / group.quantity();
            if position < view.clip_left || position > view.clip_right {
                offscreen += group.members.len();
                return false;
            }
            true
        })
        .collect();
    // Never merge again in compressed candle coordinates.
    scale_groups(groups, view, off_axis, offscreen)
}
