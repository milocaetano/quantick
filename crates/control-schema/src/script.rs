//! An indicator written in conversation: compile Quantick Pine, read the
//! diagnostics as data, attach what compiled, detach it again.
//!
//! This is the closed loop the plan calls the highest-value capability on its
//! list (§PR 5b), and it is cheap because both halves already exist:
//! `quantick_pine::compile` returns `Vec<PineError>` with a stable code, a
//! byte span, a message and notes, and the indicator host is headless. What
//! this module adds is refusing to render them into a string first — an agent
//! that has to parse "line 4, column 12: …" back out of prose cannot fix its
//! own script reliably, and a rendered error is exactly the pixels-instead-of-
//! data failure the control plane exists to end.

use quantick_control_host::authority::{
    ANNOTATE_EFFECT_ID, ANNOTATE_PERMISSION_ID, CAPABILITY_VERSION, NO_CONFIRMATION_ID,
    SCRIPT_MODULE_ID, SCRIPT_PERMISSION_ID, UI_BOUNDED_COST_ID,
};
use quantick_control_host::wire::PaneSideDto;

use std::collections::BTreeSet;

use quantick_control::{
    error::{ControlError, codes},
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, ModuleId, PermissionId,
        RiskFlagId,
    },
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RevisionPolicy,
    },
    schema::generated_schema,
    wire::WireU64,
};

use schemars::JsonSchema;

use serde::{Deserialize, Serialize};

use serde_json::{Value, json};

use quantick_control_host::admission::known_error;

/// The module both script capabilities belong to — the same module the
/// indicator scopes will register under, because a capability belongs to the
/// module its ID names (contract §5).

pub const ATTACH_CAPABILITY_ID: &str = "indicator.script.attach";

pub const DETACH_CAPABILITY_ID: &str = "indicator.script.detach";

pub const SCRIPT_ATTACHED_EVENT_KIND: &str = "indicator.script.attached";

pub const SCRIPT_DETACHED_EVENT_KIND: &str = "indicator.script.detached";

/// The longest script this capability accepts. An indicator written in a
/// conversation is tens of lines; the bound keeps one call from carrying a
/// library.
pub const SCRIPT_MAX_BYTES: usize = 64 * 1024;

/// The longest display name an attached script may carry.
pub const SCRIPT_NAME_MAX_BYTES: usize = 80;

/// What an attach takes: the script, its name, and optionally which pane.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttachInput {
    #[schemars(length(min = 1, max = SCRIPT_NAME_MAX_BYTES))]
    pub name: String,
    /// Quantick Pine source, exactly as a `.pine` file would hold it.
    #[schemars(length(min = 1, max = SCRIPT_MAX_BYTES))]
    pub source: String,
}

/// What an attach returns on success: the slot to detach later, and where it
/// went. A failure is an error with the diagnostics in its details, never a
/// success carrying a rendered message.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AttachResult {
    pub slot_id: WireU64,
    pub tab_id: WireU64,
    pub pane_side: PaneSideDto,
    pub name: String,
    /// The inputs the script declared, by name, so a caller can see what it
    /// can later bind without reading the source back.
    pub declared_inputs: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DetachInput {
    pub slot_id: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DetachResult {
    pub slot_id: WireU64,
    pub detached: bool,
}

/// One compile problem, as data: the stable code, the byte span, the message
/// and the notes — never a rendered line an agent has to parse.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScriptDiagnostic {
    /// The stable `pine.*` code.
    pub code: String,
    /// First byte of the offending text.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
    /// 1-based line and column of `start`, so a client can point at it
    /// without counting bytes itself.
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub notes: Vec<String>,
}

pub fn script_permissions() -> BTreeSet<PermissionId> {
    [ANNOTATE_PERMISSION_ID, SCRIPT_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect()
}

pub fn script_descriptor(
    id: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
) -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(id).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: title.to_owned(),
        description: description.to_owned(),
        module: ModuleId::new(SCRIPT_MODULE_ID).expect("static module ID is valid"),
        input_schema,
        output_schema,
        examples: Vec::new(),
        effect: EffectId::new(ANNOTATE_EFFECT_ID).expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        idempotency: IdempotencyPolicy::Forbidden,
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "Attaching adds a slot and edits none; detaching names the slot it removes, and a slot that is gone reports that it was not there."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Durable,
        reversible: true,
        destructive: false,
        risk_reducing: false,
        required_permissions: script_permissions(),
        preconditions: Vec::new(),
        confirmation_class: ConfirmationClassId::new(NO_CONFIRMATION_ID)
            .expect("static confirmation class is valid"),
        availability: Availability::available(),
        expected_cost: ExpectedCost {
            class: CostClassId::new(UI_BOUNDED_COST_ID).expect("static cost ID is valid"),
            max_items: None,
            max_response_bytes: Some(quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES),
        },
        pagination: None,
    }
}

pub fn attach_descriptor() -> CapabilityDescriptor {
    script_descriptor(
        ATTACH_CAPABILITY_ID,
        "Attach a script indicator",
        "Compiles Quantick Pine and attaches the indicator it produces to the focused pane. A script that does not compile is refused with its diagnostics as structured data — code, span, line, column, message and notes — never as a rendered string.",
        generated_schema::<AttachInput>(),
        generated_schema::<AttachResult>(),
    )
}

pub fn detach_descriptor() -> CapabilityDescriptor {
    script_descriptor(
        DETACH_CAPABILITY_ID,
        "Detach a script indicator",
        "Removes one attached slot, leaving the pane exactly as it was before the matching attach.",
        generated_schema::<DetachInput>(),
        generated_schema::<DetachResult>(),
    )
}

/// Every compile problem, as data, on one refusal.
pub fn compile_error(errors: &[quantick_pine::PineError], source: &str) -> ControlError {
    let diagnostics = errors
        .iter()
        .map(|error| {
            // Pine floors an interior byte offset to a character boundary.
            let (line, column) = error.span.line_col(source);
            ScriptDiagnostic {
                code: error.code.as_str().to_owned(),
                start: u32::try_from(error.span.start).unwrap_or(u32::MAX),
                end: u32::try_from(error.span.end).unwrap_or(u32::MAX),
                line: u32::try_from(line).unwrap_or(u32::MAX),
                column: u32::try_from(column).unwrap_or(u32::MAX),
                message: error.message.clone(),
                notes: error.notes.clone(),
            }
        })
        .collect::<Vec<_>>();
    let mut control_error =
        known_error(codes::INVALID_REQUEST, "the script does not compile", false);
    control_error.context.details = Some(json!({ "diagnostics": diagnostics }));
    control_error.context.next_steps = vec!["Fix the reported spans and attach again.".to_owned()];
    control_error
}
