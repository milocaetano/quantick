//! Past tape groups, retaining the partition already drawn by the pane.
//!
//! Stable native panes carry one factual display history between live and
//! past. New facts use a cold causal pass over their fetch block; retained
//! groups survive view transforms. Other panes keep the existing frozen
//! block cache on the window's grid, rebuilt when its display epoch changes.

use std::collections::BTreeMap;

use rust_decimal::prelude::ToPrimitive as _;

use super::{Group, TapeDotFrame, TapeDotMemory, TapeDotView, window_start};
use crate::config::{BubbleStyle, LiveLaneStyle};
use crate::projection::constants::{MAX_PAST_BLOCKS, PAST_PRICE_SPAN_BAND};
use crate::projection::{AggressionPrimitive, DotSizing, PastTape, PriceWindow, TapeDotGeometry};

#[path = "tape_past_stable.rs"]
mod stable;

#[derive(Debug, Clone, Copy, PartialEq)]
struct PastEpoch {
    window_ms: i64,
    dot_window_ms: i64,
    block_ms: i64,
    width_px: f32,
    height_px: f32,
    prices: PriceWindow,
}

impl PastEpoch {
    fn from_view(view: TapeDotView, block_ms: i64) -> Self {
        Self {
            window_ms: view.window_ms,
            dot_window_ms: view.dot_window_ms,
            block_ms,
            width_px: view.geometry.width_px,
            height_px: view.geometry.height_px,
            prices: view.prices,
        }
    }
}

/// Per-pane past history. Stable native panes transfer their live groups;
/// other panes use an independent cache of frozen blocks.
#[derive(Debug, Default)]
pub struct PastTapeMemory {
    epoch: Option<PastEpoch>,
    blocks: BTreeMap<i64, Vec<Group>>,
    /// Stable panes move their one display history between live and past.
    stable: Option<TapeDotMemory>,
    stable_block_ms: Option<i64>,
    stable_source: Option<stable::Source>,
}

/// The inputs every block of one epoch is merged with.
struct BlockInputs<'a> {
    epoch: PastEpoch,
    evicted_through_ms: Option<i64>,
    sizing: DotSizing,
    bubbles: &'a BubbleStyle,
    lane: &'a LiveLaneStyle,
    opening_bursts: &'a [i64],
}

impl TapeDotMemory {
    /// The same offscreen budget follows stable groups back to live. A held
    /// unchanged frame needs no bucket walk.
    pub(super) fn trim_history(&mut self, view: TapeDotView, changed: bool) {
        let Some(block_ms) = self.retained_bucket_ms else {
            return;
        };
        let position = (view.now_ms, view.window_ms);
        if changed || self.retained_trim_at != Some(position) {
            stable::trim(self, block_ms, view);
            self.retained_trim_at = Some(position);
        }
    }
}

impl PastTapeMemory {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    #[must_use]
    pub fn cached_block_count(&self) -> usize {
        self.stable.as_ref().map_or(self.blocks.len(), |memory| {
            stable::bucket_count(memory, self.stable_block_ms.unwrap_or(1))
        })
    }

