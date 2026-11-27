//! The chart context menu's Save drawing preference, independent of workspace Save.
use crate::wire::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const DRAWING_AUTOSAVE_CAPABILITY_ID: &str = "chart.drawings.autosave.set";
pub const EVENT_KIND: &str = "chart.drawings.autosave.changed";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    /// Omit to inspect. True saves current drawings and resumes autosave;
    /// false suspends drawing writes while retaining the saved snapshot.
    #[serde(default)]
    pub on: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Output {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub on: bool,
    pub changed: bool,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::wire::layout::transient_descriptor::<Input, Output>(
        DRAWING_AUTOSAVE_CAPABILITY_ID,
        crate::wire::chart::MODULE_ID,
        "Set drawing autosave",
        "Reads or sets the chart context menu's Save drawing checkbox. Omit on to inspect. On by default, persisted per venue/asset and stable workspace/layout across timeframes. False suspends drawing writes and keeps the saved snapshot; true saves current drawings and resumes autosave. Drawing writes never save pending workspace configuration. Workspace Save and discard never change saved drawings or this preference. Unknown tab or pane and malformed input are refused before changing state. A scope not ready or failed write returns retryable control.capability_unavailable with reason drawing_autosave_not_written; the same explicit value can be retried.",
        "The call names the desired value; repeating it is harmless. Omit on to read the effective checkbox state after a lost answer.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor
}

pub const READBACKS: &[Readback] = &[journal(
    DRAWING_AUTOSAVE_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    EVENT_KIND,
    "payload.drawing_autosave.on",
    "the event for the addressed tab and pane reports the requested value; calling again without on also reads the effective preference",
    &[
        EVERY_OPTIONAL_TEST,
        "drawing_autosave_call_inspects_sets_and_retries",
    ],
)];

pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    use quantick_control::schema::generated_schema as schema;
    vec![
        (
            "chart-drawings-autosave-input-v1.schema.json",
            schema::<Input>(),
        ),
        (
            "chart-drawings-autosave-result-v1.schema.json",
            schema::<Output>(),
        ),
    ]
}
