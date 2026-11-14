//! Immutable observer authority, capability, and request-dispatch contract.

use std::{collections::BTreeSet, fmt, sync::Arc};

use quantick_control::{
    error::{ControlError, codes},
    handshake::{CURRENT_PROTOCOL_VERSION, ProtocolLimits},
    id::{CapabilityId, InstanceId, PermissionId, ProfileId},
    registry::{ControlRegistry, PermissionDescriptor, RegistryError},
    wire::{ModuleRevision, RequestEnvelope, WireU64},
};
use quantick_control_host::{
    admission::TierPolicy,
    authority,
    contract::{AdmittedRoute, CapabilityContract, ReadPreparation, ScopeCatalogue},
};
use serde_json::Value;
// The scope-denied error that used it moved to `admission`; the tests below
// still build their payloads with it through `use super::*`.
#[cfg(test)]
use serde_json::json;

use crate::app::ControlWindow;

use super::{
    actions::ActionRegistry,
    events::EventsWaitInput,
    evidence::{EvidenceStore, RawScreenshot, SessionIdentity},
    journal::EventJournal,
    registry::ProjectionRegistry,
    types::known_error,
};

mod reads;

#[cfg(test)]
#[path = "contract/tests/preparation.rs"]
mod preparation;
// `EventsReadInvocation` is re-exported: the gateway completes a parked wait
// with it and reaches it by the path it always had.
pub(crate) use reads::EventsReadInvocation;

#[cfg(test)]
pub(crate) use quantick_control_host::authority::DESCRIBE_CAPABILITY_ID;
pub(crate) use quantick_control_host::authority::{
    ANALYST_PROFILE_ID, COCKPIT_PERMISSION_ID, COCKPIT_PROFILE_ID, DescribeResult, EmptyInput,
    OBSERVE_PERMISSION_ID, OBSERVER_PROFILE_ID, SnapshotReadInput,
};
#[cfg(test)]
pub(crate) use quantick_control_host::authority::{SNAPSHOT_CAPABILITY_ID, TRADER_PROFILE_ID};

pub(crate) use quantick_control_schema::chart::ChartWindowInput;

pub(crate) enum PreparedDispatch {
    Worker(Box<dyn PreparedWorkerRead>),
    Ui(Box<dyn PreparedUiRead>),
    /// `events.wait`: park on the gateway side until the journal moves past
    /// the resolved position or the timeout elapses, then run the bounded
    /// read through the UI queue.
    Parked(ParkedWait),
    /// A registered action: it runs on the application thread with mutable
    /// application state, through the very handler the trader's own gesture
    /// calls. The gateway attaches the connection's trusted actor; the
    /// payload never carries one.
    Action(PreparedAction),
}

/// An action that passed its permission check and its input schema, waiting
/// for the application thread.
#[derive(Clone, Debug)]
pub(crate) struct PreparedAction {
    pub capability_id: CapabilityId,
    pub capability_version: u32,
    pub input: Value,
}

/// A `wait_for_change` that has been validated and is about to park.
#[derive(Clone, Debug)]
pub(crate) struct ParkedWait {
    pub input: EventsWaitInput,
}

impl fmt::Debug for PreparedDispatch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Worker(_) => "PreparedDispatch::Worker(<registered>)",
            Self::Ui(_) => "PreparedDispatch::Ui(<registered>)",
            Self::Parked(_) => "PreparedDispatch::Parked(<events.wait>)",
            Self::Action(_) => "PreparedDispatch::Action(<registered>)",
        })
    }
}

#[derive(Debug)]
pub(crate) struct PreparedRequest {
    pub envelope: RequestEnvelope,
    pub required_permissions: BTreeSet<PermissionId>,
    pub dispatch: PreparedDispatch,
}

pub(crate) struct SerializedUiRead {
    pub capture_revision: Option<WireU64>,
    pub module_revisions: Vec<ModuleRevision>,
    pub result: Value,
}

/// Work a capture still owes after it has left the application thread.
///
/// The failure is a full `ControlError` and not a marker: everything after the
/// application thread — encoding, hashing, retention — can refuse for a reason
/// the client can act on, and an evidence bundle that does not fit its store
/// has to be able to say `control.backpressure` rather than "serialization
/// failed".
pub(crate) trait DeferredUiRead: Send {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError>;
}

pub(crate) type UiReadExecution = Box<dyn DeferredUiRead>;

/// An action's result, on its way off the application thread. It is already
/// a value; the wrapper only lets it travel the same channel a capture does.
pub(crate) struct DeferredActionResult(pub Value);

