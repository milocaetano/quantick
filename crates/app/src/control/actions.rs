//! The action registry port: named, attributed calls that change session
//! state, and the first one — a human mark.
//!
//! Plan §2.2 and `arch-review`'s second operator: a capability a trader does
//! exists as a named call with an actor in its signature, not only inside a
//! click handler. The UI hotkey, the `QUANTICK_CONTROL_MARK` hook, a
//! deterministic test and — in a later pull request — an authorized agent all
//! arrive at the same handler through [`ActionRegistry`]. The gateway keeps
//! every action here unavailable to remote observer clients: an action's
//! permissions are not in the observer ceiling, so a remote invocation is
//! refused before dispatch.

use std::collections::{BTreeMap, BTreeSet};

use quantick_control::{
    error::ControlError,
    id::{CapabilityId, CostClassId, EventKind, ModuleId, RiskFlagId},
    limits::CONTROL_REASON_MAX_BYTES,
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::{CompiledSchema, generated_schema},
    wire::{ActorContext, ActorKind, WireU64},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{app::QuantickApp, metrics};

use super::{
    gateway::ControlAccess,
    interaction::{CursorSnapshot, cursor_snapshot},
    journal::{EventActor, NewEvent},
};

/// The first registered action: a human (or, later, an agent) points at what
/// is under the pointer and says "this".
pub(crate) const MARK_CAPABILITY_ID: &str = "attention.mark.create";
pub(crate) const MARK_EVENT_KIND: &str = "attention.mark.created";
pub(crate) const ATTENTION_MODULE_ID: &str = "attention";

/// The annotate tier's identifiers this action docks into (contract §7).
pub(crate) const ANNOTATE_PERMISSION_ID: &str = "annotate";
pub(crate) const ANNOTATE_ATTENTION_PERMISSION_ID: &str = "annotate.attention";
pub(crate) const ANNOTATE_EFFECT_ID: &str = "annotate";
pub(crate) const ANNOTATOR_PROFILE_ID: &str = "annotator";

const CAPABILITY_VERSION: u32 = 1;
const NO_CONFIRMATION_ID: &str = "none";
const UI_BOUNDED_COST_ID: &str = "ui_bounded";

/// One action's handler. It receives the application and the control access
/// it lives in (the journal, the trace), the trusted actor, and the validated
/// input; it returns the structured result the registry's output schema
/// describes.
pub(crate) type ActionHandler =
    fn(&mut QuantickApp, &mut ControlAccess, &ActorContext, &Value) -> Result<Value, ControlError>;

struct RegisteredAction {
    descriptor: CapabilityDescriptor,
    handler: ActionHandler,
    input: CompiledSchema,
    output: CompiledSchema,
}

/// The registry: descriptors for discovery, handlers for execution, schemas
/// for both sides of every call.
pub(crate) struct ActionRegistry {
    actions: BTreeMap<(CapabilityId, u32), RegisteredAction>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            actions: BTreeMap::new(),
        }
    }

    /// Dock one action. The descriptor is what `describe` and search report;
    /// its schemas are compiled once here and reused for every invocation.
    pub fn register(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler,
    ) -> Result<(), RegistryError> {
        let key = (descriptor.id.clone(), descriptor.version);
        if self.actions.contains_key(&key) {
            return Err(RegistryError::Duplicate {
                kind: "capability",
                id: descriptor.id.to_string(),
            });
        }
        let input = CompiledSchema::new(&descriptor.input_schema).map_err(|error| {
            RegistryError::InvalidDescriptor(format!(
                "action `{}` input schema is invalid: {error}",
                descriptor.id
            ))
        })?;
        let output = CompiledSchema::new(&descriptor.output_schema).map_err(|error| {
            RegistryError::InvalidDescriptor(format!(
                "action `{}` output schema is invalid: {error}",
                descriptor.id
            ))
        })?;
        self.actions.insert(
            key,
            RegisteredAction {
                descriptor,
                handler,
                input,
                output,
            },
        );
        Ok(())
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &CapabilityDescriptor> {
        self.actions.values().map(|action| &action.descriptor)
    }

    /// The handler for one registered action, with the schemas to validate
    /// its input before and its output after.
    pub fn lookup(
        &self,
        capability_id: &str,
        version: u32,
    ) -> Option<(
        &CapabilityDescriptor,
        ActionHandler,
        &CompiledSchema,
        &CompiledSchema,
    )> {
        let id = CapabilityId::new(capability_id).ok()?;
        self.actions.get(&(id, version)).map(|action| {
            (
                &action.descriptor,
                action.handler,
                &action.input,
                &action.output,
            )
        })
    }
}

/// The actions every instance registers. A later module adds one descriptor
/// and one handler here; nothing else opens.
pub(crate) fn standard_actions() -> Result<ActionRegistry, RegistryError> {
    let mut registry = ActionRegistry::new();
    registry.register(mark_descriptor(), create_mark)?;
    Ok(registry)
}

/// What a mark takes: an optional note the human typed, and optionally the
/// target itself. The hotkey resolves the pointer and supplies the target, so
/// the input alone determines the mark and a control trace can replay it
/// identically; a caller that supplies none gets what is under the pointer
/// at that moment.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MarkInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = CONTROL_REASON_MAX_BYTES))]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<CursorSnapshot>,
}

