//! Bounded readback of exactly the FLOW execution frame painted.
use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_control_host::wire::{canonical_decimal, wire_usize};
use quantick_orderflow::projection::flow_tape::{
    FlowProgress, FlowReference, FlowScaleBasis, FlowTapeDot, FlowTapeFrame,
};
use rust_decimal::Decimal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MAX_MARKS: usize = 256;
const MAX_MEMBERS: usize = 128;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FlowExecutionSnapshot {
    pub source_revision: WireU64,
    pub worker: FlowExecutionWorkerSnapshot,
    pub retained_source_count: WireU64,
    pub omitted_executions: WireU64,
    pub off_axis_executions: WireU64,
    /// Loaded native cells outside the actual horizontal viewport, not missing source.
    pub offscreen_executions: WireU64,
    pub clip_left: CanonicalDecimal,
    pub clip_right: CanonicalDecimal,
    pub first_slot: WireU64,
    pub end_slot: WireU64,
    pub scale_mode: FlowExecutionScaleMode,
    pub scale_basis: FlowExecutionScaleBasis,
    pub typed_reference: Option<CanonicalDecimal>,
    pub effective_reference: Option<CanonicalDecimal>,
    pub opening_exclusion_effective: bool,
    pub radius_limit_px: CanonicalDecimal,
    pub merge_support_radius_px: CanonicalDecimal,
    pub ignore_opening: bool,
    pub buy_quantity: CanonicalDecimal,
    pub sell_quantity: CanonicalDecimal,
    pub trade_count: WireU64,
    pub mark_count: WireU64,
    pub marks_truncated: bool,
    pub marks: Vec<FlowExecutionMark>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlowExecutionScaleMode {
    VisibleRegions,
    Typed,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FlowExecutionScaleBasis {
    Empty,
    VisibleRegionMax,
    VisibleOrdinaryRegionMax,
    OpeningOnlyFallback,
    TypedReference,
}
impl From<FlowScaleBasis> for FlowExecutionScaleBasis {
    fn from(value: FlowScaleBasis) -> Self {
        match value {
            FlowScaleBasis::Empty => Self::Empty,
            FlowScaleBasis::VisibleRegionMax => Self::VisibleRegionMax,
            FlowScaleBasis::VisibleOrdinaryRegionMax => Self::VisibleOrdinaryRegionMax,
            FlowScaleBasis::OpeningOnlyFallback => Self::OpeningOnlyFallback,
            FlowScaleBasis::TypedReference => Self::TypedReference,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FlowExecutionWorkerSnapshot {
    pub layout_revision: WireU64,
    pub requested_layout_revision: WireU64,
    pub requested_first_ordinal: WireU64,
    pub requested_end_ordinal: WireU64,
    pub loaded_executions: WireU64,
    /// Loaded source records excluded for nonpositive quantity or invalid tick denominator.
    pub ineligible_executions: WireU64,
    pub computed_through_ordinal: WireU64,
    pub cache_limit_reached: bool,
    pub pending: bool,
    pub submitted_executions: WireU64,
    pub requested_executions: WireU64,
    pub projected_source_count: WireU64,
    pub current_requested_first_ordinal: WireU64,
    pub current_requested_end_ordinal: WireU64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FlowExecutionMember {
    /// Zero-based index in this pane's retained source at source_revision.
    pub ordinal: WireU64,
    pub source_id: WireU64,
    pub candle_slot: WireU64,
    pub accepted_ordinal: WireU64,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FlowExecutionMark {
    pub members: Vec<FlowExecutionMember>,
    pub members_truncated: bool,
    pub native_cells: WireU64,
    pub first_slot: WireU64,
    pub end_slot: WireU64,
    pub candle_position: CanonicalDecimal,
    pub price: CanonicalDecimal,
    pub price_low: CanonicalDecimal,
    pub price_high: CanonicalDecimal,
    pub first_timestamp_ms: i64,
    pub last_timestamp_ms: i64,
    pub buy_quantity: CanonicalDecimal,
    pub sell_quantity: CanonicalDecimal,
    pub trade_count: WireU64,
    /// Actual painted circle radius; gross quantity determines its area.
    pub radius_px: CanonicalDecimal,
    /// Radius of a disc with the same area as the buy sector; not a separate painted disc.
    pub buy_radius_px: CanonicalDecimal,
    /// Radius of a disc with the same area as the sell sector; not a separate painted disc.
    pub sell_radius_px: CanonicalDecimal,
    pub opening_quantity: CanonicalDecimal,
    pub opening_capped: bool,
}
impl From<(&FlowTapeFrame, FlowProgress)> for FlowExecutionSnapshot {
    fn from((frame, progress): (&FlowTapeFrame, FlowProgress)) -> Self {
        let buy: Decimal = frame.dots.iter().map(|dot| dot.mark.buy_quantity).sum();
        let sell: Decimal = frame
            .dots
            .iter()
            .map(|dot| dot.mark.quantity - dot.mark.buy_quantity)
            .sum();
        Self {
            source_revision: WireU64::new(frame.source_revision),
            worker: FlowExecutionWorkerSnapshot {
                layout_revision: WireU64::new(frame.layout_revision),
                requested_layout_revision: WireU64::new(progress.layout_revision),
                requested_first_ordinal: wire_usize(frame.requested_ordinals.start),
                requested_end_ordinal: wire_usize(frame.requested_ordinals.end),
                loaded_executions: wire_usize(frame.loaded_executions),
                ineligible_executions: wire_usize(frame.ineligible_executions),
                computed_through_ordinal: wire_usize(frame.computed_through_ordinal),
                cache_limit_reached: frame.cache_limit_reached,
                pending: progress.pending,
                submitted_executions: wire_usize(progress.submitted_executions),
                requested_executions: wire_usize(progress.requested_executions),
                projected_source_count: wire_usize(frame.source_count),
                current_requested_first_ordinal: wire_usize(progress.requested_first_ordinal),
                current_requested_end_ordinal: wire_usize(progress.requested_end_ordinal),
            },
            retained_source_count: wire_usize(progress.current_source_count),
            omitted_executions: wire_usize(frame.omitted_executions),
            off_axis_executions: wire_usize(frame.off_axis_executions),
            offscreen_executions: wire_usize(frame.offscreen_executions),
            clip_left: canonical_decimal(frame.view.clip_left),
            clip_right: canonical_decimal(frame.view.clip_right),
            first_slot: wire_usize(frame.view.first_slot),
            end_slot: wire_usize(frame.view.end_slot),
            scale_mode: match frame.view.reference {
                FlowReference::VisibleRegions => FlowExecutionScaleMode::VisibleRegions,
                FlowReference::Typed(_) => FlowExecutionScaleMode::Typed,
            },
            scale_basis: frame.scale_basis.into(),
            typed_reference: frame.view.reference.typed().map(canonical_decimal),
            effective_reference: frame.effective_reference.map(canonical_decimal),
            opening_exclusion_effective: frame.opening_exclusion_effective,
            radius_limit_px: canonical_decimal(
                Decimal::from_f32_retain(frame.view.radius_limit).unwrap_or_default(),
            ),
            merge_support_radius_px: canonical_decimal(
                Decimal::from_f32_retain(frame.view.merge_support_radius).unwrap_or_default(),
            ),
            ignore_opening: frame.view.exclude_opening,
            buy_quantity: canonical_decimal(buy),
            sell_quantity: canonical_decimal(sell),
            trade_count: wire_usize(frame.dots.iter().map(|dot| dot.mark.trade_count).sum()),
            mark_count: wire_usize(frame.dots.len()),
            marks_truncated: frame.dots.len() > MAX_MARKS,
            marks: frame.dots.iter().take(MAX_MARKS).map(Into::into).collect(),
        }
    }
}
impl From<&FlowTapeDot> for FlowExecutionMark {
    fn from(dot: &FlowTapeDot) -> Self {
        let mark = &dot.mark;
        let (buy_radius, sell_radius) = dot.side_radii();
        Self {
            members: dot
                .members
                .iter()
                .take(MAX_MEMBERS)
                .map(|member| FlowExecutionMember {
                    ordinal: wire_usize(member.ordinal),
                    source_id: WireU64::new(member.source_id),
                    candle_slot: wire_usize(member.candle_slot),
                    accepted_ordinal: wire_usize(member.accepted_ordinal),
                })
                .collect(),
            members_truncated: dot.members.len() > MAX_MEMBERS,
            native_cells: wire_usize(dot.native_cells),
            first_slot: wire_usize(dot.first_slot),
            end_slot: wire_usize(dot.end_slot),
            candle_position: canonical_decimal(dot.candle_position),
            price: canonical_decimal(mark.price),
            price_low: canonical_decimal(mark.price_bucket),
            price_high: canonical_decimal(mark.price_bucket + mark.price_span),
            first_timestamp_ms: mark.first_timestamp_ms,
            last_timestamp_ms: mark.last_timestamp_ms,
            buy_quantity: canonical_decimal(mark.buy_quantity),
            sell_quantity: canonical_decimal(mark.quantity - mark.buy_quantity),
            trade_count: wire_usize(mark.trade_count),
            radius_px: canonical_decimal(Decimal::from_f32_retain(dot.radius).unwrap_or_default()),
            buy_radius_px: canonical_decimal(
                Decimal::from_f32_retain(buy_radius).unwrap_or_default(),
            ),
            sell_radius_px: canonical_decimal(
                Decimal::from_f32_retain(sell_radius).unwrap_or_default(),
            ),
            opening_quantity: canonical_decimal(dot.opening_quantity),
            opening_capped: dot.opening_capped,
        }
    }
}

#[cfg(test)]
#[path = "tests/flow_execution.rs"]
mod tests;
