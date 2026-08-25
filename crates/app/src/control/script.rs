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

use std::collections::BTreeSet;

use quantick_control::{
    error::{ControlError, codes},
    id::{
        CapabilityId, ConfirmationClassId, CostClassId, EffectId, EventKind, ModuleId,
        PermissionId, RiskFlagId,
    },
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::{ActorContext, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{app::QuantickApp, metrics};

use super::{
    actions::{ANNOTATE_EFFECT_ID, ANNOTATE_PERMISSION_ID, ActionRegistry},
    gateway::ControlAccess,
    journal::{EventActor, NewEvent},
    types::{PaneSideDto, known_error},
};

/// The module both script capabilities belong to — the same module the
/// indicator scopes will register under, because a capability belongs to the
/// module its ID names (contract §5).
pub(crate) const SCRIPT_MODULE_ID: &str = "indicator";
/// The scope that lets an operator put code on the chart.
pub(crate) const SCRIPT_PERMISSION_ID: &str = "annotate.script";

pub(crate) const ATTACH_CAPABILITY_ID: &str = "indicator.script.attach";
pub(crate) const DETACH_CAPABILITY_ID: &str = "indicator.script.detach";

pub(crate) const SCRIPT_ATTACHED_EVENT_KIND: &str = "indicator.script.attached";
pub(crate) const SCRIPT_DETACHED_EVENT_KIND: &str = "indicator.script.detached";

const CAPABILITY_VERSION: u32 = 1;
const NO_CONFIRMATION_ID: &str = "none";
const UI_BOUNDED_COST_ID: &str = "ui_bounded";

/// The longest script this capability accepts. An indicator written in a
/// conversation is tens of lines; the bound keeps one call from carrying a
/// library.
pub(crate) const SCRIPT_MAX_BYTES: usize = 64 * 1024;
/// The longest display name an attached script may carry.
const SCRIPT_NAME_MAX_BYTES: usize = 80;

/// What an attach takes: the script, its name, and optionally which pane.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AttachInput {
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
pub(crate) struct AttachResult {
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
pub(crate) struct DetachInput {
    pub slot_id: WireU64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct DetachResult {
    pub slot_id: WireU64,
    pub detached: bool,
}

/// One compile problem, as data: the stable code, the byte span, the message
/// and the notes — never a rendered line an agent has to parse.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct ScriptDiagnostic {
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

/// Dock the script actions.
pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(attach_descriptor(), attach_script)?;
    registry.register(detach_descriptor(), detach_script)?;
    Ok(())
}

fn script_permissions() -> BTreeSet<PermissionId> {
    [ANNOTATE_PERMISSION_ID, SCRIPT_PERMISSION_ID]
        .into_iter()
        .map(|id| PermissionId::new(id).expect("static permission ID is valid"))
        .collect()
}

fn script_descriptor(
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

fn attach_descriptor() -> CapabilityDescriptor {
    script_descriptor(
        ATTACH_CAPABILITY_ID,
        "Attach a script indicator",
        "Compiles Quantick Pine and attaches the indicator it produces to the focused pane. A script that does not compile is refused with its diagnostics as structured data — code, span, line, column, message and notes — never as a rendered string.",
        generated_schema::<AttachInput>(),
        generated_schema::<AttachResult>(),
    )
}

fn detach_descriptor() -> CapabilityDescriptor {
    script_descriptor(
        DETACH_CAPABILITY_ID,
        "Detach a script indicator",
        "Removes one attached slot, leaving the pane exactly as it was before the matching attach.",
        generated_schema::<DetachInput>(),
        generated_schema::<DetachResult>(),
    )
}

fn attach_script(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: AttachInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    // Compile before touching the chart: the diagnostics are the point of
    // this capability, and a script that cannot run never becomes a slot the
    // trader has to clean up.
    let compiled = quantick_pine::compile(&input.source, &input.name)
        .map_err(|errors| compile_error(&errors, &input.source))?;
    let declared_inputs = compiled
        .inputs
        .iter()
        .map(|spec| spec.name().to_owned())
        .collect::<Vec<_>>();
    // Attached *by an operator* when it was not the trader's own hand, which
    // is what the detach then checks before it removes anything.
    let by_operator = actor.actor_kind != quantick_control::wire::ActorKind::HumanUi;
    let (tab_id, pane_side, slot) =
        app.attach_script_indicator(input.name.clone(), input.source, by_operator);
    let result = AttachResult {
        slot_id: WireU64::new(slot.0),
        tab_id: WireU64::new(tab_id),
        pane_side,
        name: input.name,
        declared_inputs,
    };
    journal_script(access, actor, SCRIPT_ATTACHED_EVENT_KIND, &result)?;
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("attach result: {error}")))
}

fn detach_script(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: DetachInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let detached = app
        .detach_script_indicator(input.slot_id.get())
        .map_err(|()| {
            known_error(
                codes::PERMISSION_DENIED,
                "that indicator is the trader's own; this tier detaches only what an operator attached",
                false,
            )
        })?;
    let result = DetachResult {
        slot_id: input.slot_id,
        detached,
    };
    if detached {
        journal_script(access, actor, SCRIPT_DETACHED_EVENT_KIND, &result)?;
    }
    serde_json::to_value(&result)
        .map_err(|error| ControlError::invalid_request(format!("detach result: {error}")))
}

/// Every compile problem, as data, on one refusal.
fn compile_error(errors: &[quantick_pine::PineError], source: &str) -> ControlError {
    let diagnostics = errors
        .iter()
        .map(|error| {
            let (line, column) = line_and_column(source, error.span.start);
            ScriptDiagnostic {
                code: error.code.as_str().to_owned(),
                start: u32::try_from(error.span.start).unwrap_or(u32::MAX),
                end: u32::try_from(error.span.end).unwrap_or(u32::MAX),
                line,
                column,
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

/// 1-based line and column of a byte offset, counting characters rather than
/// bytes in the column so a note over accented text points where it looks.
fn line_and_column(source: &str, offset: usize) -> (u32, u32) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, tail)| tail)
        .chars()
        .count()
        + 1;
    (
        u32::try_from(line).unwrap_or(u32::MAX),
        u32::try_from(column).unwrap_or(u32::MAX),
    )
}

fn journal_script<T: Serialize>(
    access: &mut ControlAccess,
    actor: &ActorContext,
    kind: &str,
    payload: &T,
) -> Result<(), ControlError> {
    let event_actor = EventActor {
        kind: actor.actor_kind,
        client_name: actor.client_name.clone(),
    };
    let payload = serde_json::to_value(payload)
        .map_err(|error| ControlError::invalid_request(format!("script event: {error}")))?;
    access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(SCRIPT_MODULE_ID).expect("static module ID is valid"),
            kind: EventKind::new(kind).expect("static event kind is valid"),
            actor: Some(event_actor),
            payload: json!({ "script": payload }),
        },
        metrics::wall_clock_ms(),
    );
    Ok(())
}
