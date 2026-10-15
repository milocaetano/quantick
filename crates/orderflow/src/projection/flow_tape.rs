//! Execution aggregation in authoritative tick-candle coordinates.
//! Raw membership is retained; every zoom starts again from native execution cells.
use super::tape::{Full, MergeDisc, TapeMoment, collide};
use super::{AggressionPrimitive, DotSizing, PriceWindow, TapeDotGeometry};
use crate::{BubbleStyle, LiveLaneStyle};
use quantick_engine::Trade;
use rust_decimal::Decimal;
use std::sync::Arc;
mod carry;
pub use carry::FlowTail;
mod constants;
use constants::{FLOW_MERGE_HEIGHT_PX, FLOW_MERGE_WIDTH_PX};
pub use constants::{
    FLOW_REGION_WINDOW_MS, FLOW_SCALE_HOLD, LARGE_REGION_REFERENCE_DIVISOR, MAX_FLOW_EXECUTIONS,
};
mod reading;
pub use reading::{
    CaptionInk, caption_qualifications, caption_segments, caption_text, notes_segments,
    placement_note, styled_caption_text,
};
mod source;
pub use source::{FlowCoverage, FlowOpeningSelection, FlowTapeSource};
mod stream;
pub use stream::{
    FlowChunk, FlowKeep, FlowProgress, FlowRequest, FlowRunner, FlowSession, FlowWorkerCache,
    OwnedFlowExecution,
};

