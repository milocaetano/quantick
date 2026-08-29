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

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

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

pub(crate) const CAPABILITY_VERSION: u32 = 1;
/// The version of `attention.mark.create` the hotkey and the hook invoke.
pub(crate) const MARK_CAPABILITY_VERSION: u32 = CAPABILITY_VERSION;
pub(crate) const NO_CONFIRMATION_ID: &str = "none";
pub(crate) const UI_BOUNDED_COST_ID: &str = "ui_bounded";

/// One action's handler. It receives the application and the control access
/// it lives in (the journal, the trace), the trusted actor, and the resolved
/// input; it returns the structured result the registry's output schema
/// describes.
pub(crate) type ActionHandler =
    fn(&mut QuantickApp, &mut ControlAccess, &ActorContext, &Value) -> Result<Value, ControlError>;

/// The step that turns what a caller wrote into what actually happened, before
/// anything happens.
///
/// An action that reads live state at call time — the mark takes whatever is
/// under the pointer when no target is given — leaves an intent the control
/// trace cannot reproduce: replay a "mark here" with no *here* and the rerun
/// resolves a pointer that was somewhere else, or nowhere. Resolving first and
/// recording the resolved input makes the trace line say what was done rather
/// than what was asked (contract §11), and an action with nothing to resolve
/// uses [`identity_resolution`] and pays nothing.
pub(crate) type ActionResolver =
    fn(&QuantickApp, &ActorContext, Value) -> Result<Value, ControlError>;

/// The resolver of an action whose input is already exactly what it will do.
fn identity_resolution(
    _app: &QuantickApp,
    _actor: &ActorContext,
    input: Value,
) -> Result<Value, ControlError> {
    Ok(input)
}

/// One docked action: what `describe` publishes, what runs, and the three
/// schemas that bound it — the caller's input, the resolved input the trace
/// records and a replay feeds back, and the result.
pub(crate) struct RegisteredAction {
    pub descriptor: CapabilityDescriptor,
    pub handler: ActionHandler,
    pub resolve: ActionResolver,
    pub input: CompiledSchema,
    pub canonical: CompiledSchema,
    pub output: CompiledSchema,
}

/// The registry: descriptors for discovery, handlers for execution, schemas
/// for both sides of every call.
pub(crate) struct ActionRegistry {
    actions: BTreeMap<(CapabilityId, u32), Arc<RegisteredAction>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            actions: BTreeMap::new(),
        }
    }

    /// Dock one action whose input is already what it will do.
    pub fn register(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler,
    ) -> Result<(), RegistryError> {
        let canonical = descriptor.input_schema.clone();
        self.register_resolved(descriptor, handler, identity_resolution, canonical)
    }

    /// Dock one action that resolves live state before it acts. The descriptor
    /// is what `describe` and search report; its schemas are compiled once
    /// here and reused for every invocation. `canonical_schema` describes the
    /// resolved input — what the control trace records and what a replay
    /// hands back — and is not published to clients: a caller writes the
    /// descriptor's `input_schema` and the resolver produces this.
    pub fn register_resolved(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler,
        resolve: ActionResolver,
        canonical_schema: Value,
    ) -> Result<(), RegistryError> {
        let key = (descriptor.id.clone(), descriptor.version);
        if self.actions.contains_key(&key) {
            return Err(RegistryError::Duplicate {
                kind: "capability",
                id: descriptor.id.to_string(),
            });
        }
        let compile = |schema: &Value, half: &str| {
            CompiledSchema::new(schema).map_err(|error| {
                RegistryError::InvalidDescriptor(format!(
                    "action `{}` {half} schema is invalid: {error}",
                    descriptor.id
                ))
            })
        };
        let input = compile(&descriptor.input_schema, "input")?;
        let canonical = compile(&canonical_schema, "canonical input")?;
        let output = compile(&descriptor.output_schema, "output")?;
        self.actions.insert(
            key,
            Arc::new(RegisteredAction {
                descriptor,
                handler,
                resolve,
                input,
                canonical,
                output,
            }),
        );
        Ok(())
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &CapabilityDescriptor> {
        self.actions.values().map(|action| &action.descriptor)
    }

    /// One registered action, owned: the handler needs `&mut ControlAccess`,
    /// so the registry cannot stay borrowed across the call.
    pub fn lookup(&self, capability_id: &str, version: u32) -> Option<Arc<RegisteredAction>> {
        let id = CapabilityId::new(capability_id).ok()?;
        self.actions.get(&(id, version)).map(Arc::clone)
    }
}

