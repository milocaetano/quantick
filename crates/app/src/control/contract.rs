//! Native window capture and clock adapters for the headless observer contract.
use super::{
    evidence::{EvidenceStore, RawScreenshot, SessionIdentity},
    journal::EventJournal,
    registry::ProjectionRegistry,
};
use crate::app::ControlWindow;
use quantick_control::{
    error::ControlError,
    id::{InstanceId, PermissionId, SnapshotScopeId},
    wire::WireU64,
};
use quantick_control_handlers::observer::UiReadPort;
pub(crate) use quantick_control_handlers::observer::{
    ANALYST_PROFILE_ID, COCKPIT_PERMISSION_ID, COCKPIT_PROFILE_ID, DeferredActionResult,
    DescribeResult, OBSERVE_PERMISSION_ID, OBSERVER_PROFILE_ID, ParkedWait, PreparedDispatch,
    PreparedRequest, UiReadExecution,
};
use quantick_control_host::{
    events::{EventsReadInput, read_page},
    journal::EventPage,
    projection::SnapshotCapture,
};
use quantick_control_schema::{
    chart::{ChartWindowInput, ChartWindowPage},
    evidence::{EvidenceCapture, EvidenceCaptureInput},
};
use std::collections::BTreeSet;
pub(crate) type ObserverContract = quantick_control_handlers::observer::ObserverContract<
    ControlWindow,
    super::gateway::ControlAccess,
>;

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

impl UiReadPort for UiReadContext<'_> {
    fn snapshot(&mut self, scopes: &[SnapshotScopeId]) -> Result<SnapshotCapture, ControlError> {
        self.projections.capture(self.app, self.instance_id, scopes)
    }
    fn window_id(&self) -> Option<WireU64> {
        self.window_id
    }
    fn chart(
        &self,
        input: &ChartWindowInput,
        canonical_query: &serde_json::Value,
    ) -> Result<ChartWindowPage, ControlError> {
        super::chart::chart_window_prevalidated(
            self.app,
            self.instance_id,
            &input.query,
            canonical_query,
            input.cursor.as_ref(),
            crate::metrics::wall_clock_ms(),
        )
    }
    fn events(&self, input: &EventsReadInput, timed_out: bool) -> Result<EventPage, ControlError> {
        read_page(
            self.journal,
            self.instance_id,
            input.cursor.as_ref(),
            input.start,
            input.limit,
            timed_out,
        )
    }
    fn evidence(
        &mut self,
        input: &EvidenceCaptureInput,
        source_scopes: BTreeSet<PermissionId>,
    ) -> Result<EvidenceCapture, ControlError> {
        super::evidence::capture_prevalidated(self, input, source_scopes)
    }
}

pub(crate) use quantick_control_handlers::observer::EventsReadInvocation;
#[cfg(test)]
#[path = "contract/tests/preparation.rs"]
mod preparation;

#[cfg(test)]
use quantick_control::{
    error::codes,
    handshake::{CURRENT_PROTOCOL_VERSION, ProtocolLimits},
    id::CapabilityId,
    wire::{ModuleRevision, RequestEnvelope},
};
#[cfg(test)]
use quantick_control_host::authority::{
    self, EmptyInput, SNAPSHOT_CAPABILITY_ID, module, permission, profile,
};
#[cfg(test)]
use serde_json::{Value, json};
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
mod tests;
