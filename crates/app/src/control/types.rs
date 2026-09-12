//! Shared owned DTO vocabulary for application snapshot modules.

use quantick_control::wire::{CanonicalDecimal, WireU64};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    pane::{ChartPane, PaneSide},
    state::BarSpec,
    tab::{CanvasLayout, Tab},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PaneSideDto {
    Flow,
    Time,
}

impl From<PaneSide> for PaneSideDto {
    fn from(side: PaneSide) -> Self {
        match side {
            PaneSide::Flow => Self::Flow,
            PaneSide::Time(_) => Self::Time,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CanvasLayoutDto {
    Flow,
    Time,
    TimeAndFlow,
    TimeTimeAndFlow,
}

impl From<CanvasLayout> for CanvasLayoutDto {
    fn from(layout: CanvasLayout) -> Self {
        match layout {
            CanvasLayout::Single => Self::Flow,
            CanvasLayout::Time => Self::Time,
            CanvasLayout::TimeAndFlow => Self::TimeAndFlow,
            CanvasLayout::TimeTimeAndFlow => Self::TimeTimeAndFlow,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct DecimalRange {
    pub low: CanonicalDecimal,
    pub high: CanonicalDecimal,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct BarSpecDto {
    pub kind: String,
    pub parameter: CanonicalDecimal,
    pub parameter_unit: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imbalance_unit: Option<String>,
}

impl From<&BarSpec> for BarSpecDto {
    fn from(spec: &BarSpec) -> Self {
        match spec {
            BarSpec::Tick(count) => Self {
                kind: "tick".to_owned(),
                parameter: canonical_u64(*count),
                parameter_unit: "trades".to_owned(),
                imbalance_unit: None,
            },
            BarSpec::Volume(quantity) => Self {
                kind: "volume".to_owned(),
                parameter: canonical_decimal(*quantity),
                parameter_unit: "base_asset_quantity".to_owned(),
                imbalance_unit: None,
            },
            BarSpec::Dollar(notional) => Self {
                kind: "dollar".to_owned(),
                parameter: canonical_decimal(*notional),
                parameter_unit: "quote_asset_notional".to_owned(),
                imbalance_unit: None,
            },
            BarSpec::Time(interval_ms) => Self {
                kind: "time".to_owned(),
                parameter: canonical_i64(*interval_ms),
                parameter_unit: "milliseconds".to_owned(),
                imbalance_unit: None,
            },
            BarSpec::Imbalance(unit, target) => Self {
                kind: "imbalance".to_owned(),
                parameter: canonical_u64(*target),
                parameter_unit: "target_trades".to_owned(),
                imbalance_unit: Some(unit.as_str().to_owned()),
            },
            // "deals", not "trades": the tick kind's unit already says
            // `trades` for prints, and a client reading both must not be
            // told the two rules count the same thing.
            BarSpec::Trades(count) => Self {
                kind: "trades".to_owned(),
                parameter: canonical_u64(*count),
                parameter_unit: "deals".to_owned(),
                imbalance_unit: None,
            },
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
pub(crate) struct AvailabilitySnapshot {
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

pub(crate) fn available() -> AvailabilitySnapshot {
    AvailabilitySnapshot {
        available: true,
        reason: None,
    }
}

pub(crate) fn unavailable(reason: &str) -> AvailabilitySnapshot {
    AvailabilitySnapshot {
        available: false,
        reason: Some(reason.to_owned()),
    }
}

/// The panes the active layout actually shows, in the order they are drawn.
///
/// One rule for every projection that walks a tab's canvases: a pane the
/// cursor can resolve against is a pane the scene lists, because both ask
/// here.
pub(crate) fn visible_panes(tab: &Tab) -> Vec<(&ChartPane, PaneSide)> {
    let mut panes = Vec::with_capacity(crate::canvas_layout::MAX_CANVAS_PANES);
    if tab.layout.shows_time() {
        let shown = tab.context_panes_shown();
        panes.extend(
            tab.time_panes
                .iter()
                .take(shown)
                .enumerate()
                .map(|(slot, time)| (time, PaneSide::Time(slot))),
        );
    }
    if tab.layout.shows_flow() {
        panes.push((&tab.flow_pane, PaneSide::Flow));
    }
    panes
}

/// Decimal places every screen coordinate on the wire is rounded to.
///
/// One constant, because two scopes are meant to be comparable without either
/// being rounded first: `interaction.cursor` reports where the pointer is and
/// `scene.controls` reports where a control is, and a client that asks whether
/// the first is inside the second must not lose the answer to a rounding
/// difference. Raising it for one of them raises it for both.
pub(crate) const SCREEN_DECIMAL_PLACES: u32 = 3;

pub(crate) fn canonical_decimal(value: Decimal) -> CanonicalDecimal {
    CanonicalDecimal::new(value.normalize().to_string())
        .expect("rust_decimal normalization is canonical")
}

pub(crate) fn canonical_u64(value: u64) -> CanonicalDecimal {
    CanonicalDecimal::new(value.to_string()).expect("u64 text is a canonical decimal")
}

pub(crate) fn canonical_i64(value: i64) -> CanonicalDecimal {
    CanonicalDecimal::new(value.to_string()).expect("i64 text is a canonical decimal")
}

pub(crate) fn canonical_f64(value: f64, decimal_places: u32) -> Option<CanonicalDecimal> {
    if !value.is_finite() {
        return None;
    }
    let value = Decimal::from_f64(value)?
        .round_dp(decimal_places)
        .normalize();
    Some(canonical_decimal(value))
}

pub(crate) fn canonical_f32(value: f32, decimal_places: u32) -> Option<CanonicalDecimal> {
    canonical_f64(f64::from(value), decimal_places)
}

pub(crate) fn wire_usize(value: usize) -> WireU64 {
    WireU64::new(u64::try_from(value).unwrap_or(u64::MAX))
}

/// One `control.*` failure, built from a code the crate declares. The whole
/// control module answers with the same shape, so a client never has to guess
/// which surface refused it.
pub(crate) fn known_error(
    code: &str,
    message: impl AsRef<str>,
    retryable: bool,
) -> quantick_control::error::ControlError {
    quantick_control::error::ControlError::new(
        quantick_control::id::ErrorCode::new(code).expect("static error code is valid"),
        message.as_ref(),
        retryable,
    )
}

/// The wire name of an actor kind — the same text `ActorKind` serializes to,
/// for the places that carry it as plain text (a drawing's author).
pub(crate) fn actor_kind_name(kind: quantick_control::wire::ActorKind) -> &'static str {
    match kind {
        quantick_control::wire::ActorKind::HumanUi => "human_ui",
        quantick_control::wire::ActorKind::Automation => "automation",
        quantick_control::wire::ActorKind::Agent => "agent",
    }
}
