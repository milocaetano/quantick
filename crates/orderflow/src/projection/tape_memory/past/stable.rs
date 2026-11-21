//! Stable native panes own one factual display history across navigation.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::super::{native_facts, window_start};
use super::{BlockInputs, PastTapeMemory, settle_block};
use crate::projection::constants::MAX_PAST_BLOCKS;
use crate::projection::{
    AggressionPrimitive, HeatmapProjection, PastTape, TapeDotFrame, TapeDotMemory, TapeDotView,
};

/// A held immutable publication needs one reconciliation. Navigation may
/// expose facts evicted from the bounded cache, so it also participates.
#[derive(Debug)]
pub(super) struct Source {
    projection: Arc<HeatmapProjection>,
    evicted: Option<i64>,
    retained_from: Option<i64>,
    now_ms: i64,
    window_ms: i64,
}

impl PastTapeMemory {
    /// Move the currently drawn live history here for a stable native pane.
    /// Its exact groups return through [`Self::resume_live`].
    pub fn hold_live(&mut self, mut memory: TapeDotMemory) {
        if self.stable_block_ms.is_none() {
            self.stable_block_ms = memory.epoch.map(|epoch| epoch.window_ms.max(1));
        }
        memory.frozen = true;
        memory.retained_bucket_ms = self.stable_block_ms;
        self.blocks.clear();
        self.stable_source = None;
        self.stable = Some(memory);
    }

    /// Return the same display history, including groups first seen in the
    /// past, so live projection cannot build a competing partition.
    pub fn resume_live(&mut self) -> Option<TapeDotMemory> {
        self.stable_source = None;
        self.stable.take().map(|mut memory| {
            memory.frozen = false;
            memory.ingested = None;
            memory
        })
    }

    pub(super) fn project_stable(
        &mut self,
        facts: &[AggressionPrimitive],
        tape: &PastTape,
        view: TapeDotView,
        mut inputs: BlockInputs<'_>,
    ) -> TapeDotFrame {
        if self
            .epoch
            .is_some_and(|epoch| epoch.dot_window_ms != view.dot_window_ms)
        {
            self.clear();
        }
        let bucket_ms = *self.stable_block_ms.get_or_insert(tape.block_ms);
        self.epoch = Some(inputs.epoch);
        let evicted = view.evicted_through_ms.max(
            tape.retained_from_ms
                .map(|from| window_start(from, view.dot_window_ms).saturating_sub(1)),
        );
        inputs.evicted_through_ms = evicted;
        let unchanged = self.stable_source.as_ref().is_some_and(|source| {
            Arc::ptr_eq(&source.projection, &tape.projection)
                && source.evicted == evicted
                && source.retained_from == tape.retained_from_ms
                && source.now_ms == view.now_ms
                && source.window_ms == view.window_ms
        });
        let memory = self.stable.get_or_insert_with(TapeDotMemory::default);
        memory.epoch = Some(view);
        memory.frozen = true;
        memory.ingested = None;
        memory.retained_ms = bucket_ms
            .saturating_mul(MAX_PAST_BLOCKS as i64)
            .max(view.window_ms);
        memory.retained_bucket_ms = Some(bucket_ms);
        // Ownership is keyed on native facts, independent of the worker's
        // window-sized fetch blocks. Existing groups consume their keys;
        // only genuinely changed facts refresh their exact quantities.
        if !unchanged {
            let mut incoming = native_facts(
                facts,
                TapeDotView {
                    now_ms: tape.until_ms,
                    window_ms: tape.until_ms.saturating_sub(tape.from_ms),
                    evicted_through_ms: evicted,
                    ..view
                },
            );
            memory.replace_facts(&mut incoming, evicted, None, true);
            let unowned: Vec<_> = incoming.into_values().cloned().collect();
            let first = tape.from_ms.div_euclid(tape.block_ms);
            let last = tape.until_ms.saturating_sub(1).div_euclid(tape.block_ms);
            for index in first..=last {
                let start = index.saturating_mul(tape.block_ms);
                let end = start.saturating_add(tape.block_ms);
                if start >= tape.from_ms && end <= tape.until_ms {
                    memory
                        .settled
                        .extend(settle_block(&unowned, start, end, &inputs));
                }
            }
            self.stable_source = Some(Source {
                projection: Arc::clone(&tape.projection),
                evicted,
                retained_from: tape.retained_from_ms,
                now_ms: view.now_ms,
                window_ms: view.window_ms,
            });
        }
        // Partial fetched blocks can be retained too: a later native-cell
        // replacement above updates facts without regrouping its neighbours.
        memory.trim_history(view, !unchanged);
        memory.draw_retained(
            &[],
            view,
            inputs.sizing,
            inputs.bubbles,
            inputs.lane,
            inputs.opening_bursts,
        )
    }
}

fn buckets(memory: &TapeDotMemory, block_ms: i64) -> BTreeSet<i64> {
    memory
        .settled
        .iter()
        .chain(&memory.frontier)
        .map(|group| group.mark.first_timestamp_ms.div_euclid(block_ms))
        .collect()
}

pub(super) fn bucket_count(memory: &TapeDotMemory, block_ms: i64) -> usize {
    buckets(memory, block_ms).len()
}

pub(super) fn trim(memory: &mut TapeDotMemory, block_ms: i64, view: TapeDotView) {
    let first = view
        .now_ms
        .saturating_sub(view.window_ms)
        .div_euclid(block_ms);
    let last = view.now_ms.div_euclid(block_ms);
    let visible: BTreeSet<_> = memory
        .settled
        .iter()
        .chain(&memory.frontier)
        .filter(|group| {
            group.visible_at(view.now_ms, view.window_ms)
                && group.mean_ms() <= rust_decimal::Decimal::from(view.now_ms)
        })
        .map(|group| group.mark.first_timestamp_ms.div_euclid(block_ms))
        .collect();
    let mut held: BTreeSet<_> = buckets(memory, block_ms)
        .difference(&visible)
        .copied()
        .collect();
    while held.len() > MAX_PAST_BLOCKS {
        let oldest = *held.first().expect("a populated cache has a first bucket");
        let newest = *held.last().expect("a populated cache has a last bucket");
        let far = if first.saturating_sub(oldest) >= newest.saturating_sub(last) {
            oldest
        } else {
            newest
        };
        held.remove(&far);
        memory
            .settled
            .retain(|group| group.mark.first_timestamp_ms.div_euclid(block_ms) != far);
        memory
            .frontier
            .retain(|group| group.mark.first_timestamp_ms.div_euclid(block_ms) != far);
    }
}
