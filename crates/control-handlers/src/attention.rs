//! The first action — a human mark: "look at this" turned into a referent a
//! client can quote back.
//!
//! Plan §2.2 and `arch-review`'s second operator: a capability a trader does
//! exists as a named call with an actor in its signature, not only inside a
//! click handler. The UI hotkey, the `QUANTICK_CONTROL_MARK` hook, a
//! deterministic test and an authorized agent all arrive at the same handler
//! through the action registry. The gateway keeps the action unavailable to
//! remote observer clients: its permissions are not in the observer ceiling,
//! so a remote invocation is refused before dispatch.
//!
//! The one thing the mark reads of the window is what is under the pointer,
//! through [`MarkPort`]; the resolved target, the note and the journal line
//! are decided here.

pub use quantick_control_schema::attention::*;

use std::collections::BTreeSet;

use quantick_control::{
    error::ControlError,
    id::{CapabilityId, CostClassId, ModuleId, RiskFlagId},
    limits::CONTROL_REASON_MAX_BYTES,
    registry::{
        Availability, CapabilityDescriptor, EffectPersistence, ExpectedCost, IdempotencyPolicy,
        RegistryError, RevisionPolicy,
    },
    schema::generated_schema,
    wire::{ActorContext, ActorKind, WireU64},
};
use quantick_control_host::journal::{EventActor, NewEvent};
use quantick_control_schema::interaction::CursorSnapshot;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::dock::ActionDock;

/// The annotate tier's identifiers this action docks into (contract §7).
pub use quantick_control_host::authority::{
    ANNOTATE_ATTENTION_PERMISSION_ID, ANNOTATE_EFFECT_ID, ANNOTATE_PERMISSION_ID,
    ANNOTATOR_PROFILE_ID, ATTENTION_MODULE_ID, CAPABILITY_VERSION, NO_CONFIRMATION_ID,
    UI_BOUNDED_COST_ID,
};

/// The window as a mark reads it: what is under the pointer now.
pub trait MarkPort {
    /// The fully resolved cursor target — pane, bar, price, flow cell,
    /// drawing — at this instant.
    fn cursor_snapshot(&self) -> CursorSnapshot;
}

/// What a mark needs of the control access it runs in.
pub trait MarkAccess {
    /// Journal one event, stamped by the access's own clock; answers its
    /// sequence.
    fn record_event(&mut self, event: NewEvent) -> WireU64;
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
pub struct MarkInput {
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
pub struct MarkCanonicalInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = CONTROL_REASON_MAX_BYTES))]
    pub note: Option<String>,
    pub target: CursorSnapshot,
    pub target_source: MarkTargetSource,
}

/// Who settled a mark's target.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MarkTargetSource {
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
pub struct MarkResult {
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

/// The mark's descriptor, as the registry publishes it.
#[must_use]
pub fn mark_descriptor() -> CapabilityDescriptor {
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
fn resolve_mark<P: MarkPort + ?Sized>(
    app: &P,
    _actor: &ActorContext,
    input: Value,
) -> Result<Value, ControlError> {
    let input: MarkInput = serde_json::from_value(input)
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let (target, target_source) = match input.target {
        Some(target) => (target, MarkTargetSource::Supplied),
        None => (app.cursor_snapshot(), MarkTargetSource::Pointer),
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
pub fn create_mark<P: ?Sized, A: MarkAccess>(
    _app: &mut P,
    access: &mut A,
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
    let event_actor = EventActor::from(actor);
    let payload = json!({
        "target": target,
        "target_source": target_source,
        "note": input.note,
        "actor": event_actor,
    });
    let sequence = access.record_event(NewEvent::by(
        ATTENTION_MODULE_ID,
        MARK_EVENT_KIND,
        actor,
        payload,
    ));
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

/// Dock the mark: resolved against the pointer at call time, recorded in
/// the control trace with its target settled.
pub fn register<D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    D: ActionDock<H, A>,
    H: MarkPort + ?Sized,
    A: MarkAccess,
{
    registry.register_resolved(
        mark_descriptor(),
        create_mark::<H, A>,
        resolve_mark::<H>,
        generated_schema::<MarkCanonicalInput>(),
    )
}

#[cfg(test)]
#[path = "attention_tests.rs"]
mod attention_tests;
