//! The in-process door: invoking a registered action or read from inside the
//! application -- the hotkey, a launch hook, a test, a replayed trace entry.
//!
//! The same door a remote client comes through, minus the socket: the same
//! registered handler, the same schemas, the same `ObserverContract::prepare`
//! for a read. What this file adds is what only the inside knows -- the
//! trusted actor the window vouches for, and the control trace a replaying
//! tab records the intent and the result to, before and after the handler
//! runs. It decides nothing about permission: an action's grant is the
//! caller's, and a read's is the scopes the trader configured.
//!
//! Rate class: per request. An action is a human gesture; opening the trace
//! sidecar is rare and off the frame path.

use quantick_control::{
    error::{ControlError, codes},
    handshake::{CURRENT_PROTOCOL_VERSION, ProtocolLimits},
    id::RequestId,
    wire::{RequestEnvelope, WireU64},
};
use serde_json::Value;

use crate::app::QuantickApp;

use super::super::{
    contract::{PreparedDispatch, UiReadContext},
    trace::{ControlTrace, NoTrace, ReplayTraceFile, TRACE_VERSION, TraceEntry, result_digest},
    types::known_error,
};
use super::{ActionOrigin, ControlAccess};

impl ControlAccess {
    /// Invoke one registered action from inside the application — the hotkey,
    /// the `QUANTICK_CONTROL_MARK` hook, a test, or a replayed trace entry.
    /// Validates the input and the result against the action's schemas, and
    /// during a replay appends the intent and the result to the session's
    /// control trace before and after the handler runs (contract §11).
    pub(crate) fn invoke_local_action(
        &mut self,
        app: &mut QuantickApp,
        capability_id: &str,
        capability_version: u32,
        input: Value,
        origin: ActionOrigin,
    ) -> Result<Value, ControlError> {
        let actor_kind = origin.actor_kind();
        if self.identity.is_none() {
            return Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "local actions need a process identity, which failed to generate",
                false,
            ));
        }
        let action = self
            .actions
            .lookup(capability_id, capability_version)
            .ok_or_else(|| {
                known_error(
                    codes::CAPABILITY_UNKNOWN,
                    "capability ID or version is not a registered action",
                    false,
                )
            })?;
        let descriptor = &action.descriptor;
        let actor = match &origin {
            // A remote caller's actor is what the connection proved at the
            // handshake, so an agent cannot sign an action as the trader.
            ActionOrigin::Remote(actor) => (**actor).clone(),
            ActionOrigin::Human => self.local_actor(actor_kind, None),
            ActionOrigin::TraceReplay(_) => self.local_actor(
                actor_kind,
                Some("replayed from the control trace".to_owned()),
            ),
        };
        // What the caller asked becomes what will happen, before the intent
        // line is written: a trace entry names the bar that was marked, not
        // "wherever the pointer is". A replayed entry is already resolved and
        // is validated against that same shape instead.
        let input = if origin.is_trace_replay() {
            action
                .canonical
                .validate(&input)
                .map_err(|error| ControlError::invalid_request(error.to_string()))?;
            input
        } else {
            action
                .input
                .validate(&input)
                .map_err(|error| ControlError::invalid_request(error.to_string()))?;
            let resolved = (action.resolve)(app, &actor, input)?;
            action.canonical.validate(&resolved).map_err(|error| {
                known_error(
                    codes::CAPABILITY_UNAVAILABLE,
                    format!("the action resolved an input it cannot record: {error}"),
                    false,
                )
            })?;
            resolved
        };

        // The trace: a replaying tab records the action at its logical time;
        // a live tab has nothing to record. Opening the sidecar is rare and
        // off the hot path (an action is a human gesture).
        let replaying = {
            let tabs = app.control_tabs();
            let active = &tabs[app
                .control_active_tab_index()
                .min(tabs.len().saturating_sub(1))];
            active
                .replay
                .as_ref()
                .map(|link| (link.session.path.clone(), link.status.elapsed_ms()))
        };
        // A replayed entry is the trace speaking; recording it again would
        // double the sidecar on every run.
        let mut trace: Box<dyn ControlTrace> = match &replaying {
            Some(_) if origin.is_trace_replay() => Box::new(NoTrace),
            Some((session_path, _)) => {
                Box::new(ReplayTraceFile::open(session_path).map_err(|error| {
                    known_error(
                        codes::CAPABILITY_UNAVAILABLE,
                        format!("the replay's control trace cannot be written: {error}"),
                        true,
                    )
                })?)
            }
            None => Box::new(NoTrace),
        };
        let replay_elapsed_ms = replaying.as_ref().map_or(0, |(_, elapsed)| *elapsed);
        let trace_sequence = WireU64::new(self.next_trace_sequence);
        self.next_trace_sequence = self.next_trace_sequence.saturating_add(1);
        let mut entry = TraceEntry {
            trace_version: TRACE_VERSION,
            replay_elapsed_ms,
            sequence: trace_sequence,
            actor_kind,
            client_name: actor.client_name.clone(),
            capability_id: descriptor.id.clone(),
            capability_version: descriptor.version,
            canonical_input: input.clone(),
            expected_revisions: Vec::new(),
            result_code: None,
            result_digest: None,
        };
        trace.append_intent(&entry).map_err(|error| {
            known_error(
                codes::CAPABILITY_UNAVAILABLE,
                format!("the action could not be recorded before it ran: {error}"),
                true,
            )
        })?;

        // A rerun attributes what it produces to the operator the recorded
        // run named, so a replayed session carries the same authorship the
        // original did. Set here rather than earlier, and cleared immediately
        // after: every refusal above returns without running a handler, and a
        // stale author left behind would sign the *next* action's object with
        // the recorded run's operator — or, when that operator was the trader,
        // leave an agent's object carrying no author at all.
        self.replayed_author = match &origin {
            ActionOrigin::TraceReplay(recorded) => Some((**recorded).clone()),
            ActionOrigin::Human | ActionOrigin::Remote(_) => None,
        };
        let outcome = (action.handler)(app, self, &actor, &input).and_then(|result| {
            action
                .output
                .validate(&result)
                .map(|()| result)
                .map_err(|error| ControlError::invalid_request(error.to_string()))
        });
        self.replayed_author = None;
        entry.result_code = Some(match &outcome {
            Ok(_) => quantick_control::id::ErrorCode::new("control.ok")
                .expect("static result code is valid"),
            Err(error) => error.code.clone(),
        });
        entry.result_digest = outcome.as_ref().ok().and_then(result_digest);
        match trace.append_result(&entry) {
            Err(error) => tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_TRACE_RESULT_FAILED",
                error = %error,
                "the control trace did not record an action's result"
            ),
            // Recorded: the walk of this recording learns the action now, so
            // an in-session restart replays it like a fresh process would.
            Ok(()) => {
                // Whoever acted, the entry is on disk and belongs to this
                // pass: an in-session restart must replay exactly what a
                // fresh process would. Only the trace's own re-injection is
                // excluded, and it never reaches here.
                if let Some((session_path, _)) = &replaying
                    && !origin.is_trace_replay()
                    && let Some(state) = self.trace_reinjection.get_mut(session_path)
                {
                    state.record_this_pass(entry);
                }
            }
        }
        outcome
    }

    /// Invoke one registered *read* from inside the application — a launch
    /// hook, or a test.
    ///
    /// The same door a remote client comes through: the same
    /// [`ObserverContract::prepare`], the same permission check against the
    /// scopes the trader configured, the same invocation, the same
    /// serialization. Only the socket is missing. A read reachable one way
    /// from the outside and another way from the inside would be two
    /// implementations of one contract, and the second would be the one
    /// nobody tests.
    ///
    /// The gateway does not have to be enabled: a read costs nothing until it
    /// is asked for, and refusing it because no door is open would make the
    /// hook prove something other than what a client would see.
    ///
    /// [`ObserverContract::prepare`]: crate::control::contract::ObserverContract::prepare
    pub(crate) fn invoke_local_read(
        &mut self,
        app: &QuantickApp,
        capability_id: &str,
        input: Value,
    ) -> Result<Value, ControlError> {
        let Some(identity) = self.identity.as_ref() else {
            return Err(known_error(
                codes::INSTANCE_GONE,
                "the running instance has no control identity",
                false,
            ));
        };
        let instance_id = identity.instance_id.clone();
        let session = identity.session();
        let envelope = RequestEnvelope {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            request_id: RequestId::new(format!("ui-read-{}", self.next_ui_request))
                .expect("generated request ID is valid"),
            instance_id: instance_id.clone(),
            capability_id: quantick_control::id::CapabilityId::new(capability_id).map_err(
                |error| ControlError::invalid_request(format!("invalid capability ID: {error}")),
            )?,
            capability_version: 1,
            expected_revisions: Vec::new(),
            idempotency_key: None,
            dry_run: false,
            reason: None,
            payload: input,
        };
        self.next_ui_request = self.next_ui_request.saturating_add(1);
        let prepared = self.contract.prepare(envelope, &self.configured_scopes)?;
        let profile = self.configured_profile();
        match &prepared.dispatch {
            PreparedDispatch::Worker(_) => prepared
                .dispatch
                .execute_worker(
                    &self.contract,
                    &instance_id,
                    &profile,
                    &self.configured_scopes,
                    &ProtocolLimits::default(),
                )
                .expect("a worker dispatch always answers on the worker path"),
            PreparedDispatch::Ui(_) => {
                let execution = prepared.dispatch.execute_ui(UiReadContext {
                    projections: &mut self.projections,
                    journal: &self.journal,
                    app,
                    instance_id: &instance_id,
                    session: &session,
                    evidence: &self.evidence,
                    screenshot: &mut self.screenshot,
                })?;
                execution
                    .into_serialized()
                    .map(|serialized| serialized.result)
            }
            PreparedDispatch::Parked(_) | PreparedDispatch::Action(_) => Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "only registered reads are invoked from inside the application",
                false,
            )),
        }
    }
}
