//! Independent bubble magnification, applied after factual grouping and sizing.
use quantick_control::{
    error::ControlError,
    id::PreconditionId,
    registry::{CapabilityDescriptor, IdempotencyPolicy, PreconditionDescriptor},
    wire::{CanonicalDecimal, WireU64},
};
use quantick_orderflow::{MAX_BUBBLE_DISPLAY_SCALE, MIN_BUBBLE_DISPLAY_SCALE};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const BUBBLE_SCALE_CAPABILITY_ID: &str = "orderflow.bubbles.size.set";

/// The addressed pane draws tick bars. Candle bubble size and tick spheres
/// need it; a refusal naming it is fixed by switching the pane to a tick
/// chart, never by retrying.
pub const TICK_CHART_PRECONDITION: &str = "orderflow.bubbles.tick_chart";
/// The addressed bubble display is drawing: the tape's bubbles for `tape`,
/// candle bubbles beside the candles for `candle`. Fixed by switching that
/// display on.
pub const DISPLAY_ACTIVE_PRECONDITION: &str = "orderflow.bubbles.display_active";

/// A declared precondition, for a descriptor's `preconditions` and the
/// `violated_precondition` of the refusal that names it.
#[must_use]
pub fn precondition(id: &'static str, description: &str) -> PreconditionDescriptor {
    PreconditionDescriptor {
        id: PreconditionId::new(id).expect("bubble precondition IDs are valid registry IDs"),
        description: description.to_owned(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BubbleScaleTarget {
    Tape,
    Candle,
}

impl From<BubbleScaleTarget> for quantick_orderflow::BubbleScaleTarget {
    fn from(target: BubbleScaleTarget) -> Self {
        match target {
            BubbleScaleTarget::Tape => Self::Tape,
            BubbleScaleTarget::Candle => Self::Candle,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BubbleScaleInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// The Tape or tick-chart bubbles on the addressed pane.
    pub target: BubbleScaleTarget,
    /// Canonical decimal string from 0.25 through 3, inclusive. Multiplies
    /// final radii; "1" preserves the current appearance.
    pub scale: CanonicalDecimal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BubbleScaleResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub target: BubbleScaleTarget,
    /// The actual multiplier retained by the addressed display owner.
    pub scale: CanonicalDecimal,
    pub changed: bool,
}

impl BubbleScaleInput {
    pub fn validated_scale(&self) -> Result<f32, ControlError> {
        let invalid = || {
            ControlError::invalid_request(format!(
                "bubble scale must be finite and between {MIN_BUBBLE_DISPLAY_SCALE} and {MAX_BUBBLE_DISPLAY_SCALE}"
            ))
        };
        let value = self.scale.as_str().parse::<f64>().map_err(|_| invalid())?;
        quantick_orderflow::admitted_bubble_display_scale(value).ok_or_else(invalid)
    }

    pub fn result(self, scale: f32, changed: bool) -> Result<BubbleScaleResult, ControlError> {
        let scale = CanonicalDecimal::new(scale.to_string())
            .map_err(|error| ControlError::invalid_request(error.to_string()))?;
        Ok(BubbleScaleResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            target: self.target,
            scale,
            changed,
        })
    }
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<BubbleScaleInput, BubbleScaleResult>(
        BUBBLE_SCALE_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Set bubble size",
        "Sets the independent radius multiplier for Tape or tick-chart bubbles on one pane. The target must already be active. One preserves its appearance; changing size preserves execution quantities, sides, membership, grouping, source positions and the other target's scale. The existing asset save switch governs persistence: saved for the displayed asset when enabled, otherwise retained for this pane's session.",
        "Stable tab and pane IDs select the display owner. The result and orderflow.bubbles report the actual multiplier; repeating a value has no further effect.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor.preconditions = vec![
        precondition(
            TICK_CHART_PRECONDITION,
            "Candle bubble size needs a pane that draws tick bars.",
        ),
        precondition(
            DISPLAY_ACTIVE_PRECONDITION,
            "The target's bubbles are drawing: the tape's bubbles for tape, candle bubbles beside the candles (not Tape alone) for candle.",
        ),
    ];
    descriptor
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::snapshot(
    BUBBLE_SCALE_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].bubbles.display_scale",
    "the addressed pane reports independent tape and candle bubble radius multipliers",
    &[
        "bubble_size_is_named_permission_checked_retry_safe_and_read_back",
        "bubble_size_refuses_invalid_values_stale_targets_and_inactive_context",
        crate::readback::EVERY_OPTIONAL_TEST,
    ],
)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bubble_scale_validation_rejects_nonfinite_and_out_of_range_values() {
        let mut input = BubbleScaleInput {
            tab_id: WireU64::new(1),
            pane_id: WireU64::new(2),
            target: BubbleScaleTarget::Tape,
            scale: CanonicalDecimal::new("1").unwrap(),
        };
        for value in [MIN_BUBBLE_DISPLAY_SCALE, 1.0, MAX_BUBBLE_DISPLAY_SCALE] {
            input.scale = CanonicalDecimal::new(value.to_string()).unwrap();
            assert_eq!(input.validated_scale().unwrap(), value);
        }
        for value in ["0", "-1", "4", "0.2499999999", "3.00000001"] {
            input.scale = CanonicalDecimal::new(value).unwrap();
            assert!(input.validated_scale().is_err());
        }
        input.scale = CanonicalDecimal::new("9".repeat(400)).unwrap();
        assert!(input.validated_scale().is_err());
        for value in ["NaN", "inf", "-inf", "1.0", "1e0"] {
            assert!(CanonicalDecimal::new(value).is_err());
        }
    }

    #[test]
    fn bubble_scale_schemas_and_results_use_control_decimal_strings() {
        use quantick_control::canonical::validate_control_value;
        let descriptor = descriptor();
        assert!(validate_control_value(&descriptor.input_schema).is_ok());
        assert!(validate_control_value(&descriptor.output_schema).is_ok());
        let input: BubbleScaleInput = serde_json::from_value(serde_json::json!({
            "tab_id": "1", "pane_id": "2", "target": "candle", "scale": "0.5"
        }))
        .unwrap();
        let result = serde_json::to_value(input.result(0.5, true).unwrap()).unwrap();
        assert_eq!(result["scale"], "0.5");
        assert!(validate_control_value(&result).is_ok());
    }
}
