//! One typed adapter for every call whose input and answer are owner types.
//!
//! A call is an owner type — the input its owner already decodes — plus one
//! `impl Call` here: what the call publishes ([`CallSpec`]) and what it runs.
//! Calls that share one input are zero-sized markers naming it as
//! [`Call::Input`], so the shared input publishes one schema.
//! [`dock_call`] builds the descriptor from the two types' own schemas, so
//! the published contract cannot drift from what is decoded, and docks one
//! erasure that parses, runs, journals and encodes. The impls live in this
//! crate, beside the family's other handlers, so an owner gains no dependency
//! on the control plane.

use quantick_control::{
    error::ControlError,
    id::{CapabilityId, ModuleId, PermissionId},
    registry::{CapabilityDescriptor, RegistryError},
    schema::generated_schema,
    wire::ActorContext,
};
use quantick_control_host::{actions::ReplayTrace, journal::NewEvent};
use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::JournalAccess;
use crate::dock::ActionDock;
use crate::invalid;

/// What a call publishes besides its two schemas.
#[derive(Clone, Copy, Debug)]
pub struct CallSpec {
    pub id: &'static str,
    pub module: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub permissions: &'static [&'static str],
    /// The journal kind a successful call appends under; `None` journals
    /// nothing.
    pub event: Option<&'static str>,
    pub replay: ReplayTrace,
    /// The family's effect, idempotency, cost and risk posture: every
    /// descriptor field the spec and the two schemas do not state. The
    /// adapter sets those over whatever this returns.
    pub posture: fn(&CallSpec) -> CapabilityDescriptor,
}

/// What a running call may reach besides its host: who asked and the
/// journal.
pub struct CallCx<'a, A> {
    pub actor: &'a ActorContext,
    pub access: &'a mut A,
}

/// One call over host `H`.
pub trait Call<H: ?Sized> {
    /// The owner type the call decodes: `Self`, or the input a marker shares.
    type Input: DeserializeOwned + JsonSchema;
    type Answer: Serialize + JsonSchema;

    const SPEC: CallSpec;

    /// Run the call on `host`.
    ///
    /// # Errors
    ///
    /// The refusal, in words a caller can correct.
    fn run<A: JournalAccess>(
        input: Self::Input,
        host: &mut H,
        cx: &mut CallCx<'_, A>,
    ) -> Result<Self::Answer, ControlError>;

    /// The journal payload for a successful call: the encoded answer, unless
    /// the family records more.
    fn entry(_asked: Value, answer: &Value) -> Value {
        answer.clone()
    }
}

/// The descriptor `C` publishes.
#[must_use]
pub fn descriptor<C: Call<H>, H: ?Sized>() -> CapabilityDescriptor {
    let spec = C::SPEC;
    CapabilityDescriptor {
        id: CapabilityId::new(spec.id).expect("static capability ID is valid"),
        module: ModuleId::new(spec.module).expect("static module ID is valid"),
        title: spec.title.to_owned(),
        description: spec.description.to_owned(),
        required_permissions: spec
            .permissions
            .iter()
            .map(|id| PermissionId::new(*id).expect("static permission ID is valid"))
            .collect(),
        input_schema: generated_schema::<C::Input>(),
        output_schema: generated_schema::<C::Answer>(),
        ..(spec.posture)(&spec)
    }
}

/// Dock `C` into `registry`.
///
/// # Errors
///
/// The capability is already docked.
pub fn dock_call<C, D, H, A>(registry: &mut D) -> Result<(), RegistryError>
where
    C: Call<H>,
    D: ActionDock<H, A>,
    H: ?Sized,
    A: JournalAccess,
{
    let descriptor = descriptor::<C, H>();
    match C::SPEC.replay {
        ReplayTrace::Recorded => registry.register(descriptor, erased::<C, H, A>),
        ReplayTrace::Excluded => registry.register_outside_replay(descriptor, erased::<C, H, A>),
    }
}

/// The handler [`dock_call`] docks for `C`, for a test that drives one call.
#[cfg(test)]
pub(crate) fn handler<C: Call<H>, H: ?Sized, A: JournalAccess>()
-> quantick_control_host::actions::ActionHandler<H, A> {
    erased::<C, H, A>
}

fn erased<C: Call<H>, H: ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let asked: C::Input = serde_json::from_value(input.clone()).map_err(invalid)?;
    let mut cx = CallCx { actor, access };
    let answer = C::run(asked, host, &mut cx)?;
    let answer = serde_json::to_value(answer).map_err(invalid)?;
    if let Some(kind) = C::SPEC.event {
        let entry = C::entry(input.clone(), &answer);
        access.record_event(NewEvent::by(C::SPEC.module, kind, actor, entry));
    }
    Ok(answer)
}

#[cfg(test)]
#[path = "call_tests.rs"]
mod call_tests;
