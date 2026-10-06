//! Explicit per-pane price framing, shared by the chart and independent tape.

use quantick_control::{
    error::ControlError,
    registry::{CapabilityDescriptor, IdempotencyPolicy},
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::chart::{MODULE_ID, SCOPE_ID, ViewportSnapshot};
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};

pub const PRICE_AXIS_CAPABILITY_ID: &str = "chart.price_axis.set";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PriceAxisInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub mode: PriceAxisMode,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PriceAxisMode {
    /// Resume the pane's own automatic price fit.
    Auto,
    /// Hold this range until the operator changes or resets it.
    Manual {
        low: CanonicalDecimal,
        high: CanonicalDecimal,
    },
}

impl PriceAxisMode {
    /// Parse the same finite ordered bounds from a `low:high` launch value.
    #[must_use]
    pub fn parse_range(value: &str) -> Option<(f64, f64)> {
        let (low, high) = value.split_once(':')?;
        Self::Manual {
            low: CanonicalDecimal::new(low.trim()).ok()?,
            high: CanonicalDecimal::new(high.trim()).ok()?,
        }
        .range()
        .ok()
        .flatten()
    }

    pub fn range(&self) -> Result<Option<(f64, f64)>, ControlError> {
        let Self::Manual { low, high } = self else {
            return Ok(None);
        };
        let invalid = || {
            ControlError::invalid_request("manual price bounds must be finite with low below high")
        };
        let low = low.as_str().parse::<f64>().map_err(|_| invalid())?;
        let high = high.as_str().parse::<f64>().map_err(|_| invalid())?;
        if !low.is_finite() || !high.is_finite() || low >= high || !(high - low).is_finite() {
            return Err(invalid());
        }
        Ok(Some((low, high)))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct PriceAxisResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub viewport: ViewportSnapshot,
}

pub fn descriptor() -> CapabilityDescriptor {
    crate::layout::transient_descriptor::<PriceAxisInput, PriceAxisResult>(
        PRICE_AXIS_CAPABILITY_ID,
        MODULE_ID,
        "Set a pane's price axis",
        "Sets an explicit manual price range or resumes the pane's own automatic fit. Applies to the chart or independent tape without changing its time window or other panes.",
        "Stable tab and pane IDs select one price axis; setting the same range or automatic mode is harmless, and the result reports the actual framing.",
    )
}

pub const READBACKS: &[Readback] = &[snapshot(
    PRICE_AXIS_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    SCOPE_ID,
    "panes[].viewport",
    "The target pane reports manual framing with the requested price_range, or automatic fitting after reset.",
    &[
        EVERY_OPTIONAL_TEST,
        "the_price_axis_call_holds_manual_tape_framing_and_reads_it_back_without_moving_the_left_pane",
    ],
)];
