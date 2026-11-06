//! Bounded live appearance, with a host-selected persistence destination.
use crate::readback::{EVERY_OPTIONAL_TEST, Readback, snapshot};
use quantick_control::{
    registry::{CapabilityDescriptor, EffectPersistence, IdempotencyPolicy},
    schema::generated_schema,
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const APPEARANCE_CAPABILITY_ID: &str = "appearance.apply";
pub const SCOPE_ID: &str = "appearance.state";
pub const MODULE_ID: &str = "appearance";

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplyInput {
    /// Versioned TOML; schema maxLength is 65536 Unicode characters.
    /// Runtime additionally caps UTF-8 bytes at 65536 before parsing.
    #[schemars(length(max = 65536))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Select dark or light instead of source. Exactly one is required.
    /// Durable choices preserve the original file using a sibling choice store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(regex(pattern = "^(dark|light)$"))]
    pub builtin: Option<String>,
    /// False is session-only. True writes the selected store before publication.
    #[serde(default)]
    pub persist: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub struct AppearanceSnapshot {
    /// dark, light, or custom; exact bundled source identity, never RGB matching.
    pub choice: String,
    /// Paint revision: semantic equality does not invalidate rendering caches.
    pub revision: WireU64,
    /// bundled_dark, session, or selected_store; never a local path.
    pub origin: String,
    /// The active candidate's original TOML, including omitted Dark fallbacks.
    #[schemars(length(max = 65536))]
    pub source: String,
    /// Last successful load/save. Files are not polled for external edits.
    pub durable: bool,
    pub writes_enabled: bool,
    /// Startup fell back to Dark after an invalid/unreadable selected file.
    pub load_failed: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ApplyResult {
    pub state: AppearanceSnapshot,
    /// Semantic palette/typography change; source/durability may change without it.
    pub changed: bool,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut value = crate::recovery::descriptor(
        APPEARANCE_CAPABILITY_ID,
        "Apply appearance configuration",
        "Apply exactly one of source (version 1 TOML) or builtin (dark/light). Session-only by default. persist=true saves before publication; builtin choices preserve the original file in a sibling choice store, which becomes the active persistence destination. Saves-off refuses persistent changes. Input never selects a path. Invalid candidates/write failures preserve state. Inspect appearance.state; keyed retries replay without another write. External files are not polled.",
        false,
        generated_schema::<ApplyInput>(),
    );
    value.module = quantick_control::id::ModuleId::new(MODULE_ID).expect("static module");
    value.required_permissions.insert(
        quantick_control::id::PermissionId::new("cockpit.layout").expect("static permission"),
    );
    value.output_schema = generated_schema::<ApplyResult>();
    value.persistence = EffectPersistence::Durable;
    value.stale_input_safety = Some("The whole candidate replaces the prior theme. No expected revision is checked; inspect appearance.state before applying. Existing authored chart, drawing and preset overrides are preserved.".into());
    value
}

pub const READBACKS: &[Readback] = &[snapshot(
    APPEARANCE_CAPABILITY_ID,
    IdempotencyPolicy::Optional,
    SCOPE_ID,
    "source",
    "the active source matches the candidate; durable reports the last successful load/save",
    &[EVERY_OPTIONAL_TEST],
)];

/// Published wire documents, owned beside their shapes.
pub fn documents() -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "appearance-apply-input-v1.schema.json",
            generated_schema::<ApplyInput>(),
        ),
        (
            "appearance-apply-result-v1.schema.json",
            generated_schema::<ApplyResult>(),
        ),
        (
            "observer-appearance-state-v1.schema.json",
            generated_schema::<AppearanceSnapshot>(),
        ),
    ]
}