/// What a mark returns: where it landed in the journal and exactly what was
/// pointed at when it was taken. The journal event at `sequence` carries the
/// wall-clock time; the result does not repeat it, so the trace's result
/// digest depends on what was marked, not on when.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct MarkResult {
    pub sequence: WireU64,
    pub target: CursorSnapshot,
    /// Who resolved the target: `pointer` — the human's pointer, at the
    /// gesture (the hotkey and the hook pass it) or at the call (a caller
    /// that passed none); `supplied` — an agent passed a target it read from
    /// a snapshot; `replayed` — a control trace re-injected a recorded mark.
    pub target_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub actor: EventActor,
}

fn mark_descriptor() -> CapabilityDescriptor {
    CapabilityDescriptor {
        id: CapabilityId::new(MARK_CAPABILITY_ID).expect("static capability ID is valid"),
        version: CAPABILITY_VERSION,
        title: "Mark what is under the pointer".to_owned(),
        description: "Appends a journal event carrying the fully resolved cursor target — pane, bar, price, flow cell, drawing — plus an optional note, attributed to its actor. The primitive that turns \"look at this\" into a referent a client can quote back.".to_owned(),
        module: ModuleId::new(ATTENTION_MODULE_ID).expect("static module ID is valid"),
        input_schema: generated_schema::<MarkInput>(),
        output_schema: generated_schema::<MarkResult>(),
        examples: Vec::new(),
        effect: quantick_control::id::EffectId::new(ANNOTATE_EFFECT_ID)
            .expect("static effect ID is valid"),
        risk_flags: BTreeSet::<RiskFlagId>::new(),
        read_only: false,
        idempotency: IdempotencyPolicy::Forbidden,
        // A mark adds to an append-only journal; stale input cannot damage
        // existing state, which is why no expected revision is demanded.
        revision_policy: RevisionPolicy::OptionalForAdditive,
        stale_input_safety: Some(
            "A mark appends one journal event and overwrites nothing; a stale caller can only mark the wrong thing, which the event's resolved target makes visible."
                .to_owned(),
        ),
        dry_run_supported: false,
        persistence: EffectPersistence::Transient,
        reversible: false,
        destructive: false,
        risk_reducing: false,
        required_permissions: [ANNOTATE_PERMISSION_ID, ANNOTATE_ATTENTION_PERMISSION_ID]
            .into_iter()
            .map(|id| quantick_control::id::PermissionId::new(id).expect("static permission"))
            .collect(),
        preconditions: Vec::new(),
        confirmation_class: quantick_control::id::ConfirmationClassId::new(NO_CONFIRMATION_ID)
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

/// The mark handler: resolve the pointer the same way the cursor scope does,
/// then append the event. One path for the hotkey, the hook, the tests and
/// any authorized agent.
fn create_mark(
    app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: MarkInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    if input
        .note
        .as_ref()
        .is_some_and(|note| note.len() > CONTROL_REASON_MAX_BYTES)
    {
        return Err(ControlError::invalid_request(format!(
            "a mark note is at most {CONTROL_REASON_MAX_BYTES} bytes"
        )));
    }
    let (target, target_source) = match (actor.actor_kind, input.target) {
        (ActorKind::Automation, Some(target)) => (target, "replayed"),
        (ActorKind::Agent, Some(target)) => (target, "supplied"),
        (ActorKind::HumanUi, Some(target)) => (target, "pointer"),
        (_, None) => (cursor_snapshot(app), "pointer"),
    };
    let event_actor = EventActor {
        kind: actor.actor_kind,
        client_name: actor.client_name.clone(),
    };
    let recorded_at_unix_ms = metrics::wall_clock_ms();
    let payload = json!({
        "target": target,
        "target_source": target_source,
        "note": input.note,
        "actor": event_actor,
    });
    let sequence = access.journal_mut().record(
        NewEvent {
            module_id: ModuleId::new(ATTENTION_MODULE_ID).expect("static module ID is valid"),
            kind: EventKind::new(MARK_EVENT_KIND).expect("static event kind is valid"),
            actor: Some(event_actor.clone()),
            payload,
        },
        recorded_at_unix_ms,
    );
    let result = MarkResult {
        sequence,
        target,
        target_source: target_source.to_owned(),
        note: input.note,
        actor: event_actor,
    };
    serde_json::to_value(result)
        .map_err(|error| ControlError::invalid_request(format!("mark result: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_actions_register_and_validate_their_schemas() {
        let registry = standard_actions().unwrap();
        let (descriptor, _, input, output) = registry.lookup(MARK_CAPABILITY_ID, 1).unwrap();
        assert_eq!(descriptor.effect.as_str(), ANNOTATE_EFFECT_ID);
        assert!(!descriptor.read_only);
        assert!(
            descriptor
                .required_permissions
                .iter()
                .any(|permission| permission.as_str() == ANNOTATE_ATTENTION_PERMISSION_ID)
        );
        input.validate(&json!({})).unwrap();
        input
            .validate(&json!({ "note": "this absorption" }))
            .unwrap();
        assert!(input.validate(&json!({ "note": 1 })).is_err());
        assert!(input.validate(&json!({ "unexpected": true })).is_err());
        assert!(
            output.validate(&json!({})).is_err(),
            "a result needs its fields"
        );
        assert!(registry.lookup("attention.mark.create", 2).is_none());
        assert!(registry.lookup("not an id", 1).is_none());
    }

    #[test]
    fn a_second_registration_of_the_same_action_is_refused() {
        let mut registry = standard_actions().unwrap();
        assert!(matches!(
            registry.register(mark_descriptor(), create_mark),
            Err(RegistryError::Duplicate { .. })
        ));
    }
}