impl DeferredUiRead for DeferredActionResult {
    fn into_serialized(self: Box<Self>) -> Result<SerializedUiRead, ControlError> {
        Ok(SerializedUiRead {
            capture_revision: None,
            module_revisions: Vec::new(),
            result: self.0,
        })
    }
}

/// Everything one application-thread read may touch.
///
/// A struct rather than a parameter list because the set grows with the tier:
/// evidence needs the retained store, the connection's grant and the frame's
/// pixels on top of what a snapshot needs, and threading four more arguments
/// through every implementation would make each of them harder to read for the
/// benefit of one.
pub(crate) struct UiReadContext<'a> {
    pub projections: &'a mut ProjectionRegistry,
    pub journal: &'a EventJournal,
    pub app: &'a ControlWindow,
    pub instance_id: &'a InstanceId,
    pub session: &'a SessionIdentity,
    pub evidence: &'a EvidenceStore,
    /// The pixels of the frame just painted, when one was asked for and has
    /// arrived. Taken by the read that uses it, so a stale image can never be
    /// served to the next capture.
    pub screenshot: &'a mut Option<RawScreenshot>,
    pub window_id: Option<WireU64>,
}

pub(crate) trait PreparedUiRead: Send {
    /// `Some(None)` means the focused live window must be pinned at UI admission.
    fn window_target(&self) -> Option<Option<WireU64>> {
        None
    }

    /// Only reads of painted controls wait for a matching native paint.
    fn needs_paint(&self) -> bool {
        self.needs_screenshot()
    }

    fn execute(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError>;

    /// Whether this read needs the frame rasterised before it can run.
    ///
    /// The gateway asks before dispatching: a read that says yes and finds no
    /// image waits one frame while the window is asked for one, rather than
    /// answering without it.
    fn needs_screenshot(&self) -> bool {
        false
    }
}

pub(crate) trait PreparedWorkerRead: Send {
    fn execute(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_profile: &ProfileId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Result<Value, ControlError>;
}

impl PreparedDispatch {
    pub fn needs_paint(&self) -> bool {
        match self {
            Self::Ui(read) => read.needs_paint(),
            _ => false,
        }
    }
    pub fn window_target(&self) -> Option<Option<WireU64>> {
        match self {
            Self::Ui(read) => read.window_target(),
            _ => None,
        }
    }

    pub fn execute_worker(
        &self,
        contract: &ObserverContract,
        instance_id: &InstanceId,
        effective_profile: &ProfileId,
        effective_scopes: &BTreeSet<PermissionId>,
        effective_limits: &ProtocolLimits,
    ) -> Option<Result<Value, ControlError>> {
        match self {
            Self::Worker(invocation) => Some(invocation.execute(
                contract,
                instance_id,
                effective_profile,
                effective_scopes,
                effective_limits,
            )),
            Self::Ui(_) | Self::Parked(_) | Self::Action(_) => None,
        }
    }

    pub fn execute_ui(&self, context: UiReadContext<'_>) -> Result<UiReadExecution, ControlError> {
        match self {
            Self::Worker(_) | Self::Parked(_) => Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "a request that does not execute on the application thread entered the UI queue",
                false,
            )),
            Self::Action(_) => Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "an action reached the read path instead of the action path",
                false,
            )),
            Self::Ui(invocation) => invocation.execute(context),
        }
    }

    /// Whether this request needs the window rasterised first.
    pub fn needs_screenshot(&self) -> bool {
        match self {
            Self::Ui(invocation) => invocation.needs_screenshot(),
            Self::Worker(_) | Self::Parked(_) | Self::Action(_) => false,
        }
    }
}

pub(crate) struct PreparedCapability {
    dispatch: PreparedDispatch,
    dynamic_permissions: BTreeSet<PermissionId>,
}

