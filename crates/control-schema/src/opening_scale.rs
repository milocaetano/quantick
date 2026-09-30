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

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OpeningScaleTarget {
    #[default]
    Tape,
    Candle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpeningScaleInput {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// Tape is the backward-compatible default; Candle has an independent
    /// transient pane preference and requires no tape worker.
    #[serde(default)]
    pub target: OpeningScaleTarget,
    /// Exclude each day's first recorded 100 ms burst from the automatic
    /// targeted size reference. Preserve its exact volume and cap its radius.
    pub ignore_opening_burst_in_scale: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpeningScaleResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// Tape is the backward-compatible default; Candle has an independent
    /// transient pane preference and requires no tape worker.
    #[serde(default)]
    pub target: OpeningScaleTarget,
    pub ignore_opening_burst_in_scale: bool,
    pub changed: bool,
}

impl OpeningScaleInput {
    pub fn result(self, changed: bool) -> OpeningScaleResult {
        OpeningScaleResult {
            tab_id: self.tab_id,
            pane_id: self.pane_id,
            target: self.target,
            ignore_opening_burst_in_scale: self.ignore_opening_burst_in_scale,
            changed,
        }
    }
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::descriptor(
        OPENING_SCALE_CAPABILITY_ID,
        "Set opening burst size reference",
        "Optionally excludes the first recorded 100 ms burst per UTC date from the targeted size reference. The default target is tape; target candle is independent and works on context panes without a tape worker. Executions, prices and pies remain exact; the opening dot is capped at full radius. Uses the first available recorded burst if session-opening data is missing. Transient until the bubble preset is explicitly saved; defaults off for tape, with no effect on a typed tape reference. The candle preference is transient for its pane and defaults off; a frame with only opening quantities uses an explicitly reported full-scale fallback.",
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
    "tabs[].panes[].opening_scale",
    "the addressed pane reports independent tape and candle opening-burst scaling preferences",
    &[
        "opening_scale_is_default_off_named_permission_checked_and_read_back",
        "candle_opening_scale_is_independent_retry_safe_and_readable_without_a_tape_worker",
        crate::readback::EVERY_OPTIONAL_TEST,
    ],
)];
