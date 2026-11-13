//! The note editor and repeat-placement workflow on the control plane.
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, journal};
use quantick_control::{
    annotation::ANNOTATE_MODULE_ID,
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    wire::{CanonicalDecimal, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SET_CAPABILITY_ID: &str = "annotate.text.set";
pub const REPEAT_CAPABILITY_ID: &str = "annotate.text.repeat";
pub const CHANGED: &str = "annotate.text.changed";
pub const REPEATED: &str = "annotate.text.repeated";
pub const MAX_TEXT_BYTES: usize = 16_384;

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextTarget {
    pub tab_id: WireU64,
    pub pane_id: WireU64,
    pub drawing_id: WireU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Proportional,
    Monospace,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextProperties {
    /// Canonical decimal string from 6 through 72 screen pixels, inclusive.
    pub size_px: CanonicalDecimal,
    pub family: Family,
    pub bold: bool,
    pub italic: bool,
    /// Straight RGBA bytes.
    pub color: [u8; 4],
    pub background: Option<[u8; 4]>,
    pub border: Option<[u8; 4]>,
    /// Canonical decimal string from 40 through 1200 screen pixels; null disables wrapping.
    pub wrap_width_px: Option<CanonicalDecimal>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextAnchor {
    pub time_unix_ms: i64,
    pub price: quantick_control::wire::CanonicalDecimal,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetInput {
    pub target: TextTarget,
    /// Omit properties and text to inspect. Words are never returned.
    pub properties: Option<TextProperties>,
    #[schemars(length(max = 16384))]
    pub text: Option<String>,
    /// Reposition the note on its existing value axis.
    pub anchor: Option<TextAnchor>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepeatInput {
    pub source: TextTarget,
    pub anchor: TextAnchor,
    /// New content only; omission creates an empty, independently editable note.
    #[schemars(length(max = 16384))]
    pub text: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct TextResult {
    pub target: TextTarget,
    pub properties: TextProperties,
    pub text_present: bool,
    pub changed: bool,
}

pub fn set_descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<SetInput, TextResult>(
        SET_CAPABILITY_ID,
        ANNOTATE_MODULE_ID,
        "Edit or inspect an anchored note",
        "Reads and optionally edits a text drawing's family, size, bold, italic, RGBA text/background/border colors, wrap width, words and market-time/price anchor. Omit properties, text and anchor to inspect. Properties replace the whole appearance. Words are redacted in results and events. Locked notes refuse edits. Invalid inputs change nothing. One undo step; saves with drawings. For a new note use annotate.label.create, then this call; for repeated placement use annotate.text.repeat.",
        "Explicit values make retries harmless; identifiers are pane-scoped and locked notes refuse edits.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor
}

pub fn repeat_descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<RepeatInput, TextResult>(
        REPEAT_CAPABILITY_ID,
        ANNOTATE_MODULE_ID,
        "Place another note with the same style",
        "The text properties bar's T workflow: place an independent note at the supplied market time and price, copying only the source note's appearance. Words are empty unless explicitly supplied; source content, lock and identity are never copied. The source remains unchanged. One undo step. A required idempotency key prevents a transport retry from placing two notes.",
        "This adds an independent note and does not overwrite the source; unknown source or time is refused.",
    );
    descriptor.persistence = EffectPersistence::Durable;
    descriptor.idempotency = IdempotencyPolicy::Required;
    descriptor.expected_cost.max_response_bytes = Some(16_384);
    descriptor
}

pub const READBACKS: &[Readback] = &[
    journal(
        SET_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        CHANGED,
        "payload.target",
        "the event names the note and its resulting appearance",
        &[EVERY_OPTIONAL_TEST],
    ),
    journal(
        REPEAT_CAPABILITY_ID,
        IdempotencyPolicy::Required,
        REPEATED,
        "payload.target",
        "the event identifies the independent new note",
        &["text_repeat_requires_authority_and_key_and_replays_once"],
    ),
];
