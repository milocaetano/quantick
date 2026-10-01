//! Execution aggregation in authoritative tick-candle coordinates.
//! Raw membership is retained; every zoom starts again from native execution cells.
use super::tape::{Full, MergeDisc, TapeMoment, collide};
use super::{AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry};
use crate::{BubbleStyle, LiveLaneStyle};
use quantick_engine::Trade;
use rust_decimal::Decimal;
use std::sync::Arc;
mod source;
pub use source::{FlowCoverage, FlowTapeSource};
mod stream;
pub use stream::{
    FlowChunk, FlowKeep, FlowProgress, FlowRequest, FlowRunner, FlowSession, FlowWorkerCache,
    OwnedFlowExecution,
};

/// Retained worker cache bound, independent of per-frame source admission.
/// The readback reports missing coverage if a larger source exhausts it.
pub const MAX_FLOW_EXECUTIONS: usize = 2_000_000;

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
    /// Collision support in logical pixels, independent of painted quantity area.
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
    pub opening_capped: bool,
}
impl FlowTapeDot {
    /// Actual buy/sell disc radii under the region's common area scale and cap.
    pub fn side_radii(&self) -> (f32, f32) {
        use rust_decimal::prelude::ToPrimitive as _;
        let total = self.mark.quantity.to_f64().unwrap_or_default();
        let radius = |quantity: Decimal| {
            if quantity <= Decimal::ZERO || total <= 0.0 {
                return 0.0;
            }
            // Convert the exact quantities at the geometry boundary. Dividing
            // in Decimal first could round a positive tiny share down to zero.
            self.radius * (quantity.to_f64().unwrap_or_default() / total).sqrt() as f32
        };
        (
            radius(self.mark.buy_quantity),
            radius(self.mark.quantity - self.mark.buy_quantity),
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
}

struct Group {
    moment: TapeMoment,
    first_ordinal: usize,
    members: FlowMembers,
    position_quantity: Decimal,
    first_slot: usize,
    end_slot: usize,
    opening: Decimal,
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
    fn radius(
        &mut self,
        _: DotSizing,
        bubbles: &BubbleStyle,
        _: &LiveLaneStyle,
        _full: Full,
    ) -> f32 {
        // Spatial resolution groups small prints as well as large ones. Their
        // painted radii are calculated only after regional quantities are known.
        bubbles.max_radius
    }
    fn absorb(&mut self, mut other: Self, right_x: f64) {
        // The collision frontier often folds an old large group into a new tiny
        // group. Keep the larger allocation, avoiding quadratic member copying.
        if self.members.len() < other.members.len() {
            std::mem::swap(self, &mut other);
        }
        self.first_ordinal = self.first_ordinal.min(other.first_ordinal);
        self.position_quantity += other.position_quantity;
        self.first_slot = self.first_slot.min(other.first_slot);
        self.end_slot = self.end_slot.max(other.end_slot);
        self.opening += other.opening;
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
    for execution in executions {
        if execution.opening {
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
        &openings,
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
    let groups = groups
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
        .collect::<Vec<_>>();
    let (groups, _) = collide(
        groups,
        sizing,
        &bubbles,
        &LiveLaneStyle::default(),
        geometry,
        Decimal::ZERO,
        false,
    );
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
                - if view.exclude_opening {
                    group.opening
                } else {
                    Decimal::ZERO
                }
        })
        .max()
        .unwrap_or_default();
    let has_opening = groups.iter().any(|group| group.opening > Decimal::ZERO);
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
            group.members.parts.sort_by_key(|part| part[0].ordinal);
            let mut mark = group.moment.finish(effective_reference);
            mark.folded_marks = if group.cells > 1 {
                group.cells as u32
            } else {
                0
            };
            FlowTapeDot {
                radius: view.radius_limit
                    * super::normalized_area_size(quantity, effective_reference),
                opening_capped: opening_exclusion_effective
                    && group.opening > Decimal::ZERO
                    && quantity > effective_reference,
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