/// The actions every instance registers. A later module adds one descriptor
/// and one handler here; nothing else opens.
pub(crate) fn standard_actions() -> Result<ActionRegistry, RegistryError> {
    let mut registry = ActionRegistry::new();
    registry.register_resolved(
        mark_descriptor(),
        create_mark,
        resolve_mark,
        generated_schema::<MarkCanonicalInput>(),
    )?;
    super::annotate::register(&mut registry)?;
    super::notify::register(&mut registry)?;
    super::layout::register(&mut registry)?;
    super::recovery::register(&mut registry)?;
    super::script::register(&mut registry)?;
    super::trade::register(&mut registry)?;
    Ok(registry)
}

/// What a mark takes: an optional note the human typed, and optionally the
/// target itself. The hotkey resolves the pointer and supplies the target, so
/// the input alone determines the mark and a control trace can replay it
/// identically; a caller that supplies none gets what is under the pointer
/// at that moment — and a replayed entry without one is refused rather than
/// resolved against the rerun's pointer, which is not the one that was
/// marked.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MarkInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = CONTROL_REASON_MAX_BYTES))]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<CursorSnapshot>,
}

/// A mark after resolution: the target is settled, and the input says how it
/// was settled. This is what the control trace records and what a replay
/// hands back, so a rerun marks the bar that was marked rather than whatever
/// the pointer happens to be over.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MarkCanonicalInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = CONTROL_REASON_MAX_BYTES))]
    pub note: Option<String>,
    pub target: CursorSnapshot,
    pub target_source: MarkTargetSource,
}

/// Who settled a mark's target.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MarkTargetSource {
    /// The pointer of this window, read when the mark was taken.
    Pointer,
    /// The caller passed the target it had read from a snapshot.
    Supplied,
    /// A control trace re-injected a recorded mark.
    Replayed,
}

impl MarkTargetSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pointer => "pointer",
            Self::Supplied => "supplied",
            Self::Replayed => "replayed",
        }
    }
}

/// What a mark returns: where it landed in the journal and exactly what was
/// pointed at when it was taken. The journal event at `sequence` carries the
/// wall-clock time; the result does not repeat it, so the trace's result
/// digest depends on what was marked and on its place in the journal, never
/// on the clock.
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

/// A mark's resolver: what is under the pointer *now* becomes part of the
/// input, so the trace line and a replay of it name the same bar.
fn resolve_mark(
    app: &QuantickApp,
    _actor: &ActorContext,
    input: Value,
) -> Result<Value, ControlError> {
    let input: MarkInput = serde_json::from_value(input)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let (target, target_source) = match input.target {
        Some(target) => (target, MarkTargetSource::Supplied),
        None => (cursor_snapshot(app), MarkTargetSource::Pointer),
    };
    serde_json::to_value(MarkCanonicalInput {
        note: input.note,
        target,
        target_source,
    })
    .map_err(|error| ControlError::invalid_request(format!("mark input: {error}")))
}

/// The mark handler: append the event for the resolved target. One path for
/// the hotkey, the hook, the tests, a replayed trace entry and any authorized
/// agent.
fn create_mark(
    _app: &mut QuantickApp,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: MarkCanonicalInput = serde_json::from_value(input.clone())
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
    let target = input.target;
    // A rerun says how *this* mark arrived: from the trace, not from a
    // pointer that was somewhere else. What the original run resolved stays
    // in the trace line.
    let target_source = if actor.actor_kind == ActorKind::Automation {
        MarkTargetSource::Replayed
    } else {
        input.target_source
    }
    .as_str();
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
        let action = registry.lookup(MARK_CAPABILITY_ID, 1).unwrap();
        let (descriptor, input, output) = (&action.descriptor, &action.input, &action.output);
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
