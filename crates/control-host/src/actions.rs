//! The action registry: descriptors for discovery, handlers for execution,
//! schemas for both sides of every call — generic over the host state `H`
//! a handler mutates and the access `A` it journals through, so the window
//! binds its own and a second host binds another.

use std::{collections::BTreeMap, sync::Arc};

use quantick_control::{
    error::ControlError,
    id::CapabilityId,
    registry::{CapabilityDescriptor, RegistryError},
    schema::CompiledSchema,
    wire::ActorContext,
};
use serde_json::Value;

use crate::contract::ExternalSchemas;

/// One action's handler. It receives the application and the control access
/// it lives in (the journal, the trace), the trusted actor, and the resolved
/// input; it returns the structured result the registry's output schema
/// describes.
pub type ActionHandler<H, A> =
    fn(&mut H, &mut A, &ActorContext, &Value) -> Result<Value, ControlError>;

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
pub type ActionResolver<H> = fn(&H, &ActorContext, Value) -> Result<Value, ControlError>;

/// The resolver of an action whose input is already exactly what it will do.
pub fn identity_resolution<H>(
    _host: &H,
    _actor: &ActorContext,
    input: Value,
) -> Result<Value, ControlError> {
    Ok(input)
}

/// One docked action: what `describe` publishes, what runs, and the three
/// schemas that bound it — the caller's input, the resolved input the trace
/// records and a replay feeds back, and the result.
pub struct RegisteredAction<H, A> {
    pub descriptor: CapabilityDescriptor,
    pub handler: ActionHandler<H, A>,
    pub resolve: ActionResolver<H>,
    pub input: CompiledSchema,
    pub canonical: CompiledSchema,
    pub output: CompiledSchema,
}

/// The registry: descriptors for discovery, handlers for execution, schemas
/// for both sides of every call.
pub struct ActionRegistry<H, A> {
    actions: BTreeMap<(CapabilityId, u32), Arc<RegisteredAction<H, A>>>,
}

impl<H, A> Default for ActionRegistry<H, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H, A> ActionRegistry<H, A> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            actions: BTreeMap::new(),
        }
    }

    /// Dock one action whose input is already what it will do.
    pub fn register(
        &mut self,
        descriptor: CapabilityDescriptor,
        handler: ActionHandler<H, A>,
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
        handler: ActionHandler<H, A>,
        resolve: ActionResolver<H>,
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

    /// Borrow the actual registered identity and validators without cloning
    /// an execution handle or exposing handlers and canonical-input machinery.
    pub fn schemas(&self, id: &CapabilityId, version: u32) -> Option<ExternalSchemas<'_>> {
        let action = self.actions.get(&(id.clone(), version))?;
        Some(ExternalSchemas {
            capability_id: &action.descriptor.id,
            version: action.descriptor.version,
            input: &action.input,
            output: &action.output,
        })
    }

    /// One registered action, owned: the handler needs the access,
    /// so the registry cannot stay borrowed across the call.
    pub fn lookup(&self, capability_id: &str, version: u32) -> Option<Arc<RegisteredAction<H, A>>> {
        let id = CapabilityId::new(capability_id).ok()?;
        self.actions.get(&(id, version)).map(Arc::clone)
    }
}
