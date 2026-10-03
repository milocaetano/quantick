//! Execution aggregation in authoritative tick-candle coordinates.
//! Raw membership is retained; every zoom starts again from native execution cells.
use super::tape::{Full, MergeDisc, TapeMoment, collide};
use super::{AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry};
use crate::{BubbleStyle, LiveLaneStyle};
use quantick_engine::Trade;
use rust_decimal::Decimal;
use std::sync::Arc;
mod reading;
pub use reading::caption_text;
mod source;
pub use source::{FlowCoverage, FlowOpeningSelection, FlowTapeSource};
mod stream;
pub use stream::{
    FlowChunk, FlowKeep, FlowProgress, FlowRequest, FlowRunner, FlowSession, FlowWorkerCache,
    OwnedFlowExecution,
};

/// Retained worker cache bound, independent of per-frame source admission.
/// The readback reports missing coverage if a larger source exhausts it.
pub const MAX_FLOW_EXECUTIONS: usize = 2_000_000;

// Merges may pool a local time/price region, but may not walk along an entire swing.
// An indivisible native cell wider than this remains alone; it is never split or lost.
const FLOW_MERGE_WIDTH_PX: f64 = 48.0;
const FLOW_MERGE_HEIGHT_PX: f64 = 32.0;

/// Fixed time support prevents compressed tick coordinates from pooling minutes
/// of routine flow into peers of a brief large execution. These are regions,
/// not reconstructed orders. A multiple of the native 100 ms cell keeps facts whole.
pub const FLOW_REGION_WINDOW_MS: i64 = 1_000;

