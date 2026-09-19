//! Shared owned DTO vocabulary for host snapshot modules: the wire shapes
//! every projection speaks, and the canonical-number helpers that keep two
//! scopes comparable without either being rounded first.

use quantick_control::wire::{CanonicalDecimal, WireU64};
use quantick_engine::bar_registry::BarConfiguration;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use quantick_control::annotation::PaneSideDto;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanvasLayoutDto {
    Flow,
    Time,
    TimeAndFlow,
    TimeTimeAndFlow,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DecimalRange {
    pub low: CanonicalDecimal,
    pub high: CanonicalDecimal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BarSpecDto {
    pub kind: String,
    pub parameter: CanonicalDecimal,
    pub parameter_unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imbalance_unit: Option<String>,
}

impl From<&BarConfiguration> for BarSpecDto {
    fn from(spec: &BarConfiguration) -> Self {
        Self {
            kind: spec.id().to_owned(),
            parameter: canonical_decimal(spec.parameter()),
            parameter_unit: spec.definition().parameter.unit.to_owned(),
            imbalance_unit: spec.choice().map(str::to_owned),
        }
    }
}

/// Whether something is there to be read or acted on, and the coded reason
/// when it is not.
///
/// The reason is a stable identifier, never the sentence an interface shows a
/// human: a client made to parse that sentence would break the day it is
/// reworded, and translating the interface would break every such client at
/// once.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AvailabilitySnapshot {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub fn available() -> AvailabilitySnapshot {
    AvailabilitySnapshot {
        available: true,
        reason: None,
    }
}

pub fn unavailable(reason: &str) -> AvailabilitySnapshot {
    AvailabilitySnapshot {
        available: false,
        reason: Some(reason.to_owned()),
    }
}

/// Decimal places every screen coordinate on the wire is rounded to.
///
/// One constant, because two scopes are meant to be comparable without either
/// being rounded first: `interaction.cursor` reports where the pointer is and
/// `scene.controls` reports where a control is, and a client that asks whether
/// the first is inside the second must not lose the answer to a rounding
/// difference. Raising it for one of them raises it for both.
pub const SCREEN_DECIMAL_PLACES: u32 = 3;

pub fn canonical_decimal(value: Decimal) -> CanonicalDecimal {
    CanonicalDecimal::new(value.normalize().to_string())
        .expect("rust_decimal normalization is canonical")
}

pub fn canonical_f64(value: f64, decimal_places: u32) -> Option<CanonicalDecimal> {
    if !value.is_finite() {
        return None;
    }
    let value = Decimal::from_f64(value)?
        .round_dp(decimal_places)
        .normalize();
    Some(canonical_decimal(value))
}

pub fn canonical_f32(value: f32, decimal_places: u32) -> Option<CanonicalDecimal> {
    canonical_f64(f64::from(value), decimal_places)
}

pub fn wire_usize(value: usize) -> WireU64 {
    WireU64::new(u64::try_from(value).unwrap_or(u64::MAX))
}

/// One `control.*` failure, built from a code the crate declares. The whole
/// control module answers with the same shape, so a client never has to guess
/// which surface refused it — and the admission checks in
/// `quantick-control-host` build theirs with this same function.
pub use crate::admission::known_error;

/// The wire name of an actor kind — the same text `ActorKind` serializes to,
/// for the places that carry it as plain text (a drawing's author).
pub fn actor_kind_name(kind: quantick_control::wire::ActorKind) -> &'static str {
    match kind {
        quantick_control::wire::ActorKind::HumanUi => "human_ui",
        quantick_control::wire::ActorKind::Automation => "automation",
        quantick_control::wire::ActorKind::Agent => "agent",
    }
}