/// The quantity a FLOW sphere is sized against.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum FlowReference {
    /// One common reference from the final visible regional groups.
    #[default]
    VisibleRegions,
    /// The pane's fixed reference (the tick spheres' preset value): no pan,
    /// zoom or new print resizes a sphere, and a larger region caps at the
    /// radius limit.
    Typed(Decimal),
    /// Visible ordinary region maximum with a configured floor. This keeps
    /// ordinary areas proportional when regions exceed the preset quantity.
    VisibleRegionsAtLeast(Decimal),
}
impl FlowReference {
    pub fn typed(self) -> Option<Decimal> {
        match self {
            Self::VisibleRegions => None,
            Self::Typed(value) | Self::VisibleRegionsAtLeast(value) => Some(value),
        }
    }
}
impl From<crate::TickBubbleStyle> for FlowReference {
    fn from(style: crate::TickBubbleStyle) -> Self {
        match style.reference_mode {
            crate::TickReferenceMode::Fixed => Self::Typed(style.reference()),
            crate::TickReferenceMode::VisibleMax => Self::VisibleRegionsAtLeast(style.reference()),
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
    VisibleRegionMaxWithFloor,
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
    /// Start of the loaded coverage that runs to `requested_ordinals.end`:
    /// a newest-first fill has computed everything from here on.
    pub covered_from_ordinal: usize,
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

/// The merge frame of a view: every group a disc of the fixed support
/// radius, in the view's pixels.
fn merge_frame(view: FlowTapeView) -> (BubbleStyle, DotSizing, TapeDotGeometry) {
    (
        BubbleStyle {
            max_radius: view.merge_support_radius,
            ..BubbleStyle::default()
        },
        DotSizing {
            native_tape: true,
            tape_column_px: 0.0,
            candle_column_px: 0.0,
            px_per_price: 0.0,
            typed_full: view.reference.typed(),
        },
        TapeDotGeometry {
            left_x: 0.0,
            right_x: 1.0,
            width_px: view.width_px,
            height_px: view.height_px,
        },
    )
}

/// Where one group stands in a view before merging.
enum Admission {
    /// Off the price axis, with its executions.
    OffAxis(usize),
    /// Outside the painted clip, with its executions.
    Offscreen(usize),
    /// Merged with its region window's neighbours.
    Mergeable,
    /// An oversized native cell: it cannot merge, and stays out of the
    /// neighbour index, where near-identical centroids would cause
    /// quadratic rejection.
    Standalone,
}

fn admit(group: &Group, view: FlowTapeView) -> Admission {
    if !(0.0..=1.0).contains(&group.y()) {
        return Admission::OffAxis(group.members.len());
    }
    let position = group.position_quantity / group.quantity();
    if position < view.clip_left || position > view.clip_right {
        Admission::Offscreen(group.members.len())
    } else if group.hull.within_merge_limits() {
        Admission::Mergeable
    } else {
        Admission::Standalone
    }
}

/// The region window a group merges in.
fn region_window(group: &Group) -> i64 {
    group.first_timestamp_ms().div_euclid(FLOW_REGION_WINDOW_MS)
}

/// One region window's mergeable groups, merged.
fn merge_window(window: Vec<Group>, view: FlowTapeView) -> Vec<Group> {
    let (bubbles, sizing, geometry) = merge_frame(view);
    collide(
        window,
        sizing,
        &bubbles,
        &LiveLaneStyle::default(),
        geometry,
        Decimal::ZERO,
        false,
    )
    .0
}

/// A merged group as the sphere drawn against `effective_reference`;
/// `exclusion` says whether opening regions are kept out of the scale. An
/// oversized opening draws its earned area past the radius limit only over
/// visible regions: a fixed reference caps it like any region.
fn finish_group(
    mut group: Group,
    view: FlowTapeView,
    effective_reference: Decimal,
    exclusion: bool,
) -> FlowTapeDot {
    let quantity = group.quantity();
    group.members.parts.sort_by_key(|part| part[0].ordinal);
    let mut mark = group.moment.finish(effective_reference);
    mark.folded_marks = if group.cells > 1 {
        group.cells as u32
    } else {
        0
    };
    let mut dot = FlowTapeDot {
        radius: 0.0,
        opening_anchor: group.opening_anchor,
        opening_oversized: false,
        candle_position: group.position_quantity / quantity,
        mark,
        members: group.members,
        first_slot: group.first_slot,
        end_slot: group.end_slot,
        opening_quantity: group.opening,
        native_cells: group.cells,
    };
    resize_dot(&mut dot, view, effective_reference, exclusion);
    dot
}

/// Paint an already consolidated region at the frame's common reference.
/// The factual group, its membership and its center never change here.
fn resize_dot(dot: &mut FlowTapeDot, view: FlowTapeView, reference: Decimal, exclusion: bool) {
    dot.mark.size = super::normalized_area_size(dot.mark.quantity, reference);
    dot.opening_oversized = exclusion && dot.opening_anchor && dot.mark.quantity > reference;
    dot.radius = view.radius_limit
        * if dot.opening_oversized && !matches!(view.reference, FlowReference::Typed(_)) {
            use rust_decimal::prelude::ToPrimitive as _;
            (dot.mark.quantity.to_f64().unwrap_or_default() / reference.to_f64().unwrap_or(1.0))
                .sqrt() as f32
        } else {
            dot.mark.size
        };
}

impl FlowTapeFrame {
    /// Cached windows and carried history retain factual groups. A visible
    /// floor mode sizes their complete publication against one common scale.
    fn rescale_visible_floor(&mut self) {
        let FlowReference::VisibleRegionsAtLeast(floor) = self.view.reference else {
            return;
        };
        if self.dots.is_empty() {
            return;
        }
        let visible = |dot: &&FlowTapeDot| {
            dot.candle_position >= self.view.clip_left
                && dot.candle_position <= self.view.clip_right
                && dot.mark.price >= self.view.prices.low
                && dot.mark.price <= self.view.prices.high
        };
        let reference = self
            .dots
            .iter()
            .filter(visible)
            .map(|dot| {
                dot.mark.quantity
                    - if self.view.exclude_opening && dot.opening_anchor {
                        dot.opening_quantity
                    } else {
                        Decimal::ZERO
                    }
            })
            .max()
            .unwrap_or_default()
            .max(floor);
        self.effective_reference = Some(reference);
        self.scale_basis = FlowScaleBasis::VisibleRegionMaxWithFloor;
        self.opening_exclusion_effective = self.view.exclude_opening
            && self
                .dots
                .iter()
                .filter(visible)
                .any(|dot| dot.opening_anchor);
        for dot in &mut self.dots {
            resize_dot(dot, self.view, reference, self.opening_exclusion_effective);
        }
    }
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
    let mut off_axis_executions = 0;
    let mut offscreen_executions = 0;
    // Separate indexes bound neighbour work even when thousands of distant
    // time intervals occupy the same pixels. Rejecting only in permits_merge
    // would leave all those impossible neighbours in one growing index.
    let mut windows = std::collections::BTreeMap::<i64, Vec<Group>>::new();
    let mut standalone = Vec::new();
    for group in groups {
        match admit(&group, view) {
            Admission::OffAxis(executions) => off_axis_executions += executions,
            Admission::Offscreen(executions) => offscreen_executions += executions,
            Admission::Mergeable => windows
                .entry(region_window(&group))
                .or_default()
                .push(group),
            Admission::Standalone => standalone.push(group),
        }
    }
    let mut groups = Vec::new();
    for window in windows.into_values() {
        groups.extend(merge_window(window, view));
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
        // The pane's one scale: nothing on screen moves it. A region larger
        // than the reference caps at the radius limit instead of shrinking
        // every other sphere (`normalized_area_size` clamps the ratio).
        FlowReference::Typed(reference) => (
            reference,
            FlowScaleBasis::TypedReference,
            view.exclude_opening && has_opening,
        ),
        FlowReference::VisibleRegionsAtLeast(reference) => (
            reference.max(ordinary_max),
            FlowScaleBasis::VisibleRegionMaxWithFloor,
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
        .map(|group| {
            finish_group(
                group,
                view,
                effective_reference,
                opening_exclusion_effective,
            )
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