    /// The frame at `view.now_ms`, a past instant, from `facts` — the
    /// native facts of `tape`'s blocks. A block the facts do not cover yet
    /// (the worker still answering an older end) is drawn once they do. A
    /// frozen block is drawn only while the retained tape holds all of it.
    #[allow(clippy::too_many_arguments)]
    pub fn project(
        &mut self,
        facts: &[AggressionPrimitive],
        tape: &PastTape,
        view: TapeDotView,
        sizing: DotSizing,
        bubbles: &BubbleStyle,
        lane: &LiveLaneStyle,
        opening_bursts: &[i64],
    ) -> TapeDotFrame {
        let block_ms = tape.block_ms;
        if !sizing.native_tape || !view.geometry.valid() || view.window_ms <= 0 || block_ms <= 0 {
            return TapeDotMemory::default().project(
                facts,
                view,
                sizing,
                bubbles,
                lane,
                opening_bursts,
            );
        }
        let mut inputs = BlockInputs {
            epoch: PastEpoch::from_view(view, block_ms),
            evicted_through_ms: view.evicted_through_ms,
            sizing,
            bubbles,
            lane,
            opening_bursts,
        };
        if lane.stable_size && sizing.typed_full.is_some() {
            return self.project_stable(facts, tape, view, inputs);
        }
        self.stable = None;
        self.stable_block_ms = None;
        self.stable_source = None;
        inputs.epoch = self.prepare(view, block_ms);
        let first = view
            .now_ms
            .saturating_sub(view.window_ms)
            .div_euclid(block_ms);
        let last = view.now_ms.div_euclid(block_ms);
        let mut shown = Vec::new();
        for index in first..=last {
            let start = index.saturating_mul(block_ms);
            let end = start.saturating_add(block_ms);
            // Eviction that reached into a frozen block left it holding prints
            // the tape no longer has: it goes, and the facts settle what is left.
            let whole = tape.retained_from_ms.is_some_and(|from| start >= from);
            if !whole {
                self.blocks.remove(&index);
            } else if let Some(groups) = self.blocks.get(&index) {
                shown.extend(groups.iter().cloned());
                continue;
            }
            if start < tape.from_ms || end > tape.until_ms {
                continue;
            }
            let groups = settle_block(facts, start, end, &inputs);
            // Frozen only once no print can still join it, and only whole.
            if end <= tape.settled_through_ms && whole {
                self.blocks.insert(index, groups.clone());
            }
            shown.extend(groups);
        }
        while self.blocks.len() > MAX_PAST_BLOCKS {
            let (oldest, newest) = (
                *self.blocks.keys().next().expect("non-empty"),
                *self.blocks.keys().next_back().expect("non-empty"),
            );
            let far = if first - oldest >= newest - last {
                oldest
            } else {
                newest
            };
            self.blocks.remove(&far);
        }
        TapeDotMemory {
            epoch: Some(view),
            settled: shown,
            frontier: Vec::new(),
            frozen: true,
            ..TapeDotMemory::default()
        }
        .project(&[], view, sizing, bubbles, lane, opening_bursts)
    }

    fn prepare(&mut self, view: TapeDotView, block_ms: i64) -> PastEpoch {
        let span = |prices: PriceWindow| (prices.high - prices.low).to_f64().unwrap_or(0.0);
        let held = self.epoch.filter(|epoch| {
            let ratio = span(view.prices) / span(epoch.prices);
            epoch.window_ms == view.window_ms
                && epoch.dot_window_ms == view.dot_window_ms
                && epoch.block_ms == block_ms
                && epoch.width_px == view.geometry.width_px
                && epoch.height_px == view.geometry.height_px
                && ratio.is_finite()
                && (1.0 / PAST_PRICE_SPAN_BAND..=PAST_PRICE_SPAN_BAND).contains(&ratio)
        });
        held.unwrap_or_else(|| {
            let epoch = PastEpoch::from_view(view, block_ms);
            self.blocks.clear();
            self.epoch = Some(epoch);
            epoch
        })
    }
}

/// One cold causal pass over one block, at the pixels-per-ms the window is
/// drawn at: every native window in it closes by its end, so every group is
/// settled.
fn settle_block(
    facts: &[AggressionPrimitive],
    start: i64,
    end: i64,
    inputs: &BlockInputs<'_>,
) -> Vec<Group> {
    let epoch = inputs.epoch;
    let block: Vec<_> = facts
        .iter()
        .filter(|mark| {
            (start..end).contains(&window_start(mark.first_timestamp_ms, epoch.dot_window_ms))
        })
        .cloned()
        .collect();
    if block.is_empty() {
        return Vec::new();
    }
    let mut memory = TapeDotMemory::default();
    let _ = memory.project(
        &block,
        TapeDotView {
            now_ms: end,
            window_ms: end - start,
            dot_window_ms: epoch.dot_window_ms,
            evicted_through_ms: inputs.evicted_through_ms,
            prices: epoch.prices,
            geometry: TapeDotGeometry {
                left_x: 0.0,
                right_x: 1.0,
                width_px: epoch.width_px * (end - start) as f32 / epoch.window_ms as f32,
                height_px: epoch.height_px,
            },
        },
        inputs.sizing,
        inputs.bubbles,
        inputs.lane,
        inputs.opening_bursts,
    );
    let mut groups = memory.settled;
    groups.append(&mut memory.frontier);
    groups
}
