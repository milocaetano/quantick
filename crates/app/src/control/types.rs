//! Shared owned DTO vocabulary for application snapshot modules.

use quantick_control::wire::{CanonicalDecimal, WireU64};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{pane::PaneSide, state::BarSpec, tab::CanvasLayout};

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
            PaneSide::Time => Self::Time,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CanvasLayoutDto {
    Flow,
    Time,
    TimeAndFlow,
}

impl From<CanvasLayout> for CanvasLayoutDto {
    fn from(layout: CanvasLayout) -> Self {
        match layout {
            CanvasLayout::Single => Self::Flow,
            CanvasLayout::Time => Self::Time,
            CanvasLayout::TimeAndFlow => Self::TimeAndFlow,
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
        }
    }
}

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
