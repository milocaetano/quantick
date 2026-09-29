//! Optional opening-burst display scaling; execution facts remain unchanged.

use quantick_control::{
    id::ModuleId,
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    schema::generated_schema,
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const OPENING_SCALE_CAPABILITY_ID: &str = "orderflow.tape.opening_scale.set";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpeningScaleInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// Exclude each day's first recorded 100 ms burst from the automatic
    /// tape size reference. Preserve its exact volume and cap its radius.
    pub ignore_opening_burst_in_scale: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpeningScaleResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub ignore_opening_burst_in_scale: bool,
    pub changed: bool,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::descriptor(
        OPENING_SCALE_CAPABILITY_ID,
        "Set opening burst size reference",
        "Optionally excludes the first recorded 100 ms burst of each day from automatic tape-only dot scaling. Executions, prices and pies remain exact; the opening dot is capped at full radius. Uses the first available recorded burst if session-opening data is missing. Transient until the bubble preset is explicitly saved; defaults off and has no effect on ordinary chart/BTC rendering or a typed size reference.",
        generated_schema::<OpeningScaleInput>(),
    );
    descriptor.module = ModuleId::new("orderflow").expect("static module ID");
    descriptor.output_schema = generated_schema::<OpeningScaleResult>();
    descriptor.persistence = EffectPersistence::Transient;
    descriptor.stale_input_safety = Some(
        "Stable tab and pane IDs are resolved before changing only a reversible display preference; the result and orderflow.bubbles report its actual value.".to_owned(),
    );
    descriptor
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::snapshot(
    OPENING_SCALE_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    "orderflow.bubbles",
    "tabs[].panes[].bubbles.ignore_opening_burst_in_scale",
    "the addressed pane reports the requested opening-burst scaling preference",
    &[
        "opening_scale_is_default_off_named_permission_checked_and_read_back",
        crate::readback::EVERY_OPTIONAL_TEST,
    ],
)];
