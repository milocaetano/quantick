//! Optional opening-burst display scaling; execution facts remain unchanged.

use quantick_control::{
    registry::{CapabilityDescriptor, IdempotencyPolicy},
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
    /// transient FLOW preference, independent of the native Tape worker.
    #[serde(default)]
    pub target: OpeningScaleTarget,
    /// Tape excludes the first recorded 100 ms burst per UTC date and caps it.
    /// Candle excludes only the canonical first region's opening portion;
    /// its radius may grow uncapped while every other region sets the reference.
    pub ignore_opening_burst_in_scale: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpeningScaleResult {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// Tape is the backward-compatible default; Candle has an independent
    /// transient FLOW preference, independent of the native Tape worker.
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
    let mut descriptor = crate::layout::transient_descriptor::<OpeningScaleInput, OpeningScaleResult>(
        OPENING_SCALE_CAPABILITY_ID,
        crate::orderflow::MODULE_ID,
        "Set opening burst size reference",
        "Optionally changes opening calibration without changing executions, prices or grouping. The default target tape excludes the first recorded 100 ms burst per UTC date and caps its radius; it has no effect on a typed tape reference. Target candle requires active tick FLOW bubbles and has an independent pane preference. Both are saved for the asset the pane shows (orderflow.bubbles names it) and come back whenever a tab shows that asset. Target candle: only the region containing the canonical first recorded execution per UTC date excludes its opening portion from the visible reference and may grow uncapped with area proportional to gross volume. Other first-window regions contribute their full volume. Offscreen or evicted anchors never transfer to visible regions. A frame with no ordinary reference uses an explicitly reported full-volume fallback. Both preferences default off; first recorded activity is not an exchange auction flag.",
        "Stable tab and pane IDs are resolved before changing only a reversible display preference; the result and orderflow.bubbles report its actual value.",
    );
    descriptor.persistence = quantick_control::registry::EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::snapshot(
    OPENING_SCALE_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    crate::orderflow::BUBBLES_SCOPE_ID,
    "tabs[].panes[].opening_scale",
    "the addressed pane reports independent tape and candle opening-burst scaling preferences",
    &[
        "opening_scale_is_default_off_named_permission_checked_and_read_back",
        "flow_opening_scale_is_independent_retry_safe_and_refuses_context",
        crate::readback::EVERY_OPTIONAL_TEST,
    ],
)];
