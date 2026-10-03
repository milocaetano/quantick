//! Retained native facts and ordinal coverage, independent of view layout.
use super::*;
use crate::history::{AggressorSide, RecordedOpenings, RestingSide};
use crate::projection::tape::combine_tape_facts;
use quantick_engine::Side;
use rust_decimal::prelude::ToPrimitive as _;
use std::{collections::BTreeMap, ops::Range};

#[derive(Debug, Default, Clone)]
pub struct FlowCoverage {
    ranges: Vec<Range<usize>>,
}
impl FlowCoverage {
    pub fn retain(&mut self, keep: Range<usize>) {
        self.ranges.retain_mut(|range| {
            range.start = range.start.max(keep.start);
            range.end = range.end.min(keep.end);
            range.start < range.end
        });
    }
    pub fn contains(&self, ordinal: usize) -> bool {
        let index = self.ranges.partition_point(|range| range.end <= ordinal);
        self.ranges
            .get(index)
            .is_some_and(|range| range.contains(&ordinal))
    }
    pub fn insert(&mut self, mut range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let first = self.ranges.partition_point(|held| held.end < range.start);
        let mut end = first;
        while end < self.ranges.len() && self.ranges[end].start <= range.end {
            range.start = range.start.min(self.ranges[end].start);
            range.end = range.end.max(self.ranges[end].end);
            end += 1;
        }
        self.ranges.splice(first..end, [range]);
    }
    pub fn first_gap(&self, requested: Range<usize>) -> Option<Range<usize>> {
        let mut start = requested.start;
        for held in &self.ranges {
            if held.end <= start {
                continue;
            }
            if held.start >= requested.end {
                break;
            }
            if held.start > start {
                return Some(start..held.start.min(requested.end));
            }
            start = start.max(held.end);
        }
        (start < requested.end).then_some(start..requested.end)
    }
    pub fn count(&self, requested: Range<usize>) -> usize {
        self.ranges
            .iter()
            .map(|held| {
                held.end
                    .min(requested.end)
                    .saturating_sub(held.start.max(requested.start))
            })
            .sum()
    }
}

struct NativeCell {
    first_ordinal: usize,
    mark: AggressionPrimitive,
    position_quantity: Decimal,
    /// Exact minimum/maximum admitted execution positions, including within one cell.
    positions: [Decimal; 2],
    members: Arc<Vec<FlowMember>>,
}
#[derive(Default)]
pub struct FlowTapeSource {
    cells: BTreeMap<(usize, i64, Decimal), NativeCell>,
    coverage: FlowCoverage,
    ineligible: FlowCoverage,
    count: usize,
}

/// Both classifications are supplied by the complete canonical source owner.
/// Missing retained or visible executions never nominate replacement anchors.
#[derive(Default, Clone, Copy)]
pub struct FlowOpeningSelection<'a> {
    pub windows: &'a [i64],
    pub ordinals: &'a [usize],
}