pub(crate) type PrepareHandler =
    fn(ScopeCatalogue<'_>, &Value) -> Result<PreparedCapability, ControlError>;

pub(crate) struct ObserverContract {
    contract: CapabilityContract<PrepareHandler>,
    actions: Arc<ActionRegistry>,
    /// The retained evidence bundles of this instance.
    ///
    /// Held here because a worker read reaches the contract and nothing else:
    /// paging a bundle needs no application state, so it must not have to
    /// travel the application thread to find its own store. The handle is
    /// shared; the application thread keeps one too, and empties it when
    /// access is withdrawn.
    evidence: EvidenceStore,
}

impl ObserverContract {
    /// The contract this instance serves: the published authority, the
    /// snapshot modules `projections` registers, the read capabilities bound
    /// to this application's handlers, and the actions as external
    /// capabilities.
    pub fn new(
        projections: &ProjectionRegistry,
        actions: Arc<ActionRegistry>,
        evidence: EvidenceStore,
    ) -> Result<Self, RegistryError> {
        let mut registry = authority::builder()?;
        for descriptor in projections.module_descriptors() {
            registry.register_module(descriptor.clone())?;
        }
        let mut contract: CapabilityContract<PrepareHandler> = registry.build(projections)?;
        for (descriptor, handler) in reads::bindings() {
            contract.register_read(descriptor, handler)?;
        }
        // Actions bind explicitly; missing read handlers never become actions.
        for descriptor in actions.descriptors() {
            contract.register_external(descriptor.clone())?;
        }
        Ok(Self {
            contract,
            actions,
            evidence,
        })
    }

    /// The host contract this one composes: the registry, the profiles, the
    /// permissions and the snapshot catalogue, for a generator or a check
    /// that reads them without the handlers.
    pub fn capabilities(&self) -> &CapabilityContract<PrepareHandler> {
        &self.contract
    }

    pub fn registry(&self) -> &ControlRegistry {
        self.contract.registry()
    }

    pub fn validate_output(
        &self,
        capability_id: &CapabilityId,
        capability_version: u32,
        result: &Value,
    ) -> bool {
        self.contract
            .validate_output(capability_id, capability_version, result, |id, version| {
                self.actions.schemas(id, version)
            })
    }

    pub fn default_grant(&self) -> BTreeSet<PermissionId> {
        authority::default_grant()
    }

    /// Every registered snapshot scope this grant already reaches, sorted by
    /// scope ID and capped at what one capture may carry.
    ///
    /// Derived from the registry, never a hand-kept list: a module that
    /// registers a scope tomorrow is in a bundle tomorrow, without an edit
    /// here or in whatever asked.
    #[cfg(any(feature = "control-harness", test))]
    pub fn readable_scopes(
        &self,
        grant: &BTreeSet<PermissionId>,
    ) -> Vec<quantick_control::id::SnapshotScopeId> {
        self.contract.readable_scopes(grant)
    }

    pub fn selectable_permissions(&self) -> impl Iterator<Item = &PermissionDescriptor> {
        self.contract
            .permissions()
            .iter()
            .filter(|descriptor| descriptor.id.as_str() != OBSERVE_PERMISSION_ID)
    }

    pub fn describe(
        &self,
        instance_id: InstanceId,
        effective_profile: ProfileId,
        effective_scopes: BTreeSet<PermissionId>,
        effective_limits: ProtocolLimits,
    ) -> DescribeResult {
        DescribeResult {
            instance_id,
            application_version: super::system::BUILD.application_version.to_owned(),
            application_commit: super::system::BUILD
                .git_commit
                .unwrap_or("unknown")
                .to_owned(),
            protocol_version: CURRENT_PROTOCOL_VERSION,
            effective_profile,
            effective_scopes,
            effective_limits,
            modules: self.contract.registry().modules().cloned().collect(),
            profiles: self.contract.profiles().to_vec(),
            permissions: self.contract.permissions().to_vec(),
            capabilities: self.contract.registry().capabilities().cloned().collect(),
            snapshot_scopes: self.contract.snapshot_scopes().to_vec(),
        }
    }

    pub fn prepare(
        &self,
        envelope: RequestEnvelope,
        effective_scopes: &BTreeSet<PermissionId>,
    ) -> Result<PreparedRequest, ControlError> {
        let admitted = self.contract.admit(
            envelope,
            effective_scopes,
            TierPolicy::STRICT,
            |id, version| self.actions.schemas(id, version),
            |handler, payload, scopes| {
                let prepared = handler(scopes, payload)?;
                Ok(ReadPreparation {
                    prepared: prepared.dispatch,
                    dynamic_permissions: prepared.dynamic_permissions,
                })
            },
        )?;
        let dispatch = match admitted.route {
            AdmittedRoute::Read(dispatch) => dispatch,
            AdmittedRoute::External => PreparedDispatch::Action(PreparedAction {
                capability_id: admitted.envelope.capability_id.clone(),
                capability_version: admitted.envelope.capability_version,
                input: admitted.envelope.payload.clone(),
            }),
        };
        Ok(PreparedRequest {
            envelope: admitted.envelope,
            required_permissions: admitted.required_permissions,
            dispatch,
        })
    }
}

#[cfg(test)]
use quantick_control_host::authority::{module, permission, profile};

#[cfg(test)]
mod tests;