/// Production uses one common reference from the final visible regional groups.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum FlowReference {
    #[default]
    VisibleRegions,
    /// Explicit fixture calibration; ordinary overflow still shrinks uniformly.
    Typed(Decimal),
}
impl FlowReference {
    pub fn typed(self) -> Option<Decimal> {
        match self {
            Self::VisibleRegions => None,
            Self::Typed(value) => Some(value),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowScaleBasis {
    Empty,
    VisibleRegionMax,
    VisibleOrdinaryRegionMax,
    OpeningOnlyFallback,
    TypedReference,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlowTapeView {
    pub first_slot: usize,
    pub end_slot: usize,
    /// Actual fractional candle-coordinate viewport; slots include admission overscan.
    pub clip_left: Decimal,
    pub clip_right: Decimal,
    /// Width of the admission slot span (`end_slot - first_slot`) at the current zoom.
    /// This includes overscan; `clip_left/right` separately describe the painted viewport.
    pub width_px: f32,
    pub height_px: f32,
    pub prices: PriceWindow,
    pub reference: FlowReference,
    pub radius_limit: f32,
    /// Fixed spatial aggregation support in logical pixels, independent of painted area.
    pub merge_support_radius: f32,
    pub exclude_opening: bool,
}

pub struct FlowExecution<'a> {
    pub ordinal: usize,
    pub slot: usize,
    pub accepted_ordinal: usize,
    pub ticks_per_bar: Decimal,
    pub trade: &'a Trade,
    pub opening: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowMember {
    pub ordinal: usize,
    pub candle_slot: usize,
    pub accepted_ordinal: usize,
    pub source_id: u64,
}

/// Shared native member buffers: regional projection never expands all identities.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FlowMembers {
    parts: Vec<Arc<Vec<FlowMember>>>,
    count: usize,
}
impl FlowMembers {
    pub fn iter(&self) -> impl Iterator<Item = &FlowMember> {
        self.parts.iter().flat_map(|part| part.iter())
    }
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlowTapeDot {
    pub mark: AggressionPrimitive,
    pub members: FlowMembers,
    pub candle_position: Decimal,
    pub first_slot: usize,
    pub end_slot: usize,
    pub opening_quantity: Decimal,
    pub native_cells: usize,
    /// Equivalent gross-area radius: buy_radius² + sell_radius² = radius².
    pub radius: f32,
    /// Contains the canonical first recorded execution of at least one UTC date.
    pub opening_anchor: bool,
    /// First daily region whose earned radius exceeds the ordinary reference radius.
    pub opening_oversized: bool,
}
impl FlowTapeDot {
    /// Exact sides converted only at the geometry boundary, without a Decimal division floor.
    pub fn side_shares(&self) -> (f64, f64) {
        use rust_decimal::prelude::ToPrimitive as _;
        let total = self.mark.quantity.to_f64().unwrap_or_default();
        let share = |quantity: Decimal| {
            if quantity <= Decimal::ZERO || total <= 0.0 {
                0.0
            } else {
                quantity.to_f64().unwrap_or_default() / total
            }
        };
        (
            share(self.mark.buy_quantity),
            share(self.mark.quantity - self.mark.buy_quantity),
        )
    }

    /// Equivalent radii of the buy/sell sector areas; both sectors share the gross circle.
    pub fn side_radii(&self) -> (f32, f32) {
        let (buy, sell) = self.side_shares();
        (
            self.radius * buy.sqrt() as f32,
            self.radius * sell.sqrt() as f32,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FlowTapeFrame {
    /// Scoped by the containing pane: source reset/rebuild changes this identity.
    pub source_revision: u64,
    pub source_count: usize,
    pub layout_revision: u64,
    pub requested_ordinals: std::ops::Range<usize>,
    pub loaded_executions: usize,
    /// Exclusive end of contiguous loaded coverage from requested_ordinals.start.
    pub computed_through_ordinal: usize,
    pub cache_limit_reached: bool,
    pub omitted_executions: usize,
    pub off_axis_executions: usize,
    pub offscreen_executions: usize,
    /// Admitted records with nonpositive quantity or an invalid tick denominator.
    pub ineligible_executions: usize,
    pub view: FlowTapeView,
    pub effective_reference: Option<Decimal>,
    pub scale_basis: FlowScaleBasis,
    pub opening_exclusion_effective: bool,
    pub dots: Vec<FlowTapeDot>,
    paint_order: Vec<usize>,
}

impl FlowTapeFrame {
    /// Computed once per worker projection: the faint first daily exception is
    /// underneath ordinary volume, then small regions precede large ones.
    pub fn dots_in_paint_order(&self) -> impl DoubleEndedIterator<Item = &FlowTapeDot> {
        self.paint_order.iter().map(|&index| &self.dots[index])
    }

    fn order_for_paint(&mut self) {
        self.paint_order = (0..self.dots.len()).collect();
        self.paint_order.sort_by_key(|&index| {
            let dot = &self.dots[index];
            (!dot.opening_oversized, dot.mark.quantity, index)
        });
    }
}

/// Bounds of original source positions in logical pixels, never centroid bounds.
#[derive(Clone, Copy)]
struct FlowHull {
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
}
impl FlowHull {
    fn union(self, other: Self) -> Self {
        Self {
            left: self.left.min(other.left),
            right: self.right.max(other.right),
            top: self.top.min(other.top),
            bottom: self.bottom.max(other.bottom),
        }
    }
    fn within_merge_limits(self) -> bool {
        self.right - self.left <= FLOW_MERGE_WIDTH_PX
            && self.bottom - self.top <= FLOW_MERGE_HEIGHT_PX
    }
}
struct Group {
    moment: TapeMoment,
    hull: FlowHull,
    first_ordinal: usize,
    members: FlowMembers,
    position_quantity: Decimal,
    first_slot: usize,
    end_slot: usize,
    opening: Decimal,
    opening_anchor: bool,
    cells: usize,
}
impl MergeDisc for Group {
    fn x(&self) -> f64 {
        self.moment.x()
    }
    fn y(&self) -> f64 {
        self.moment.y()
    }
    fn first_timestamp_ms(&self) -> i64 {
        self.moment.first_timestamp_ms()
    }
    fn agg_id(&self) -> u64 {
        self.first_ordinal as u64
    }
    fn quantity(&self) -> Decimal {
        self.moment.quantity()
    }
    fn reference_quantity(&self) -> Decimal {
        self.quantity()
    }
    fn radius(&mut self, _: DotSizing, bubbles: &BubbleStyle, _: &LiveLaneStyle, _: Full) -> f32 {
        // Define regions by proximity, so an unrelated whale cannot fragment
        // nearby small executions. Final painted area still follows full volume.
        bubbles.max_radius
    }
    fn permits_merge(&self, other: &Self) -> bool {
        self.hull.union(other.hull).within_merge_limits()
    }
    fn absorb(&mut self, mut other: Self, right_x: f64) {
        // The collision frontier often folds an old large group into a new tiny
        // group. Keep the larger allocation, avoiding quadratic member copying.
        if self.members.len() < other.members.len() {
            std::mem::swap(self, &mut other);
        }
        self.hull = self.hull.union(other.hull);
        self.first_ordinal = self.first_ordinal.min(other.first_ordinal);
        self.position_quantity += other.position_quantity;
        self.first_slot = self.first_slot.min(other.first_slot);
        self.end_slot = self.end_slot.max(other.end_slot);
        self.opening += other.opening;
        self.opening_anchor |= other.opening_anchor;
        self.cells += other.cells;
        self.members.count += other.members.count;
        self.members.parts.extend(other.members.parts);
        self.moment.absorb(other.moment, right_x);
    }
}

/// Fixture entry point. Runtime callers retain a FlowTapeSource on their worker.
#[must_use]
pub fn project_flow_tape<'a>(
    executions: impl IntoIterator<Item = FlowExecution<'a>>,
    source_revision: u64,
    source_count: usize,
    requested_executions: usize,
    view: FlowTapeView,
) -> FlowTapeFrame {
    let mut source = FlowTapeSource::default();
    let mut openings = Vec::new();
    let mut anchors = crate::history::RecordedOpeningAnchors::default();
    for execution in executions {
        if execution.opening
            && execution.trade.quantity > Decimal::ZERO
            && execution.ticks_per_bar > Decimal::ZERO
        {
            anchors.observe(execution.trade.timestamp_ms, execution.ordinal);
            openings.push(crate::history::RecordedOpenings::window_start(
                execution.trade.timestamp_ms,
            ));
        }
        source.append(std::iter::once(execution));
    }
    source.project(
        source_revision,
        0,
        source_count,
        0..requested_executions,
        view,
        FlowOpeningSelection {
            windows: &openings,
            ordinals: &anchors.ordinals().collect::<Vec<_>>(),
        },
    )
}

fn finish_groups(
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
    let bubbles = BubbleStyle {
        max_radius: view.merge_support_radius,
        ..BubbleStyle::default()
    };
    let sizing = DotSizing {
        native_tape: true,
        tape_column_px: 0.0,
        candle_column_px: 0.0,
        px_per_price: 0.0,
        typed_full: view.reference.typed(),
    };
    let geometry = TapeDotGeometry {
        left_x: 0.0,
        right_x: 1.0,
        width_px: view.width_px,
        height_px: view.height_px,
    };
    let mut off_axis_executions = 0;
    let mut offscreen_executions = 0;
    let (groups, mut standalone): (Vec<_>, Vec<_>) = groups
        .into_iter()
        .filter(|group| {
            if !(0.0..=1.0).contains(&group.y()) {
                off_axis_executions += group.members.len();
                return false;
            }
            let position = group.position_quantity / group.quantity();
            if position < view.clip_left || position > view.clip_right {
                offscreen_executions += group.members.len();
                return false;
            }
            true
        })
        // Oversized native cells cannot merge. Keep them out of the neighbour
        // index, where near-identical centroids would cause quadratic rejection.
        .partition(|group| group.hull.within_merge_limits());
    // Separate indexes bound neighbour work even when thousands of distant
    // time intervals occupy the same pixels. Rejecting only in permits_merge
    // would leave all those impossible neighbours in one growing index.
    let mut windows = std::collections::BTreeMap::<i64, Vec<Group>>::new();
    for group in groups {
        windows
            .entry(group.first_timestamp_ms().div_euclid(FLOW_REGION_WINDOW_MS))
            .or_default()
            .push(group);
    }
    let mut groups = Vec::new();
    for window in windows.into_values() {
        let (merged, _) = collide(
            window,
            sizing,
            &bubbles,
            &LiveLaneStyle::default(),
            geometry,
            Decimal::ZERO,
            false,
        );
        groups.extend(merged);
    }
    groups.append(&mut standalone);
    let Some(full_max) = groups.iter().map(Group::quantity).max() else {
        return (
            Vec::new(),
            None,
            off_axis_executions,
            offscreen_executions,
            FlowScaleBasis::Empty,
            false,
        );
    };
    let ordinary_max = groups
        .iter()
        .map(|group| {
            group.quantity()
                - if view.exclude_opening && group.opening_anchor {
                    group.opening
                } else {
                    Decimal::ZERO
                }
        })
        .max()
        .unwrap_or_default();
    let has_opening = groups.iter().any(|group| group.opening_anchor);
    let (effective_reference, scale_basis, opening_exclusion_effective) = match view.reference {
        FlowReference::Typed(reference) => (
            reference.max(ordinary_max),
            FlowScaleBasis::TypedReference,
            view.exclude_opening && has_opening,
        ),
        FlowReference::VisibleRegions if ordinary_max > Decimal::ZERO => {
            let excluded = view.exclude_opening && has_opening;
            (
                ordinary_max,
                if excluded {
                    FlowScaleBasis::VisibleOrdinaryRegionMax
                } else {
                    FlowScaleBasis::VisibleRegionMax
                },
                excluded,
            )
        }
        FlowReference::VisibleRegions => (full_max, FlowScaleBasis::OpeningOnlyFallback, false),
    };
    let dots = groups
        .into_iter()
        .map(|mut group| {
            let quantity = group.quantity();
            let opening_oversized = opening_exclusion_effective
                && group.opening_anchor
                && quantity > effective_reference;
            group.members.parts.sort_by_key(|part| part[0].ordinal);
            let mut mark = group.moment.finish(effective_reference);
            mark.folded_marks = if group.cells > 1 {
                group.cells as u32
            } else {
                0
            };
            FlowTapeDot {
                radius: view.radius_limit
                    * if opening_oversized {
                        use rust_decimal::prelude::ToPrimitive as _;
                        (quantity.to_f64().unwrap_or_default()
                            / effective_reference.to_f64().unwrap_or(1.0))
                        .sqrt() as f32
                    } else {
                        super::normalized_area_size(quantity, effective_reference)
                    },
                opening_anchor: group.opening_anchor,
                opening_oversized,
                candle_position: group.position_quantity / quantity,
                mark,
                members: group.members,
                first_slot: group.first_slot,
                end_slot: group.end_slot,
                opening_quantity: group.opening,
                native_cells: group.cells,
            }
        })
        .collect();
    (
        dots,
        Some(effective_reference),
        off_axis_executions,
        offscreen_executions,
        scale_basis,
        opening_exclusion_effective,
    )
}

#[cfg(test)]
mod tests;