impl FlowTapeSource {
    pub fn retain(&mut self, keep: &FlowKeep) {
        let beyond = self
            .cells
            .split_off(&(keep.slots.end, i64::MIN, Decimal::MIN));
        let inside = self
            .cells
            .split_off(&(keep.slots.start, i64::MIN, Decimal::MIN));
        self.cells = inside;
        drop(beyond);
        self.coverage.retain(keep.ordinals.clone());
        self.ineligible.retain(keep.ordinals.clone());
        self.count = self.coverage.count(0..usize::MAX);
    }
    pub fn coverage(&self) -> &FlowCoverage {
        &self.coverage
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// Each ordinal is admitted once, even when a retried source packet overlaps.
    pub fn append<'a>(&mut self, executions: impl IntoIterator<Item = FlowExecution<'a>>) {
        for execution in executions {
            let FlowExecution {
                ordinal,
                slot,
                accepted_ordinal,
                ticks_per_bar,
                trade,
                ..
            } = execution;
            if self.count == MAX_FLOW_EXECUTIONS {
                break;
            }
            if self.coverage.contains(ordinal) {
                continue;
            }
            self.coverage.insert(ordinal..ordinal + 1);
            self.count += 1;
            if ticks_per_bar <= Decimal::ZERO || trade.quantity <= Decimal::ZERO {
                self.ineligible.insert(ordinal..ordinal + 1);
                continue;
            }
            let position = Decimal::from(slot)
                + (Decimal::from(accepted_ordinal) + Decimal::new(5, 1)) / ticks_per_bar;
            let mark = fact(trade);
            let member = FlowMember {
                ordinal,
                candle_slot: slot,
                accepted_ordinal,
                source_id: trade.agg_id,
            };
            let key = (
                slot,
                RecordedOpenings::window_start(trade.timestamp_ms),
                trade.price,
            );
            match self.cells.entry(key) {
                std::collections::btree_map::Entry::Occupied(mut held) => {
                    let cell = held.get_mut();
                    cell.mark = combine_tape_facts([&cell.mark, &mark]).expect("two native facts");
                    cell.position_quantity += position * trade.quantity;
                    cell.positions[0] = cell.positions[0].min(position);
                    cell.positions[1] = cell.positions[1].max(position);
                    cell.first_ordinal = cell.first_ordinal.min(ordinal);
                    let members = Arc::make_mut(&mut cell.members);
                    let at = members.partition_point(|held| held.ordinal < ordinal);
                    members.insert(at, member);
                }
                std::collections::btree_map::Entry::Vacant(empty) => {
                    empty.insert(NativeCell {
                        first_ordinal: ordinal,
                        mark,
                        position_quantity: position * trade.quantity,
                        positions: [position, position],
                        members: Arc::new(vec![member]),
                    });
                }
            }
        }
    }
    pub fn project(
        &self,
        epoch: u64,
        layout_revision: u64,
        source_count: usize,
        requested: Range<usize>,
        view: FlowTapeView,
        openings: FlowOpeningSelection<'_>,
    ) -> FlowTapeFrame {
        let loaded_executions = self.coverage.count(requested.clone());
        let mut frame = FlowTapeFrame {
            source_revision: epoch,
            source_count,
            layout_revision,
            omitted_executions: requested.len().saturating_sub(loaded_executions),
            computed_through_ordinal: self
                .coverage
                .first_gap(requested.clone())
                .map_or(requested.end, |gap| gap.start),
            ineligible_executions: self.ineligible.count(requested.clone()),
            requested_ordinals: requested,
            loaded_executions,
            cache_limit_reached: self.count == MAX_FLOW_EXECUTIONS,
            off_axis_executions: 0,
            offscreen_executions: 0,
            view,
            effective_reference: None,
            scale_basis: FlowScaleBasis::Empty,
            opening_exclusion_effective: false,
            dots: Vec::new(),
            paint_order: Vec::new(),
        };
        if !valid(view) {
            return frame;
        }
        let slots = Decimal::from(view.end_slot - view.first_slot);
        // width_px covers the admission span, so padding cancels just as it does
        // in the collision geometry; no viewport-width reinterpretation is made.
        let pixels_per_slot = f64::from(view.width_px) / (view.end_slot - view.first_slot) as f64;
        let position_px = |position: Decimal| {
            (position - Decimal::from(view.first_slot))
                .to_f64()
                .unwrap_or_default()
                * pixels_per_slot
        };
        let groups = self
            .cells
            .range(
                (view.first_slot, i64::MIN, Decimal::MIN)..(view.end_slot, i64::MIN, Decimal::MIN),
            )
            .map(|(&(slot, window, _), cell)| {
                let mut mark = cell.mark.clone(); // compact facts: identities live in shared member buffers
                let position = cell.position_quantity / mark.quantity;
                mark.x = ((position - Decimal::from(view.first_slot)) / slots)
                    .to_f64()
                    .unwrap_or_default();
                mark.y = view.prices.y_unclamped(mark.price).unwrap_or_default();
                let hull = FlowHull {
                    left: position_px(cell.positions[0]),
                    right: position_px(cell.positions[1]),
                    top: mark.y * f64::from(view.height_px),
                    bottom: mark.y * f64::from(view.height_px),
                };
                let opening = if openings.windows.contains(&window) {
                    mark.quantity
                } else {
                    Decimal::ZERO
                };
                Group {
                    first_ordinal: cell.first_ordinal,
                    moment: TapeMoment::new(mark, f64::INFINITY),
                    hull,
                    members: FlowMembers {
                        parts: vec![Arc::clone(&cell.members)],
                        count: cell.members.len(),
                    },
                    position_quantity: cell.position_quantity,
                    first_slot: slot,
                    end_slot: slot + 1,
                    opening,
                    opening_anchor: openings.ordinals.iter().any(|ordinal| {
                        cell.members
                            .binary_search_by_key(ordinal, |member| member.ordinal)
                            .is_ok()
                    }),
                    cells: 1,
                }
            })
            .collect();
        (
            frame.dots,
            frame.effective_reference,
            frame.off_axis_executions,
            frame.offscreen_executions,
            frame.scale_basis,
            frame.opening_exclusion_effective,
        ) = finish_groups(groups, view);
        frame.order_for_paint();
        frame
    }
}
fn valid(view: FlowTapeView) -> bool {
    view.reference
        .typed()
        .is_none_or(|value| value > Decimal::ZERO)
        && view.clip_right > view.clip_left
        && view.end_slot > view.first_slot
        && [
            view.width_px,
            view.height_px,
            view.radius_limit,
            view.merge_support_radius,
        ]
        .iter()
        .all(|value| value.is_finite() && *value > 0.0)
}
fn fact(trade: &Trade) -> AggressionPrimitive {
    let buy = trade.side == Side::Buy;
    AggressionPrimitive {
        agg_id: trade.agg_id,
        agg_ids: Vec::new(),
        generation: None,
        side: if buy {
            AggressorSide::Buy
        } else {
            AggressorSide::Sell
        },
        consumed_side: if buy {
            RestingSide::Ask
        } else {
            RestingSide::Bid
        },
        quantity: trade.quantity,
        buy_share: f32::from(buy),
        live: false,
        price_bucket: trade.price,
        price_span: Decimal::ZERO,
        price: trade.price,
        trade_count: 1,
        first_timestamp_ms: trade.timestamp_ms,
        last_timestamp_ms: trade.timestamp_ms,
        timestamp_quantity: Decimal::from(trade.timestamp_ms) * trade.quantity,
        matched_quantity: Decimal::ZERO,
        buy_quantity: if buy { trade.quantity } else { Decimal::ZERO },
        matched_fraction: 0.0,
        liquidity_event_ids: Vec::new(),
        x: 0.0,
        y: 0.0,
        size: 0.0,
        folded_marks: 0,
    }
}
