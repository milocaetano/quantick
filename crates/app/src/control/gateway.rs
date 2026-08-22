//! Explicitly enabled authenticated loopback gateway and UI-thread dispatcher.

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write as _,
    net::{Ipv4Addr, Shutdown, TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use quantick_control::{
    codec::{BoundedCodec, CodecError, FrameRole},
    cursor::{EventCursor, resolve_event_read},
    descriptor::{
        INSTANCE_DESCRIPTOR_HOST, INSTANCE_DESCRIPTOR_TRANSPORT, INSTANCE_DESCRIPTOR_VERSION,
        InstanceDescriptor,
    },
    error::{ControlError, codes},
    handshake::{
        BearerToken, CURRENT_PROTOCOL_VERSION, HandshakeGrant, HandshakeReply, ProtocolLimits,
        ProtocolVersionRange, accept_handshake,
    },
    id::{
        ConnectionId, EventKind, InstanceId, ModuleId, PermissionId, PrincipalId, ProcessNonce,
        ProfileId, RequestId,
    },
    limits::{
        CONTROL_CLIENT_BURST, CONTROL_CLIENT_RATE_PER_SECOND, CONTROL_HANDSHAKE_TIMEOUT_MS,
        CONTROL_MAX_BUFFERED_RESPONSE_SLOTS, CONTROL_MAX_CONNECTIONS,
        CONTROL_MAX_IN_FLIGHT_PER_CONNECTION, CONTROL_MAX_PARKED_WAITERS,
        CONTROL_REQUEST_QUEUE_CAPACITY, CONTROL_REQUEST_TIMEOUT_MS, CONTROL_RUNTIME_ID_BYTES,
        CONTROL_TOKEN_BYTES, CONTROL_UI_BUDGET_US, CONTROL_UI_MAX_REQUESTS_PER_FRAME,
    },
    wire::{
        ActorContext, ActorKind, ModuleRevision, RequestEnvelope, ResponseEnvelope,
        ResponseOutcome, WireU64,
    },
};
use serde_json::{Value, json};

use crate::{app::QuantickApp, metrics};

use super::{
    actions::{ActionRegistry, standard_actions},
    contract::{
        EventsReadInvocation, OBSERVER_PROFILE_ID, ObserverContract, ParkedWait, PreparedDispatch,
        PreparedRequest, UiReadExecution,
    },
    events::EventsReadInput,
    feed::connection_state,
    interaction::{SelectionSnapshot, selection_snapshot},
    journal::{EventJournal, JournalSignal, NewEvent},
    registry::ProjectionRegistry,
    trace::{
        ControlTrace, NoTrace, ReplayTraceFile, TRACE_VERSION, TraceEntry, TraceReplay,
        result_digest,
    },
};

use quantick_control_local::discovery::publish_descriptor;
#[cfg(test)]
use quantick_control_local::discovery::publish_descriptor_in;

const GATEWAY_COMMAND_CAPACITY: usize = 64;
const GATEWAY_STATUS_CAPACITY: usize = 256;
const GATEWAY_LIFECYCLE_CAPACITY: usize = 8;
const GATEWAY_CRITICAL_STATUS_SLOTS_PER_CONNECTION: usize = 2;
const CONTROL_UI_MAX_STATUS_UPDATES_PER_FRAME: usize = 32;
const CONTROL_PANEL_DEFAULT_WIDTH_PX: f32 = 520.0;
const CONTROL_PANEL_SECTION_SPACING_PX: f32 = 6.0;
const ACCEPT_POLL_MS: u64 = 5;
/// How often the waiter manager re-checks deadlines and the cancellation
/// flag when no tick arrives; a parked wait never waits longer than this past
/// its own timeout.
const WAITER_POLL_MS: u64 = 250;
/// What the journal records as the author of a human action taken in this
/// window. Self-declared like every client name, and honest.
const UI_ACTOR_CLIENT_NAME: &str = "quantick-ui";
/// Take a mark of what is under the pointer (`attention.mark.create`).
pub(crate) const MARK_SHORTCUT: eframe::egui::KeyboardShortcut =
    eframe::egui::KeyboardShortcut::new(eframe::egui::Modifiers::CTRL, eframe::egui::Key::M);
const EXIT_SHUTDOWN_TIMEOUT_MS: u64 = 2_000;

/// The identity a human action in this window is attributed with. One
/// principal and one "connection" per process, generated with the instance
/// identity; it is how the journal tells the human from a remote client.
#[derive(Clone)]
struct UiActorIdentity {
    principal_id: PrincipalId,
    connection_id: ConnectionId,
}

/// Where a local action came from: the human at this window, whose action
/// during a replay is recorded in the control trace, or the control trace
/// itself replaying that action, which must not be recorded again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ActionOrigin {
    Human,
    TraceReplay,
}

/// A replay session whose control trace is being re-injected: the entries
/// still due, in replay-time order.
struct TraceReinjection {
    tab_id: u64,
    session_path: PathBuf,
    pending: std::collections::VecDeque<TraceEntry>,
}

#[derive(Clone)]
struct ProcessIdentity {
    instance_id: InstanceId,
    process_nonce: ProcessNonce,
    process_started_at_unix_ms: i64,
    ui_actor: UiActorIdentity,
}

impl ProcessIdentity {
    fn generate() -> Result<Self, String> {
        let ui_actor = UiActorIdentity {
            principal_id: PrincipalId::from_bytes(random_bytes::<CONTROL_RUNTIME_ID_BYTES>()?),
            connection_id: ConnectionId::from_bytes(random_bytes::<CONTROL_RUNTIME_ID_BYTES>()?),
        };
        Ok(Self {
            ui_actor,
            instance_id: InstanceId::from_bytes(random_bytes::<CONTROL_RUNTIME_ID_BYTES>()?),
            process_nonce: ProcessNonce::from_bytes(random_bytes::<CONTROL_RUNTIME_ID_BYTES>()?),
            process_started_at_unix_ms: metrics::wall_clock_ms(),
        })
    }
}

#[derive(Clone)]
struct GatewayOptions {
    request_queue_capacity: usize,
    max_connections: usize,
    max_in_flight_per_connection: usize,
    handshake_timeout: Duration,
    request_timeout: Duration,
    #[cfg(test)]
    descriptor_directory: Option<PathBuf>,
}

impl Default for GatewayOptions {
    fn default() -> Self {
        Self {
            request_queue_capacity: CONTROL_REQUEST_QUEUE_CAPACITY,
            max_connections: CONTROL_MAX_CONNECTIONS,
            max_in_flight_per_connection: CONTROL_MAX_IN_FLIGHT_PER_CONNECTION,
            handshake_timeout: Duration::from_millis(CONTROL_HANDSHAKE_TIMEOUT_MS),
            request_timeout: Duration::from_millis(CONTROL_REQUEST_TIMEOUT_MS),
            #[cfg(test)]
            descriptor_directory: None,
        }
    }
}

impl GatewayOptions {
    fn validate(&self) -> Result<(), &'static str> {
        let handshake_timeout_limit = Duration::from_millis(CONTROL_HANDSHAKE_TIMEOUT_MS);
        let request_timeout_limit = Duration::from_millis(CONTROL_REQUEST_TIMEOUT_MS);
        if self.request_queue_capacity == 0
            || self.request_queue_capacity > CONTROL_REQUEST_QUEUE_CAPACITY
            || self.max_connections == 0
            || self.max_connections > CONTROL_MAX_CONNECTIONS
            || self.max_in_flight_per_connection == 0
            || self.max_in_flight_per_connection > CONTROL_MAX_IN_FLIGHT_PER_CONNECTION
            || self.handshake_timeout.is_zero()
            || self.handshake_timeout > handshake_timeout_limit
            // Sub-millisecond is zero once advertised in milliseconds, and the
            // handshake would refuse the grant's limits.
            || self.request_timeout.as_millis() == 0
            || self.request_timeout > request_timeout_limit
        {
            return Err("Local gateway limits are invalid; access remains off.");
        }
        Ok(())
    }
}

#[derive(Clone)]
struct GatewayStart {
    identity: ProcessIdentity,
    granted_scopes: BTreeSet<PermissionId>,
    grant_generation: u64,
    options: GatewayOptions,
    cancellation: Arc<AtomicBool>,
    journal_signal: Arc<JournalSignal>,
    journal_ticks: Receiver<()>,
}

struct GatewayRuntime {
    grant_generation: u64,
    requests: Receiver<UiRequest>,
    statuses: Receiver<ConnectionStatus>,
    commands: Sender<GatewayCommand>,
    cancellation: Arc<AtomicBool>,
    public: GatewayPublicInfo,
}

impl GatewayRuntime {
    fn request_shutdown(&self) {
        self.cancellation.store(true, Ordering::Release);
        let _ = self.commands.try_send(GatewayCommand::Shutdown);
    }

    fn revoke(&self, connection_id: ConnectionId) {
        let _ = self
            .commands
            .try_send(GatewayCommand::Revoke(connection_id));
    }
}

#[derive(Clone, Debug)]
struct GatewayPublicInfo {
    instance_id: InstanceId,
    port: u16,
    descriptor_path: PathBuf,
    published_at_unix_ms: i64,
}

enum AccessState {
    Disabled,
    Enabling,
    Enabled(GatewayRuntime),
    Disabling(Option<GatewayRuntime>),
}

enum LifecycleEvent {
    Started {
        generation: u64,
        runtime: GatewayRuntime,
    },
    Failed {
        generation: u64,
        message: String,
    },
    Stopped {
        generation: u64,
    },
}

#[derive(Clone, Debug)]
struct ConnectedClient {
    connection_id: ConnectionId,
    client_name: String,
    connected_at_unix_ms: i64,
    requested_profile: ProfileId,
    effective_profile: ProfileId,
    effective_scopes: BTreeSet<PermissionId>,
    last_request_at_unix_ms: Option<i64>,
}

enum ConnectionStatus {
    Connected(ConnectedClient),
    Requested {
        connection_id: ConnectionId,
        at_unix_ms: i64,
    },
    Disconnected(ConnectionId),
}

enum GatewayCommand {
    Shutdown,
    Revoke(ConnectionId),
    Identified {
        socket_key: u64,
        connection_id: ConnectionId,
    },
    Finished {
        socket_key: u64,
    },
}

struct TrackedSocket {
    stream: TcpStream,
    connection_id: Option<ConnectionId>,
}

struct ClientRateLimiter {
    available_token_nanos: u128,
    last_refill: Instant,
}

impl ClientRateLimiter {
    const ONE_TOKEN_NANOS: u128 = 1_000_000_000;

    fn new() -> Self {
        Self {
            available_token_nanos: u128::from(CONTROL_CLIENT_BURST) * Self::ONE_TOKEN_NANOS,
            last_refill: Instant::now(),
        }
    }

    fn allow(&mut self, now: Instant) -> bool {
        let elapsed_nanos = now.duration_since(self.last_refill).as_nanos();
        self.last_refill = now;
        let capacity = u128::from(CONTROL_CLIENT_BURST) * Self::ONE_TOKEN_NANOS;
        let refill = elapsed_nanos.saturating_mul(u128::from(CONTROL_CLIENT_RATE_PER_SECOND));
        self.available_token_nanos = self
            .available_token_nanos
            .saturating_add(refill)
            .min(capacity);
        if self.available_token_nanos < Self::ONE_TOKEN_NANOS {
            return false;
        }
        self.available_token_nanos -= Self::ONE_TOKEN_NANOS;
        true
    }
}

struct UiRequest {
    prepared: PreparedRequest,
    connection_id: ConnectionId,
    grant_generation: u64,
    deadline: Instant,
    response: Sender<Result<UiReadExecution, ControlError>>,
}

#[derive(Clone, Copy, Debug, Default)]
struct DrainObservation {
    processed: usize,
    elapsed_us: u64,
    /// The drain stopped because the frame budget was spent — before any
    /// request ran (`processed == 0`: statuses and lifecycle ate it) or by
    /// the last capture. Not set when it stopped on the count ceiling or on
    /// an empty queue.
    budget_exceeded: bool,
    queue_has_more: bool,
}

/// UI-owned access state. It never exposes `QuantickApp` to worker threads.
pub(crate) struct ControlAccess {
    identity: Option<ProcessIdentity>,
    initialization_error: Option<String>,
    projections: ProjectionRegistry,
    contract: Arc<ObserverContract>,
    configured_scopes: BTreeSet<PermissionId>,
    grant_generation: u64,
    active_cancellation: Option<Arc<AtomicBool>>,
    state: AccessState,
    lifecycle_tx: Sender<LifecycleEvent>,
    lifecycle_rx: Receiver<LifecycleEvent>,
    connections: BTreeMap<ConnectionId, ConnectedClient>,
    revoked_connections: BTreeSet<ConnectionId>,
    show_panel: bool,
    notice: Option<String>,
    last_drain: DrainObservation,
    /// The semantic event journal. Written only on the application thread;
    /// read through the UI queue; signalled to parked waiters without a lock.
    journal: EventJournal,
    journal_ticks: Receiver<()>,
    actions: ActionRegistry,
    /// What the frame emitter last saw; `None` until access is enabled, so a
    /// disabled gateway costs the frame nothing and an enabled one records
    /// changes, not the state it found.
    semantic_baseline: Option<SemanticKey>,
    next_ui_request: u64,
    next_trace_sequence: u64,
    /// The replay session last seen with a control trace to re-inject, if
    /// any; `None` costs the frame one comparison.
    trace_reinjection: Option<TraceReinjection>,
}

impl ControlAccess {
    pub fn new() -> Self {
        let projections = super::standard_registry()
            .expect("built-in semantic projection registry must be valid");
        let actions = standard_actions().expect("built-in action registry must be valid");
        let contract = Arc::new(
            ObserverContract::new(&projections, &actions)
                .expect("built-in observer capability registry must be valid"),
        );
        let (journal, journal_ticks) = EventJournal::new();
        let (identity, initialization_error) = match ProcessIdentity::generate() {
            Ok(identity) => (Some(identity), None),
            Err(error) => (
                None,
                Some(format!(
                    "Local access is unavailable because secure process identity generation failed: {error}"
                )),
            ),
        };
        let configured_scopes = contract.default_grant();
        let (lifecycle_tx, lifecycle_rx) = bounded(GATEWAY_LIFECYCLE_CAPACITY);
        Self {
            identity,
            initialization_error,
            projections,
            contract,
            configured_scopes,
            grant_generation: 0,
            active_cancellation: None,
            state: AccessState::Disabled,
            lifecycle_tx,
            lifecycle_rx,
            connections: BTreeMap::new(),
            revoked_connections: BTreeSet::new(),
            show_panel: false,
            notice: None,
            last_drain: DrainObservation::default(),
            journal,
            journal_ticks,
            actions,
            semantic_baseline: None,
            next_ui_request: 1,
            next_trace_sequence: 1,
            trace_reinjection: None,
        }
    }

    /// Each frame: if the active tab plays a session that has a control
    /// trace beside it, inject the recorded actions at their logical replay
    /// time (contract §11). A live tab, or a replay without a trace, costs
    /// one comparison. Runs whether or not local access is enabled: replay
    /// determinism does not depend on a client being connected.
    pub(crate) fn service_replay_trace(&mut self, app: &mut QuantickApp) {
        let current = {
            let tabs = app.control_tabs();
            let active = &tabs[app
                .control_active_tab_index()
                .min(tabs.len().saturating_sub(1))];
            active.replay.as_ref().map(|link| {
                (
                    active.id,
                    link.session.path.clone(),
                    link.status.elapsed_ms(),
                )
            })
        };
        let Some((tab_id, session_path, elapsed_ms)) = current else {
            self.trace_reinjection = None;
            return;
        };
        let same_session = self
            .trace_reinjection
            .as_ref()
            .is_some_and(|state| state.tab_id == tab_id && state.session_path == session_path);
        if !same_session {
            let loaded = match TraceReplay::load(&session_path) {
                Ok(loaded) => loaded,
                Err(error) => {
                    tracing::warn!(
                        target: "quantick::control",
                        event_code = "CONTROL_TRACE_UNREADABLE",
                        error = %error,
                        "the replay's control trace could not be read; the run is not a fixture"
                    );
                    TraceReplay::default()
                }
            };
            if !loaded.is_complete() {
                tracing::warn!(
                    target: "quantick::control",
                    event_code = "CONTROL_TRACE_INCOMPLETE",
                    incomplete = ?loaded.incomplete,
                    "the replay's control trace has unfinished intents; the run is not a fixture"
                );
            }
            self.trace_reinjection = Some(TraceReinjection {
                tab_id,
                session_path,
                pending: loaded.completed.into_iter().collect(),
            });
        }
        self.service_replay_trace_continue(app, elapsed_ms);
    }

    /// The loop tail of [`Self::service_replay_trace`], split so the borrow
    /// of the re-injection state does not live across the action call.
    fn service_replay_trace_continue(&mut self, app: &mut QuantickApp, elapsed_ms: i64) {
        loop {
            let due = self.trace_reinjection.as_mut().and_then(|state| {
                state
                    .pending
                    .front()
                    .is_some_and(|entry| entry.replay_elapsed_ms <= elapsed_ms)
                    .then(|| state.pending.pop_front())
                    .flatten()
            });
            let Some(entry) = due else {
                return;
            };
            if let Err(error) = self.invoke_local_action(
                app,
                entry.capability_id.as_str(),
                entry.canonical_input.clone(),
                ActionOrigin::TraceReplay,
            ) {
                tracing::warn!(
                    target: "quantick::control",
                    event_code = "CONTROL_TRACE_REPLAY_REFUSED",
                    capability = %entry.capability_id,
                    code = %error.code,
                    "a traced action was refused on replay"
                );
            }
        }
    }

    #[cfg(test)]
    pub fn journal(&self) -> &EventJournal {
        &self.journal
    }

    pub fn journal_mut(&mut self) -> &mut EventJournal {
        &mut self.journal
    }

    /// The trusted actor context for an action taken in this window by the
    /// human (`HumanUi`) or replayed from a control trace (`Automation`).
    fn local_actor(&mut self, actor_kind: ActorKind, reason: Option<String>) -> ActorContext {
        let identity = self
            .identity
            .as_ref()
            .expect("local actions need the process identity");
        let request_id = RequestId::new(format!("ui-{}", self.next_ui_request))
            .expect("generated request ID is valid");
        self.next_ui_request = self.next_ui_request.saturating_add(1);
        ActorContext {
            actor_kind,
            principal_id: identity.ui_actor.principal_id.clone(),
            client_name: UI_ACTOR_CLIENT_NAME.to_owned(),
            connection_id: identity.ui_actor.connection_id.clone(),
            request_id,
            reason,
            requested_at_unix_ms: metrics::wall_clock_ms(),
        }
    }

    /// Invoke one registered action from inside the application — the hotkey,
    /// the `QUANTICK_CONTROL_MARK` hook, a test, or a replayed trace entry.
    /// Validates the input and the result against the action's schemas, and
    /// during a replay appends the intent and the result to the session's
    /// control trace before and after the handler runs (contract §11).
    pub(crate) fn invoke_local_action(
        &mut self,
        app: &mut QuantickApp,
        capability_id: &str,
        input: Value,
        origin: ActionOrigin,
    ) -> Result<Value, ControlError> {
        let actor_kind = match origin {
            ActionOrigin::Human => ActorKind::HumanUi,
            ActionOrigin::TraceReplay => ActorKind::Automation,
        };
        if self.identity.is_none() {
            return Err(known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "local actions need a process identity, which failed to generate",
                false,
            ));
        }
        let (descriptor, handler) = {
            let (descriptor, handler, input_schema, _) =
                self.actions.lookup(capability_id, 1).ok_or_else(|| {
                    known_error(
                        codes::CAPABILITY_UNKNOWN,
                        "capability ID or version is not a registered action",
                        false,
                    )
                })?;
            input_schema
                .validate(&input)
                .map_err(|error| ControlError::invalid_request(error.to_string()))?;
            (descriptor.clone(), handler)
        };
        let actor = self.local_actor(
            actor_kind,
            (origin == ActionOrigin::TraceReplay)
                .then(|| "replayed from the control trace".to_owned()),
        );

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
            Some(_) if origin == ActionOrigin::TraceReplay => Box::new(NoTrace),
            Some((session_path, _)) => {
                Box::new(ReplayTraceFile::open(session_path).map_err(|error| {
                    known_error(
                        codes::CAPABILITY_UNAVAILABLE,
                        &format!("the replay's control trace cannot be written: {error}"),
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
                &format!("the action could not be recorded before it ran: {error}"),
                true,
            )
        })?;

        let outcome = handler(app, self, &actor, &input).and_then(|result| {
            let (_, _, _, output_schema) = self
                .actions
                .lookup(capability_id, 1)
                .expect("the action was looked up a moment ago");
            output_schema
                .validate(&result)
                .map(|()| result)
                .map_err(|error| ControlError::invalid_request(error.to_string()))
        });
        entry.result_code = Some(match &outcome {
            Ok(_) => quantick_control::id::ErrorCode::new("control.ok")
                .expect("static result code is valid"),
            Err(error) => error.code.clone(),
        });
        entry.result_digest = outcome.as_ref().ok().and_then(result_digest);
        if let Err(error) = trace.append_result(&entry) {
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_TRACE_RESULT_FAILED",
                error = %error,
                "the control trace did not record an action's result"
            );
        }
        outcome
    }

    pub fn begin_frame(&mut self, app: &QuantickApp, ctx: &eframe::egui::Context) {
        let frame_started = Instant::now();
        self.poll_lifecycle();
        let statuses_have_more = self.poll_statuses(frame_started);
        let (requests, generation) = match &self.state {
            AccessState::Enabled(runtime) => {
                (Some(runtime.requests.clone()), runtime.grant_generation)
            }
            AccessState::Disabled | AccessState::Enabling | AccessState::Disabling(_) => (None, 0),
        };
        let Some(requests) = requests else {
            self.last_drain = DrainObservation::default();
            if statuses_have_more {
                ctx.request_repaint();
            }
            return;
        };

        self.emit_semantic_changes(app);

        let configured_scopes = &self.configured_scopes;
        let revoked = &self.revoked_connections;
        let identity = self
            .identity
            .as_ref()
            .expect("an enabled gateway always has a process identity");
        let projections = &mut self.projections;
        let journal = &self.journal;
        self.last_drain = drain_bounded_since(&requests, frame_started, |request| {
            let result = execute_on_ui(
                projections,
                journal,
                app,
                identity,
                generation,
                configured_scopes,
                revoked,
                &request,
            );
            let _ = request.response.try_send(result);
        });
        if statuses_have_more || self.last_drain.queue_has_more {
            ctx.request_repaint();
        }
    }

    /// Record the semantic changes since the last frame: tab, focus,
    /// selection, feed connection and market, replay state. A handful of
    /// small comparisons, no allocation unless something changed, and nothing
    /// at all while access is disabled — the journal starts when the human
    /// opens the door, and records changes rather than the state it found.
    fn emit_semantic_changes(&mut self, app: &QuantickApp) {
        let key = semantic_key(app);
        let Some(previous) = self.semantic_baseline.take() else {
            self.semantic_baseline = Some(key);
            return;
        };
        let now = metrics::wall_clock_ms();
        if previous.active_tab_id != key.active_tab_id {
            self.record_observed(
                "workspace",
                "workspace.tab.activated",
                json!({ "tab_id": key.active_tab_id.to_string() }),
                now,
            );
        }
        if previous.focused_pane_id != key.focused_pane_id {
            self.record_observed(
                "workspace",
                "workspace.focus.changed",
                json!({
                    "tab_id": key.active_tab_id.to_string(),
                    "pane_id": key.focused_pane_id.to_string(),
                }),
                now,
            );
        }
        if previous.selection != key.selection {
            self.record_observed(
                "interaction",
                "interaction.selection.changed",
                json!({ "selection": key.selection }),
                now,
            );
        }
        for tab in &key.tabs {
            match previous.tabs.iter().find(|old| old.tab_id == tab.tab_id) {
                None => self.record_observed(
                    "workspace",
                    "workspace.tab.opened",
                    json!({ "tab_id": tab.tab_id.to_string(), "feed_id": tab.feed_id, "symbol": tab.symbol }),
                    now,
                ),
                Some(old) => {
                    if old.feed_id != tab.feed_id || old.symbol != tab.symbol {
                        self.record_observed(
                            "feed",
                            "feed.market.changed",
                            json!({ "tab_id": tab.tab_id.to_string(), "feed_id": tab.feed_id, "symbol": tab.symbol }),
                            now,
                        );
                    }
                    if old.connection != tab.connection {
                        self.record_observed(
                            "feed",
                            "feed.connection.changed",
                            json!({ "tab_id": tab.tab_id.to_string(), "state": tab.connection }),
                            now,
                        );
                    }
                    if old.replay != tab.replay {
                        self.record_observed(
                            "replay",
                            "replay.state.changed",
                            json!({
                                "tab_id": tab.tab_id.to_string(),
                                "active": tab.replay.is_some(),
                                "playing": tab.replay.map(|(playing, _)| playing),
                                "finished": tab.replay.map(|(_, finished)| finished),
                            }),
                            now,
                        );
                    }
                }
            }
        }
        for old in &previous.tabs {
            if !key.tabs.iter().any(|tab| tab.tab_id == old.tab_id) {
                self.record_observed(
                    "workspace",
                    "workspace.tab.closed",
                    json!({ "tab_id": old.tab_id.to_string() }),
                    now,
                );
            }
        }
        self.semantic_baseline = Some(key);
    }

    fn record_observed(&mut self, module: &str, kind: &str, payload: Value, now: i64) {
        self.journal.record(
            NewEvent {
                module_id: ModuleId::new(module).expect("static module ID is valid"),
                kind: EventKind::new(kind).expect("static event kind is valid"),
                actor: None,
                payload,
            },
            now,
        );
    }

    pub fn needs_frame_service(&self) -> bool {
        !matches!(self.state, AccessState::Disabled)
    }

    pub fn open_panel(&mut self) {
        self.show_panel = true;
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self.state, AccessState::Enabled(_))
    }

    pub fn menu_label(&self) -> &'static str {
        match self.state {
            AccessState::Disabled => "Local agent access…",
            AccessState::Enabling => "Local agent access: enabling…",
            AccessState::Enabled(_) => "Local agent access: on",
            AccessState::Disabling(_) => "Local agent access: disabling…",
        }
    }

    pub fn draw_panel(&mut self, ctx: &eframe::egui::Context) {
        if !self.show_panel {
            return;
        }
        let mut open = self.show_panel;
        eframe::egui::Window::new("Local agent access")
            .id(eframe::egui::Id::new("control_access_panel"))
            .open(&mut open)
            .default_width(CONTROL_PANEL_DEFAULT_WIDTH_PX)
            .resizable(true)
            .show(ctx, |ui| self.draw_panel_body(ui));
        self.show_panel = open;
    }

    fn draw_panel_body(&mut self, ui: &mut eframe::egui::Ui) {
        ui.label(
            "Allows configured local tools such as Codex or Claude to read this already-open Quantick window. Granted data may be sent to the model provider used by that tool.",
        );
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        let status = match self.state {
            AccessState::Disabled => "Off",
            AccessState::Enabling => "Enabling…",
            AccessState::Enabled(_) => "On — observer only",
            AccessState::Disabling(_) => "Disabling and revoking clients…",
        };
        ui.horizontal(|ui| {
            ui.strong("Status:");
            ui.label(status);
        });

        if let Some(error) = &self.initialization_error {
            ui.colored_label(eframe::egui::Color32::LIGHT_RED, error);
        }
        if let Some(notice) = &self.notice {
            ui.label(notice);
        }
        if let AccessState::Enabled(runtime) = &self.state {
            ui.monospace(format!(
                "{}:{} · instance {}",
                INSTANCE_DESCRIPTOR_HOST, runtime.public.port, runtime.public.instance_id
            ));
            ui.small(format!(
                "Published at {} · descriptor {}",
                runtime.public.published_at_unix_ms,
                runtime.public.descriptor_path.display()
            ));
        }

        ui.separator();
        ui.strong("Read scopes for the next connection");
        let can_edit = matches!(self.state, AccessState::Disabled);
        for descriptor in self.contract.selectable_permissions() {
            let mut selected = self.configured_scopes.contains(&descriptor.id);
            // The description is the label — a first-week user reads "Chart
            // framing, viewport, and bars", not `observe.chart` — and the ID
            // stays beside it because it is what a client asks for by name.
            let label = if descriptor.sensitive {
                format!("{} · {} (sensitive)", descriptor.description, descriptor.id)
            } else {
                format!("{} · {}", descriptor.description, descriptor.id)
            };
            ui.add_enabled(can_edit, eframe::egui::Checkbox::new(&mut selected, label));
            if can_edit {
                if selected {
                    self.configured_scopes.insert(descriptor.id.clone());
                } else {
                    self.configured_scopes.remove(&descriptor.id);
                }
            }
        }
        if !can_edit {
            ui.small("Disable access before changing scopes; re-enabling rotates the token and requires a new handshake.");
        }

        ui.separator();
        match self.state {
            AccessState::Disabled => {
                if ui
                    .add_enabled(
                        self.identity.is_some(),
                        eframe::egui::Button::new("Enable observer access"),
                    )
                    .clicked()
                {
                    self.enable(ui.ctx());
                }
            }
            AccessState::Enabling => {
                if ui.button("Cancel and keep access off").clicked() {
                    self.request_disable();
                }
            }
            AccessState::Enabled(_) => {
                if ui.button("Disable and revoke all clients").clicked() {
                    self.request_disable();
                }
            }
            AccessState::Disabling(_) => {
                ui.spinner();
            }
        }

        ui.separator();
        ui.strong(format!("Connected clients ({})", self.connections.len()));
        if self.connections.is_empty() {
            ui.label("No authenticated clients.");
        } else {
            let mut revoke = None;
            for client in self.connections.values() {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&client.client_name);
                        if ui.button("Revoke").clicked() {
                            revoke = Some(client.connection_id.clone());
                        }
                    });
                    ui.small(format!(
                        "requested {} · effective {} · connected {} · last request {}",
                        client.requested_profile,
                        client.effective_profile,
                        client.connected_at_unix_ms,
                        client
                            .last_request_at_unix_ms
                            .map_or_else(|| "none".to_owned(), |at| at.to_string())
                    ));
                    ui.small(format!(
                        "scopes: {}",
                        client
                            .effective_scopes
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                });
            }
            if let Some(connection_id) = revoke {
                self.revoke(connection_id);
            }
        }

        ui.separator();
        ui.small(format!(
            "Last UI drain: {} request(s), {} µs, budget {} µs{}",
            self.last_drain.processed,
            self.last_drain.elapsed_us,
            CONTROL_UI_BUDGET_US,
            match (self.last_drain.budget_exceeded, self.last_drain.processed) {
                (true, 0) => " (budget spent before any request ran)",
                (true, _) => " (budget exceeded by one non-preemptible capture)",
                (false, _) => "",
            }
        ));
        let projection = self.projections.performance();
        ui.small(format!(
            "Projection captures: {} · last {} µs · worst {} µs · budget violations {}",
            projection.captures,
            projection.last_capture_us,
            projection.worst_capture_us,
            projection.budget_violations
        ));
    }

    /// Enable observer access with the reviewed defaults. The panel button, the
    /// `QUANTICK_CONTROL_ACCESS` hook and tests all arrive here; there is no
    /// second path to an enabled gateway.
    pub fn enable(&mut self, ctx: &eframe::egui::Context) {
        self.request_enable(ctx, GatewayOptions::default());
    }

    fn request_enable(&mut self, ctx: &eframe::egui::Context, options: GatewayOptions) {
        if !matches!(self.state, AccessState::Disabled) {
            return;
        }
        let Some(identity) = self.identity.clone() else {
            return;
        };
        self.grant_generation = self.grant_generation.saturating_add(1).max(1);
        let cancellation = Arc::new(AtomicBool::new(false));
        let start = GatewayStart {
            identity,
            granted_scopes: self.configured_scopes.clone(),
            grant_generation: self.grant_generation,
            options,
            cancellation: Arc::clone(&cancellation),
            journal_signal: self.journal.signal(),
            journal_ticks: self.journal_ticks.clone(),
        };
        let lifecycle = self.lifecycle_tx.clone();
        let contract = Arc::clone(&self.contract);
        let repaint = ctx.clone();
        self.active_cancellation = Some(cancellation);
        self.state = AccessState::Enabling;
        self.notice = Some("Creating a new private token and loopback endpoint…".to_owned());
        let spawn = thread::Builder::new()
            .name("quantick-control-gateway".to_owned())
            .spawn(move || {
                gateway_main(start, contract, lifecycle, move || {
                    repaint.request_repaint()
                })
            });
        if let Err(error) = spawn {
            self.active_cancellation = None;
            self.state = AccessState::Disabled;
            self.notice = Some(format!("Could not start the local gateway thread: {error}"));
        }
    }

    fn request_disable(&mut self) {
        if let Some(cancellation) = &self.active_cancellation {
            cancellation.store(true, Ordering::Release);
        }
        self.semantic_baseline = None;
        let previous = std::mem::replace(&mut self.state, AccessState::Disabled);
        self.grant_generation = self.grant_generation.saturating_add(1).max(1);
        self.revoked_connections
            .extend(self.connections.keys().cloned());
        self.connections.clear();
        self.notice = Some("Revoking clients and removing discovery…".to_owned());
        self.state = match previous {
            AccessState::Enabled(runtime) => {
                runtime.request_shutdown();
                AccessState::Disabling(Some(runtime))
            }
            AccessState::Enabling => AccessState::Disabling(None),
            AccessState::Disabling(runtime) => AccessState::Disabling(runtime),
            AccessState::Disabled => AccessState::Disabled,
        };
    }

    /// Revoke one authenticated client: its queued work is refused before
    /// dispatch and its socket is closed. The panel's per-client button and
    /// tests arrive here.
    pub(crate) fn revoke(&mut self, connection_id: ConnectionId) {
        self.revoked_connections.insert(connection_id.clone());
        if let AccessState::Enabled(runtime) = &self.state {
            runtime.revoke(connection_id.clone());
        }
        self.connections.remove(&connection_id);
    }

    fn poll_lifecycle(&mut self) {
        while let Ok(event) = self.lifecycle_rx.try_recv() {
            match event {
                LifecycleEvent::Started {
                    generation,
                    runtime,
                } if generation == self.grant_generation
                    && matches!(self.state, AccessState::Enabling) =>
                {
                    self.notice = Some(
                        "Observer access enabled. The token stays only in the private descriptor."
                            .to_owned(),
                    );
                    self.state = AccessState::Enabled(runtime);
                }
                LifecycleEvent::Started { runtime, .. } => {
                    runtime.request_shutdown();
                    self.state = AccessState::Disabling(Some(runtime));
                }
                LifecycleEvent::Failed {
                    generation,
                    message,
                } if generation == self.grant_generation => {
                    self.active_cancellation = None;
                    self.notice = Some(message);
                    self.state = AccessState::Disabled;
                }
                LifecycleEvent::Failed {
                    generation,
                    message,
                } if generation < self.grant_generation
                    && matches!(self.state, AccessState::Disabling(None)) =>
                {
                    self.active_cancellation = None;
                    self.notice = Some(format!(
                        "Local observer access remained off after cancellation: {message}"
                    ));
                    self.state = AccessState::Disabled;
                }
                LifecycleEvent::Failed { .. } => {}
                LifecycleEvent::Stopped { generation } => {
                    let expected = matches!(self.state, AccessState::Disabling(_));
                    if generation <= self.grant_generation {
                        self.active_cancellation = None;
                        self.connections.clear();
                        self.revoked_connections.clear();
                        self.state = AccessState::Disabled;
                        self.notice = Some(if expected {
                            "Local observer access is off and discovery was removed.".to_owned()
                        } else {
                            "The local gateway stopped unexpectedly; observer access is off."
                                .to_owned()
                        });
                    }
                }
            }
        }
    }

    fn poll_statuses(&mut self, frame_started: Instant) -> bool {
        let statuses = match &self.state {
            AccessState::Enabled(runtime) => Some(runtime.statuses.clone()),
            AccessState::Disabling(Some(runtime)) => Some(runtime.statuses.clone()),
            AccessState::Disabled | AccessState::Enabling | AccessState::Disabling(None) => None,
        };
        let Some(statuses) = statuses else {
            return false;
        };
        let mut processed = 0usize;
        while processed < CONTROL_UI_MAX_STATUS_UPDATES_PER_FRAME
            && u64::try_from(frame_started.elapsed().as_micros()).unwrap_or(u64::MAX)
                < CONTROL_UI_BUDGET_US
        {
            let Ok(status) = statuses.try_recv() else {
                break;
            };
            match status {
                ConnectionStatus::Connected(client) => {
                    if !self.revoked_connections.contains(&client.connection_id) {
                        self.connections
                            .insert(client.connection_id.clone(), client);
                    }
                }
                ConnectionStatus::Requested {
                    connection_id,
                    at_unix_ms,
                } => {
                    if let Some(client) = self.connections.get_mut(&connection_id) {
                        client.last_request_at_unix_ms = Some(at_unix_ms);
                    }
                }
                ConnectionStatus::Disconnected(connection_id) => {
                    self.connections.remove(&connection_id);
                    self.revoked_connections.remove(&connection_id);
                }
            }
            processed += 1;
        }
        !statuses.is_empty()
    }

    pub fn shutdown_for_exit(&mut self) {
        if let Some(cancellation) = &self.active_cancellation {
            cancellation.store(true, Ordering::Release);
        }
        match &self.state {
            AccessState::Enabled(runtime) => runtime.request_shutdown(),
            AccessState::Disabling(Some(runtime)) => runtime.request_shutdown(),
            // Nothing runs and nothing will report: the default state, and
            // exit must cost it nothing.
            AccessState::Disabled => return,
            AccessState::Enabling | AccessState::Disabling(None) => {}
        }
        let deadline = Instant::now() + Duration::from_millis(EXIT_SHUTDOWN_TIMEOUT_MS);
        while Instant::now() < deadline {
            match self
                .lifecycle_rx
                .recv_timeout(Duration::from_millis(ACCEPT_POLL_MS * 2))
            {
                Ok(LifecycleEvent::Started { runtime, .. }) => runtime.request_shutdown(),
                Ok(LifecycleEvent::Stopped { .. } | LifecycleEvent::Failed { .. }) => {
                    self.state = AccessState::Disabled;
                    return;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            }
        }
        tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_GATEWAY_SHUTDOWN_TIMEOUT",
            "gateway cleanup did not acknowledge before application exit"
        );
    }

    #[cfg(test)]
    pub(crate) fn enable_for_test(
        &mut self,
        ctx: &eframe::egui::Context,
        descriptor_directory: PathBuf,
        request_queue_capacity: usize,
    ) {
        let options = GatewayOptions {
            request_queue_capacity,
            descriptor_directory: Some(descriptor_directory),
            ..GatewayOptions::default()
        };
        self.request_enable(ctx, options);
    }

    #[cfg(test)]
    pub(crate) fn enable_for_test_with_limits(
        &mut self,
        ctx: &eframe::egui::Context,
        descriptor_directory: PathBuf,
        request_queue_capacity: usize,
        request_timeout: Duration,
        max_connections: usize,
    ) {
        let options = GatewayOptions {
            request_queue_capacity,
            request_timeout,
            max_connections,
            descriptor_directory: Some(descriptor_directory),
            ..GatewayOptions::default()
        };
        self.request_enable(ctx, options);
    }

    #[cfg(test)]
    pub(crate) fn disable_for_test(&mut self) {
        self.request_disable();
    }

    #[cfg(test)]
    pub(crate) fn queued_requests_for_test(&self) -> usize {
        match &self.state {
            AccessState::Enabled(runtime) => runtime.requests.len(),
            AccessState::Disabled | AccessState::Enabling | AccessState::Disabling(_) => 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn descriptor_path_for_test(&self) -> Option<PathBuf> {
        match &self.state {
            AccessState::Enabled(runtime) => Some(runtime.public.descriptor_path.clone()),
            AccessState::Disabled | AccessState::Enabling | AccessState::Disabling(_) => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_disabled_for_test(&self) -> bool {
        matches!(self.state, AccessState::Disabled)
    }

    #[cfg(test)]
    pub(crate) fn connection_ids_for_test(&self) -> Vec<ConnectionId> {
        self.connections.keys().cloned().collect()
    }
}

impl Drop for ControlAccess {
    fn drop(&mut self) {
        if let Some(cancellation) = &self.active_cancellation {
            cancellation.store(true, Ordering::Release);
        }
        match &self.state {
            AccessState::Enabled(runtime) => runtime.request_shutdown(),
            AccessState::Disabling(Some(runtime)) => runtime.request_shutdown(),
            AccessState::Disabled | AccessState::Enabling | AccessState::Disabling(None) => {}
        }
    }
}

/// What the frame emitter compares between frames. Small, owned, `Eq`; the
/// selection snapshot is the same projection the selection scope publishes,
/// so "changed" means the same thing to the journal and to a capture.
#[derive(Clone, Debug, Eq, PartialEq)]
struct SemanticKey {
    active_tab_id: u64,
    focused_pane_id: u64,
    selection: SelectionSnapshot,
    tabs: Vec<TabKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TabKey {
    tab_id: u64,
    feed_id: String,
    symbol: String,
    connection: &'static str,
    /// `(playing, finished)` while a replay is linked.
    replay: Option<(bool, bool)>,
}

fn semantic_key(app: &QuantickApp) -> SemanticKey {
    let tabs = app.control_tabs();
    let active = &tabs[app
        .control_active_tab_index()
        .min(tabs.len().saturating_sub(1))];
    SemanticKey {
        active_tab_id: active.id,
        focused_pane_id: active.pane(active.focused_side()).id,
        selection: selection_snapshot(app),
        tabs: tabs
            .iter()
            .map(|tab| TabKey {
                tab_id: tab.id,
                feed_id: tab.active.0.clone(),
                symbol: tab.active.1.clone(),
                connection: connection_state(tab.feed_connection),
                replay: tab
                    .replay
                    .as_ref()
                    .map(|link| (link.status.is_playing(), link.status.is_finished())),
            })
            .collect(),
    }
}

fn drain_bounded_since<T>(
    receiver: &Receiver<T>,
    started: Instant,
    mut handle: impl FnMut(T),
) -> DrainObservation {
    let mut processed = 0usize;
    // Why the drain stopped is recorded where it stops, not re-derived from
    // a later clock reading: the count ceiling, the budget, or an empty queue
    // are three different diagnoses.
    let mut stopped_on_budget = false;
    loop {
        if processed >= CONTROL_UI_MAX_REQUESTS_PER_FRAME {
            break;
        }
        if u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX) >= CONTROL_UI_BUDGET_US
        {
            stopped_on_budget = true;
            break;
        }
        match receiver.try_recv() {
            Ok(request) => {
                handle(request);
                processed += 1;
            }
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
        }
    }
    let elapsed_us = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    DrainObservation {
        processed,
        elapsed_us,
        budget_exceeded: stopped_on_budget,
        queue_has_more: !receiver.is_empty(),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_on_ui(
    projections: &mut ProjectionRegistry,
    journal: &EventJournal,
    app: &QuantickApp,
    identity: &ProcessIdentity,
    current_generation: u64,
    current_scopes: &BTreeSet<PermissionId>,
    revoked_connections: &BTreeSet<ConnectionId>,
    request: &UiRequest,
) -> Result<UiReadExecution, ControlError> {
    if request.deadline <= Instant::now() {
        return Err(known_error(
            codes::TIMEOUT,
            "request expired before application-thread dispatch",
            true,
        ));
    }
    if request.grant_generation != current_generation
        || revoked_connections.contains(&request.connection_id)
    {
        return Err(known_error(
            codes::PERMISSION_DENIED,
            "connection authority was revoked before dispatch",
            false,
        ));
    }
    if request.prepared.envelope.instance_id != identity.instance_id {
        return Err(known_error(
            codes::INSTANCE_GONE,
            "request names a different running instance",
            false,
        ));
    }
    if !request
        .prepared
        .required_permissions
        .is_subset(current_scopes)
    {
        return Err(known_error(
            codes::SCOPE_DENIED,
            "a required read scope was removed before dispatch",
            false,
        ));
    }

    request
        .prepared
        .dispatch
        .execute_ui(projections, journal, app, &identity.instance_id)
}

fn gateway_main(
    start: GatewayStart,
    contract: Arc<ObserverContract>,
    lifecycle: Sender<LifecycleEvent>,
    wake: impl Fn() + Send + Sync + 'static,
) {
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
    if let Err(message) = gateway_run(start.clone(), contract, &lifecycle, wake) {
        let _ = lifecycle.send(LifecycleEvent::Failed {
            generation: start.grant_generation,
            message,
        });
    }
}

fn gateway_run(
    start: GatewayStart,
    contract: Arc<ObserverContract>,
    lifecycle: &Sender<LifecycleEvent>,
    wake: Arc<dyn Fn() + Send + Sync>,
) -> Result<(), String> {
    start.options.validate().map_err(str::to_owned)?;
    let token = BearerToken::from_bytes(random_bytes::<CONTROL_TOKEN_BYTES>()?);
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .map_err(|error| format!("Could not bind the private loopback gateway: {error}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("Could not configure the loopback gateway: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("Could not read the loopback gateway address: {error}"))?;
    if address.ip() != Ipv4Addr::LOCALHOST || address.port() == 0 {
        return Err("Gateway did not bind literal IPv4 loopback; access remains off.".to_owned());
    }
    let published_at_unix_ms = metrics::wall_clock_ms();
    let descriptor = InstanceDescriptor {
        descriptor_version: INSTANCE_DESCRIPTOR_VERSION,
        instance_id: start.identity.instance_id.clone(),
        process_nonce: start.identity.process_nonce.clone(),
        process_id: std::process::id(),
        process_started_at_unix_ms: start.identity.process_started_at_unix_ms,
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        application_commit: option_env!("QUANTICK_GIT_COMMIT")
            .unwrap_or("unknown")
            .to_owned(),
        protocol_versions: ProtocolVersionRange::new(
            CURRENT_PROTOCOL_VERSION,
            CURRENT_PROTOCOL_VERSION,
        )
        .expect("current protocol range is valid"),
        transport: INSTANCE_DESCRIPTOR_TRANSPORT.to_owned(),
        host: INSTANCE_DESCRIPTOR_HOST.to_owned(),
        port: address.port(),
        bearer_token: token.clone(),
        published_at_unix_ms,
    };
    #[cfg(test)]
    let published = match &start.options.descriptor_directory {
        Some(directory) => publish_descriptor_in(directory, &descriptor),
        None => publish_descriptor(&descriptor),
    };
    #[cfg(not(test))]
    let published = publish_descriptor(&descriptor);
    let published = published.map_err(|error| {
        format!("Could not publish private gateway discovery; access remains off: {error}")
    })?;

    let (request_tx, request_rx) = bounded(start.options.request_queue_capacity);
    let (status_tx, status_rx) = bounded(GATEWAY_STATUS_CAPACITY);
    let (command_tx, command_rx) = bounded(GATEWAY_COMMAND_CAPACITY);
    let runtime = GatewayRuntime {
        grant_generation: start.grant_generation,
        requests: request_rx,
        statuses: status_rx,
        commands: command_tx.clone(),
        cancellation: Arc::clone(&start.cancellation),
        public: GatewayPublicInfo {
            instance_id: start.identity.instance_id.clone(),
            port: address.port(),
            descriptor_path: published.path().to_path_buf(),
            published_at_unix_ms,
        },
    };
    if lifecycle
        .send(LifecycleEvent::Started {
            generation: start.grant_generation,
            runtime,
        })
        .is_err()
    {
        let _ = published.remove();
        return Ok(());
    }
    wake();
    tracing::info!(
        target: "quantick::control",
        event_code = "CONTROL_GATEWAY_ENABLED",
        instance_id = %start.identity.instance_id,
        port = address.port(),
        "local observer gateway enabled"
    );

    let (park_tx, park_rx) = bounded(CONTROL_MAX_PARKED_WAITERS);
    {
        let ticks = start.journal_ticks.clone();
        let signal = Arc::clone(&start.journal_signal);
        let cancellation = Arc::clone(&start.cancellation);
        if thread::Builder::new()
            .name("quantick-control-waiters".to_owned())
            .spawn(move || waiter_manager(ticks, park_rx, signal, cancellation))
            .is_err()
        {
            return Err(
                "Could not start the gateway's waiter manager; access remains off.".to_owned(),
            );
        }
    }
    let authority = Arc::new(ConnectionAuthority {
        identity: start.identity.clone(),
        bearer_token: token,
        granted_scopes: start.granted_scopes.clone(),
        grant_generation: start.grant_generation,
        options: start.options.clone(),
        contract,
        requests: request_tx,
        statuses: status_tx,
        commands: command_tx,
        global_in_flight: Arc::new(AtomicUsize::new(0)),
        cancellation: Arc::clone(&start.cancellation),
        wake: Arc::clone(&wake),
        journal_signal: Arc::clone(&start.journal_signal),
        park: park_tx,
        parked_waiters: Arc::new(AtomicUsize::new(0)),
    });
    accept_loop(listener, command_rx, Arc::clone(&authority));

    if let Err(error) = published.remove() {
        tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_DESCRIPTOR_REMOVE_FAILED",
            error = %error,
            "could not remove the local gateway descriptor"
        );
    }
    tracing::info!(
        target: "quantick::control",
        event_code = "CONTROL_GATEWAY_DISABLED",
        instance_id = %start.identity.instance_id,
        "local observer gateway disabled"
    );
    let _ = lifecycle.send(LifecycleEvent::Stopped {
        generation: start.grant_generation,
    });
    wake();
    Ok(())
}

/// A `wait_for_change` registered with the waiter manager: wake it when the
/// journal passes `target_sequence`, or at `deadline`.
struct ParkedWaiter {
    target_sequence: u64,
    deadline: Instant,
    wake: Sender<WakeReason>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WakeReason {
    Woken,
    TimedOut,
    Shutdown,
}

/// The waiter manager: one thread per gateway run that owns the parked
/// waiters, listens to the journal's tick, and wakes each waiter when its
/// position is behind the journal or its deadline passed. The application
/// thread never sees it: it only stores an atomic and tries one send.
fn waiter_manager(
    ticks: Receiver<()>,
    park: Receiver<ParkedWaiter>,
    signal: Arc<JournalSignal>,
    cancellation: Arc<AtomicBool>,
) {
    let mut waiters: Vec<ParkedWaiter> = Vec::new();
    loop {
        let now = Instant::now();
        let poll = waiters
            .iter()
            .map(|waiter| waiter.deadline.saturating_duration_since(now))
            .min()
            .unwrap_or(Duration::from_millis(WAITER_POLL_MS))
            .min(Duration::from_millis(WAITER_POLL_MS));
        crossbeam_channel::select! {
            recv(ticks) -> _ => {}
            recv(park) -> waiter => match waiter {
                Ok(waiter) => waiters.push(waiter),
                Err(_) => break,
            },
            default(poll) => {}
        }
        if cancellation.load(Ordering::Acquire) {
            for waiter in waiters.drain(..) {
                let _ = waiter.wake.send(WakeReason::Shutdown);
            }
            break;
        }
        let next = signal.next_sequence();
        let now = Instant::now();
        waiters.retain(|waiter| {
            if next > waiter.target_sequence {
                let _ = waiter.wake.send(WakeReason::Woken);
                false
            } else if now >= waiter.deadline {
                let _ = waiter.wake.send(WakeReason::TimedOut);
                false
            } else {
                true
            }
        });
    }
    for waiter in waiters.drain(..) {
        let _ = waiter.wake.send(WakeReason::Shutdown);
    }
}

struct ConnectionAuthority {
    identity: ProcessIdentity,
    bearer_token: BearerToken,
    granted_scopes: BTreeSet<PermissionId>,
    grant_generation: u64,
    options: GatewayOptions,
    contract: Arc<ObserverContract>,
    requests: Sender<UiRequest>,
    statuses: Sender<ConnectionStatus>,
    commands: Sender<GatewayCommand>,
    global_in_flight: Arc<AtomicUsize>,
    cancellation: Arc<AtomicBool>,
    wake: Arc<dyn Fn() + Send + Sync>,
    journal_signal: Arc<JournalSignal>,
    park: Sender<ParkedWaiter>,
    parked_waiters: Arc<AtomicUsize>,
}

fn accept_loop(
    listener: TcpListener,
    commands: Receiver<GatewayCommand>,
    authority: Arc<ConnectionAuthority>,
) {
    let mut sockets = BTreeMap::<u64, TrackedSocket>::new();
    let mut next_socket_key = 1u64;
    let mut shutdown = false;
    while !shutdown && !authority.cancellation.load(Ordering::Acquire) {
        loop {
            match commands.try_recv() {
                Ok(GatewayCommand::Shutdown) => {
                    shutdown = true;
                    break;
                }
                Ok(GatewayCommand::Revoke(connection_id)) => {
                    for socket in sockets.values() {
                        if socket.connection_id.as_ref() == Some(&connection_id) {
                            let _ = socket.stream.shutdown(Shutdown::Both);
                        }
                    }
                }
                Ok(GatewayCommand::Identified {
                    socket_key,
                    connection_id,
                }) => {
                    if let Some(socket) = sockets.get_mut(&socket_key) {
                        socket.connection_id = Some(connection_id);
                    }
                }
                Ok(GatewayCommand::Finished { socket_key }) => {
                    sockets.remove(&socket_key);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    shutdown = true;
                    break;
                }
            }
        }
        if shutdown {
            break;
        }
        if authority.cancellation.load(Ordering::Acquire) {
            break;
        }
        loop {
            match listener.accept() {
                Ok((stream, peer)) => {
                    if peer.ip() != Ipv4Addr::LOCALHOST {
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    if stream.set_nonblocking(false).is_err() {
                        let _ = stream.shutdown(Shutdown::Both);
                        continue;
                    }
                    if sockets.len() >= authority.options.max_connections {
                        // Off the accept thread: the rejection reads the
                        // client's handshake first (see the function), which
                        // may wait up to the handshake timeout.
                        let options = authority.options.clone();
                        let _ = thread::Builder::new()
                            .name("quantick-control-reject".to_owned())
                            .spawn(move || reject_connection_capacity(stream, &options));
                        continue;
                    }
                    let Some(next_socket_key_value) = next_socket_key.checked_add(1) else {
                        let _ = stream.shutdown(Shutdown::Both);
                        tracing::error!(
                            target: "quantick::control",
                            event_code = "CONTROL_SOCKET_ID_EXHAUSTED",
                            "local gateway exhausted its monotonic socket identity space"
                        );
                        shutdown = true;
                        break;
                    };
                    let socket_key = next_socket_key;
                    next_socket_key = next_socket_key_value;
                    let tracked = match stream.try_clone() {
                        Ok(stream) => stream,
                        Err(_) => {
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                    };
                    sockets.insert(
                        socket_key,
                        TrackedSocket {
                            stream: tracked,
                            connection_id: None,
                        },
                    );
                    let connection_authority = Arc::clone(&authority);
                    let spawn = thread::Builder::new()
                        .name(format!("quantick-control-connection-{socket_key}"))
                        .spawn(move || connection_main(socket_key, stream, connection_authority));
                    if spawn.is_err()
                        && let Some(socket) = sockets.remove(&socket_key)
                    {
                        let _ = socket.stream.shutdown(Shutdown::Both);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                // A peer that reset or aborted before accept, or a signal, is
                // that connection's failure, not the listener's: the gateway
                // stays up for everyone else.
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::Interrupted
                    ) =>
                {
                    tracing::debug!(
                        target: "quantick::control",
                        event_code = "CONTROL_GATEWAY_ACCEPT_TRANSIENT",
                        error = %error,
                        "loopback gateway accept hit a transient error; continuing"
                    );
                    continue;
                }
                Err(error) => {
                    tracing::warn!(
                        target: "quantick::control",
                        event_code = "CONTROL_GATEWAY_ACCEPT_FAILED",
                        error = %error,
                        "loopback gateway accept failed; access is stopping"
                    );
                    shutdown = true;
                    break;
                }
            }
        }
        if shutdown {
            break;
        }
        thread::sleep(Duration::from_millis(ACCEPT_POLL_MS));
    }
    for socket in sockets.values() {
        let _ = socket.stream.shutdown(Shutdown::Both);
    }
}

fn reject_connection_capacity(mut stream: TcpStream, options: &GatewayOptions) {
    let _ = stream.set_read_timeout(Some(options.handshake_timeout));
    let _ = stream.set_write_timeout(Some(options.handshake_timeout));
    let codec = BoundedCodec::handshake();
    // Read the client's handshake before answering: closing a socket with
    // unread data resets it, and a reset can discard the rejection before
    // the client has read it. A frame that never comes or is malformed
    // changes nothing — the answer is the same.
    let _ = codec.read_handshake_request(&mut stream);
    let reply = HandshakeReply::Rejected {
        error: known_error(
            codes::BACKPRESSURE,
            "local gateway connection capacity is full",
            true,
        ),
    };
    if let Ok(frame) = codec.encode(FrameRole::Response, &reply) {
        let _ = stream.write_all(&frame);
    }
    let _ = stream.shutdown(Shutdown::Both);
}

fn connection_main(socket_key: u64, mut stream: TcpStream, authority: Arc<ConnectionAuthority>) {
    let result = connection_session(&mut stream, socket_key, &authority);
    if let Err(error_code) = result {
        tracing::debug!(
            target: "quantick::control",
            event_code = "CONTROL_CONNECTION_CLOSED",
            error_code,
            "local control connection closed"
        );
    }
    let _ = stream.shutdown(Shutdown::Both);
    let _ = authority
        .commands
        .try_send(GatewayCommand::Finished { socket_key });
}

fn connection_session(
    stream: &mut TcpStream,
    socket_key: u64,
    authority: &Arc<ConnectionAuthority>,
) -> Result<(), &'static str> {
    stream
        .set_read_timeout(Some(authority.options.handshake_timeout))
        .map_err(|_| codes::AUTH_FAILED)?;
    stream
        .set_write_timeout(Some(authority.options.handshake_timeout))
        .map_err(|_| codes::AUTH_FAILED)?;
    let handshake_codec = BoundedCodec::handshake();
    let handshake = match handshake_codec.read_handshake_request(stream) {
        Ok(request) => request,
        Err(_) => {
            send_handshake_rejection(
                stream,
                &handshake_codec,
                known_error(codes::INVALID_REQUEST, "invalid handshake frame", false),
            );
            return Err(codes::INVALID_REQUEST);
        }
    };
    let connection_id = ConnectionId::from_bytes(
        random_bytes::<CONTROL_RUNTIME_ID_BYTES>().map_err(|_| codes::AUTH_FAILED)?,
    );
    let principal_id = PrincipalId::from_bytes(
        random_bytes::<CONTROL_RUNTIME_ID_BYTES>().map_err(|_| codes::AUTH_FAILED)?,
    );
    let grant = HandshakeGrant {
        protocol_versions: ProtocolVersionRange::new(
            CURRENT_PROTOCOL_VERSION,
            CURRENT_PROTOCOL_VERSION,
        )
        .expect("current protocol range is valid"),
        instance_id: authority.identity.instance_id.clone(),
        process_nonce: authority.identity.process_nonce.clone(),
        bearer_token: authority.bearer_token.clone(),
        connection_id: connection_id.clone(),
        principal_id,
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        application_commit: option_env!("QUANTICK_GIT_COMMIT")
            .unwrap_or("unknown")
            .to_owned(),
        profile_ceiling: ProfileId::new(OBSERVER_PROFILE_ID)
            .expect("static observer profile is valid"),
        granted_scopes: authority.granted_scopes.clone(),
        // Advertise the timeout this gateway actually applies, so a client's
        // own patience is derived from the truth rather than the default.
        limits: ProtocolLimits {
            request_timeout_ms: u64::try_from(authority.options.request_timeout.as_millis())
                .unwrap_or(CONTROL_REQUEST_TIMEOUT_MS),
            ..ProtocolLimits::default()
        },
    };
    let accepted = match accept_handshake(&handshake, &grant, authority.contract.registry()) {
        Ok(response) => response,
        Err(error) => {
            let code = match error.code.as_str() {
                codes::AUTH_FAILED => codes::AUTH_FAILED,
                codes::VERSION_UNSUPPORTED => codes::VERSION_UNSUPPORTED,
                codes::INVALID_REQUEST => codes::INVALID_REQUEST,
                _ => codes::PERMISSION_DENIED,
            };
            send_handshake_rejection(stream, &handshake_codec, error);
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_AUTHENTICATION_FAILED",
                error_code = code,
                "local control authentication failed"
            );
            return Err(code);
        }
    };
    // Admission before acceptance: a client told "accepted" and then dropped
    // for a saturated channel would learn of it only on its first request,
    // as `control.instance_gone`. Refused here, it hears backpressure.
    let connected_at_unix_ms = metrics::wall_clock_ms();
    let client = ConnectedClient {
        connection_id: connection_id.clone(),
        client_name: handshake.client_name.clone(),
        connected_at_unix_ms,
        requested_profile: handshake.requested_profile.clone(),
        effective_profile: accepted.effective_profile.clone(),
        effective_scopes: accepted.effective_scopes.clone(),
        last_request_at_unix_ms: None,
    };
    let saturated = || {
        known_error(
            codes::BACKPRESSURE,
            "local gateway is saturated; retry shortly",
            true,
        )
    };
    if authority
        .commands
        .try_send(GatewayCommand::Identified {
            socket_key,
            connection_id: connection_id.clone(),
        })
        .is_err()
    {
        send_handshake_rejection(stream, &handshake_codec, saturated());
        return Err(codes::BACKPRESSURE);
    }
    if authority.statuses.len() >= activity_status_high_watermark(authority.options.max_connections)
        || authority
            .statuses
            .try_send(ConnectionStatus::Connected(client))
            .is_err()
    {
        send_handshake_rejection(stream, &handshake_codec, saturated());
        return Err(codes::BACKPRESSURE);
    }
    let frame = handshake_codec
        .encode(
            FrameRole::Response,
            &HandshakeReply::Accepted(accepted.clone()),
        )
        .map_err(|_| codes::AUTH_FAILED)?;
    if stream.write_all(&frame).is_err() {
        // The application already heard "connected": tell it the truth.
        let _ = authority
            .statuses
            .try_send(ConnectionStatus::Disconnected(connection_id.clone()));
        (authority.wake)();
        return Err(codes::INSTANCE_GONE);
    }
    // One request timeout bounds how long a frame may take to arrive once it
    // has started; a timeout with nothing received is an idle client and is
    // not an error (see the read loop below).
    stream
        .set_read_timeout(Some(authority.options.request_timeout))
        .map_err(|_| codes::INSTANCE_GONE)?;
    stream
        .set_write_timeout(Some(authority.options.request_timeout))
        .map_err(|_| codes::INSTANCE_GONE)?;

    let writer_stream = stream.try_clone().map_err(|_| codes::INSTANCE_GONE)?;
    let writer = Arc::new(Mutex::new(writer_stream));
    let codec = BoundedCodec::default();
    let in_flight = Arc::new(AtomicUsize::new(0));
    // Request IDs in flight on this connection (contract §5.2: a duplicate is
    // rejected while the first is still executing). Only requests that leave
    // for the application thread are in flight from the reader's point of
    // view; a worker-side read is answered before the next frame is read.
    let in_flight_ids: Arc<Mutex<BTreeSet<quantick_control::id::RequestId>>> =
        Arc::new(Mutex::new(BTreeSet::new()));
    (authority.wake)();
    tracing::info!(
        target: "quantick::control",
        event_code = "CONTROL_CLIENT_CONNECTED",
        connection_id = %connection_id,
        client_name = %handshake.client_name,
        effective_profile = %accepted.effective_profile,
        "authenticated local control client connected"
    );

    let mut rate_limiter = ClientRateLimiter::new();
    loop {
        let request = match codec.read_request(stream) {
            Ok(request) => request,
            // Nothing arrived within one request timeout: an idle client, not
            // a stalled frame. The connection stays unless the gateway is
            // going away.
            Err(CodecError::IdleTimeout) => {
                if authority.cancellation.load(Ordering::Acquire) {
                    break;
                }
                continue;
            }
            // A frame that started and never finished inside the timeout, a
            // malformed frame, or a closed socket all end the connection: a
            // half-written frame must not hold a connection thread open.
            Err(_) => break,
        };
        if !rate_limiter.allow(Instant::now()) {
            send_response(
                &writer,
                &codec,
                failure_response(
                    &request,
                    known_error(
                        codes::BACKPRESSURE,
                        "client request rate limit is exhausted",
                        true,
                    ),
                ),
            );
            tracing::warn!(
                target: "quantick::control",
                event_code = "CONTROL_CLIENT_RATE_LIMITED",
                connection_id = %connection_id,
                "local control client exceeded its request rate"
            );
            continue;
        }
        if in_flight_ids
            .lock()
            .map(|ids| ids.contains(&request.request_id))
            .unwrap_or(true)
        {
            send_response(
                &writer,
                &codec,
                failure_response(
                    &request,
                    known_error(
                        codes::INVALID_REQUEST,
                        "request_id is already in flight on this connection",
                        false,
                    ),
                ),
            );
            continue;
        }
        if authority.statuses.len()
            < activity_status_high_watermark(authority.options.max_connections)
            && authority
                .statuses
                .try_send(ConnectionStatus::Requested {
                    connection_id: connection_id.clone(),
                    at_unix_ms: metrics::wall_clock_ms(),
                })
                .is_ok()
        {
            (authority.wake)();
        }
        if request.instance_id != authority.identity.instance_id {
            send_response(
                &writer,
                &codec,
                failure_response(
                    &request,
                    known_error(
                        codes::INSTANCE_GONE,
                        "request names a different running instance",
                        false,
                    ),
                ),
            );
            continue;
        }
        if request.protocol_version != accepted.protocol_version {
            send_response(
                &writer,
                &codec,
                failure_response(
                    &request,
                    known_error(
                        codes::VERSION_UNSUPPORTED,
                        "request protocol version differs from the negotiated version",
                        false,
                    ),
                ),
            );
            continue;
        }
        let prepared = match authority
            .contract
            .prepare(request.clone(), &accepted.effective_scopes)
        {
            Ok(prepared) => prepared,
            Err(error) => {
                send_response(&writer, &codec, failure_response(&request, error));
                continue;
            }
        };
        dispatch_prepared(
            prepared,
            &connection_id,
            &accepted,
            &codec,
            &writer,
            &in_flight,
            &in_flight_ids,
            authority,
        );
    }

    if authority
        .statuses
        .try_send(ConnectionStatus::Disconnected(connection_id.clone()))
        .is_err()
    {
        tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_CONNECTION_STATUS_DROPPED",
            connection_id = %connection_id,
            "reserved connection status capacity was unexpectedly exhausted"
        );
    }
    (authority.wake)();
    tracing::info!(
        target: "quantick::control",
        event_code = "CONTROL_CLIENT_DISCONNECTED",
        connection_id = %connection_id,
        "local control client disconnected"
    );
    Ok(())
}

fn activity_status_high_watermark(max_connections: usize) -> usize {
    GATEWAY_STATUS_CAPACITY.saturating_sub(
        max_connections.saturating_mul(GATEWAY_CRITICAL_STATUS_SLOTS_PER_CONNECTION),
    )
}

#[allow(clippy::too_many_arguments)]
fn dispatch_prepared(
    prepared: PreparedRequest,
    connection_id: &ConnectionId,
    handshake: &quantick_control::handshake::HandshakeResponse,
    codec: &BoundedCodec,
    writer: &Arc<Mutex<TcpStream>>,
    in_flight: &Arc<AtomicUsize>,
    in_flight_ids: &Arc<Mutex<BTreeSet<quantick_control::id::RequestId>>>,
    authority: &Arc<ConnectionAuthority>,
) {
    if let PreparedDispatch::Parked(wait) = &prepared.dispatch {
        let wait = wait.clone();
        dispatch_parked_wait(
            prepared,
            wait,
            connection_id,
            handshake,
            codec,
            writer,
            in_flight,
            in_flight_ids,
            authority,
        );
        return;
    }
    if !try_reserve_in_flight(
        &authority.global_in_flight,
        CONTROL_MAX_BUFFERED_RESPONSE_SLOTS,
    ) {
        send_response(
            writer,
            codec,
            failure_response(
                &prepared.envelope,
                known_error(
                    codes::BACKPRESSURE,
                    "global buffered response capacity is full",
                    true,
                ),
            ),
        );
        return;
    }
    if let Some(result) = prepared.dispatch.execute_worker(
        &authority.contract,
        &authority.identity.instance_id,
        &handshake.effective_scopes,
        &handshake.effective_limits,
    ) {
        let response = serialize_worker_result(&authority.contract, &prepared.envelope, result);
        send_response(writer, codec, response);
        authority.global_in_flight.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    if !try_reserve_in_flight(in_flight, authority.options.max_in_flight_per_connection) {
        authority.global_in_flight.fetch_sub(1, Ordering::AcqRel);
        send_response(
            writer,
            codec,
            failure_response(
                &prepared.envelope,
                known_error(
                    codes::BACKPRESSURE,
                    "connection in-flight request capacity is full",
                    true,
                ),
            ),
        );
        return;
    }

    let envelope = prepared.envelope.clone();
    if let Ok(mut ids) = in_flight_ids.lock() {
        ids.insert(envelope.request_id.clone());
    }
    let (response_tx, response_rx) = bounded(1);
    let deadline = Instant::now() + authority.options.request_timeout;
    let response_writer = Arc::clone(writer);
    let response_codec = codec.clone();
    let response_in_flight = Arc::clone(in_flight);
    let response_in_flight_ids = Arc::clone(in_flight_ids);
    let response_global_in_flight = Arc::clone(&authority.global_in_flight);
    let contract = Arc::clone(&authority.contract);
    let wait_envelope = envelope.clone();
    let spawn = thread::Builder::new()
        .name(format!("quantick-control-response-{}", envelope.request_id))
        .spawn(move || {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let response = match response_rx.recv_timeout(remaining) {
                Ok(result) => serialize_ui_result(&contract, &wait_envelope, result),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => failure_response(
                    &wait_envelope,
                    known_error(
                        codes::TIMEOUT,
                        "request did not complete before its deadline",
                        true,
                    ),
                ),
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => failure_response(
                    &wait_envelope,
                    known_error(
                        codes::INSTANCE_GONE,
                        "application request dispatcher is unavailable",
                        true,
                    ),
                ),
            };
            send_response(&response_writer, &response_codec, response);
            if let Ok(mut ids) = response_in_flight_ids.lock() {
                ids.remove(&wait_envelope.request_id);
            }
            response_in_flight.fetch_sub(1, Ordering::AcqRel);
            response_global_in_flight.fetch_sub(1, Ordering::AcqRel);
        });
    if spawn.is_err() {
        if let Ok(mut ids) = in_flight_ids.lock() {
            ids.remove(&envelope.request_id);
        }
        in_flight.fetch_sub(1, Ordering::AcqRel);
        authority.global_in_flight.fetch_sub(1, Ordering::AcqRel);
        send_response(
            writer,
            codec,
            failure_response(
                &envelope,
                known_error(
                    codes::BACKPRESSURE,
                    "response worker could not be created",
                    true,
                ),
            ),
        );
        return;
    }

    let ui_request = UiRequest {
        prepared,
        connection_id: connection_id.clone(),
        grant_generation: authority.grant_generation,
        deadline,
        response: response_tx,
    };
    match authority.requests.try_send(ui_request) {
        Ok(()) => {
            (authority.wake)();
        }
        Err(TrySendError::Full(request)) => {
            let _ = request.response.try_send(Err(known_error(
                codes::BACKPRESSURE,
                "application request queue is full",
                true,
            )));
        }
        Err(TrySendError::Disconnected(request)) => {
            let _ = request.response.try_send(Err(known_error(
                codes::INSTANCE_GONE,
                "application request dispatcher is unavailable",
                true,
            )));
        }
    }
}

/// `events.wait`: resolve the position against the journal's published
/// bounds, answer at once if it is already behind, otherwise park on the
/// waiter manager — holding a parked-waiter slot and nothing else — and run
/// the bounded read through the ordinary UI path when woken or timed out.
#[allow(clippy::too_many_arguments)]
fn dispatch_parked_wait(
    prepared: PreparedRequest,
    wait: ParkedWait,
    connection_id: &ConnectionId,
    handshake: &quantick_control::handshake::HandshakeResponse,
    codec: &BoundedCodec,
    writer: &Arc<Mutex<TcpStream>>,
    in_flight: &Arc<AtomicUsize>,
    in_flight_ids: &Arc<Mutex<BTreeSet<quantick_control::id::RequestId>>>,
    authority: &Arc<ConnectionAuthority>,
) {
    let instance_id = authority.identity.instance_id.clone();
    let position = match resolve_event_read(
        &instance_id,
        wait.input.cursor.as_ref(),
        wait.input.start,
        authority.journal_signal.bounds(),
    ) {
        Ok(position) => position,
        Err(error) => {
            send_response(writer, codec, failure_response(&prepared.envelope, error));
            return;
        }
    };
    let target = position.next_sequence.get();
    let read_input = EventsReadInput {
        cursor: Some(EventCursor {
            instance_id: instance_id.clone(),
            next_sequence: WireU64::new(target),
        }),
        start: None,
        limit: wait.input.limit,
    };
    let to_read = move |timed_out: bool| PreparedRequest {
        envelope: prepared.envelope,
        required_permissions: prepared.required_permissions,
        dispatch: PreparedDispatch::Ui(Box::new(EventsReadInvocation {
            input: read_input,
            timed_out,
        })),
    };
    if authority.journal_signal.next_sequence() > target {
        // Already behind the journal: no parking, just the read.
        dispatch_prepared(
            to_read(false),
            connection_id,
            handshake,
            codec,
            writer,
            in_flight,
            in_flight_ids,
            authority,
        );
        return;
    }
    if !try_reserve_in_flight(&authority.parked_waiters, CONTROL_MAX_PARKED_WAITERS) {
        let request = to_read(false);
        send_response(
            writer,
            codec,
            failure_response(
                &request.envelope,
                known_error(codes::BACKPRESSURE, "parked waiter capacity is full", true),
            ),
        );
        return;
    }
    let (wake_tx, wake_rx) = bounded(1);
    let deadline = Instant::now() + Duration::from_millis(wait.input.timeout_ms);
    if authority
        .park
        .try_send(ParkedWaiter {
            target_sequence: target,
            deadline,
            wake: wake_tx,
        })
        .is_err()
    {
        authority.parked_waiters.fetch_sub(1, Ordering::AcqRel);
        let request = to_read(false);
        send_response(
            writer,
            codec,
            failure_response(
                &request.envelope,
                known_error(codes::BACKPRESSURE, "parked waiter capacity is full", true),
            ),
        );
        return;
    }
    let authority = Arc::clone(authority);
    let connection_id = connection_id.clone();
    let handshake = handshake.clone();
    let codec = codec.clone();
    let writer = Arc::clone(writer);
    let in_flight = Arc::clone(in_flight);
    let in_flight_ids = Arc::clone(in_flight_ids);
    let spawned = thread::Builder::new()
        .name("quantick-control-wait".to_owned())
        .spawn(move || {
            let reason = wake_rx.recv().unwrap_or(WakeReason::Shutdown);
            authority.parked_waiters.fetch_sub(1, Ordering::AcqRel);
            match reason {
                WakeReason::Shutdown => {
                    let request = to_read(false);
                    send_response(
                        &writer,
                        &codec,
                        failure_response(
                            &request.envelope,
                            known_error(
                                codes::INSTANCE_GONE,
                                "local access was disabled while the wait was parked",
                                true,
                            ),
                        ),
                    );
                }
                WakeReason::Woken | WakeReason::TimedOut => dispatch_prepared(
                    to_read(reason == WakeReason::TimedOut),
                    &connection_id,
                    &handshake,
                    &codec,
                    &writer,
                    &in_flight,
                    &in_flight_ids,
                    &authority,
                ),
            }
        });
    if spawned.is_err() {
        // The waiter is registered and will be woken into nothing; the slot
        // is released by the manager's send failing. Tell the client.
        tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_WAIT_THREAD_FAILED",
            "could not create a parked-wait thread"
        );
    }
}

fn try_reserve_in_flight(counter: &AtomicUsize, limit: usize) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
            (current < limit).then_some(current + 1)
        })
        .is_ok()
}

fn serialize_worker_result(
    contract: &ObserverContract,
    request: &RequestEnvelope,
    result: Result<serde_json::Value, ControlError>,
) -> ResponseEnvelope {
    match result {
        Ok(result) => validated_success(contract, request, None, Vec::new(), result),
        Err(error) => failure_response(request, error),
    }
}

fn serialize_ui_result(
    contract: &ObserverContract,
    request: &RequestEnvelope,
    execution: Result<UiReadExecution, ControlError>,
) -> ResponseEnvelope {
    match execution {
        Err(error) => failure_response(request, error),
        Ok(execution) => match execution.into_serialized() {
            Ok(serialized) => validated_success(
                contract,
                request,
                serialized.capture_revision,
                serialized.module_revisions,
                serialized.result,
            ),
            Err(_) => failure_response(
                request,
                known_error(
                    codes::CAPABILITY_UNAVAILABLE,
                    "observer result serialization failed",
                    false,
                ),
            ),
        },
    }
}

fn validated_success(
    contract: &ObserverContract,
    request: &RequestEnvelope,
    capture_revision: Option<WireU64>,
    module_revisions: Vec<ModuleRevision>,
    result: serde_json::Value,
) -> ResponseEnvelope {
    let valid =
        contract.validate_output(&request.capability_id, request.capability_version, &result);
    if !valid {
        return failure_response(
            request,
            known_error(
                codes::CAPABILITY_UNAVAILABLE,
                "observer handler returned data outside its declared schema",
                false,
            ),
        );
    }
    ResponseEnvelope {
        protocol_version: request.protocol_version,
        request_id: request.request_id.clone(),
        instance_id: request.instance_id.clone(),
        capture_revision,
        module_revisions,
        outcome: ResponseOutcome::Success { result },
        warnings: Vec::new(),
    }
}

fn failure_response(request: &RequestEnvelope, error: ControlError) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: request.protocol_version,
        request_id: request.request_id.clone(),
        instance_id: request.instance_id.clone(),
        capture_revision: None,
        module_revisions: Vec::new(),
        outcome: ResponseOutcome::Failure { error },
        warnings: Vec::new(),
    }
}

fn send_handshake_rejection(stream: &mut TcpStream, codec: &BoundedCodec, error: ControlError) {
    if let Ok(frame) = codec.encode(FrameRole::Response, &HandshakeReply::Rejected { error }) {
        let _ = stream.write_all(&frame);
    }
    let _ = stream.shutdown(Shutdown::Both);
}

fn send_response(
    writer: &Arc<Mutex<TcpStream>>,
    codec: &BoundedCodec,
    mut response: ResponseEnvelope,
) {
    let frame = match codec.encode(FrameRole::Response, &response) {
        Ok(frame) => frame,
        Err(error) => {
            response.capture_revision = None;
            response.module_revisions.clear();
            response.outcome = ResponseOutcome::Failure {
                error: match error {
                    CodecError::PayloadTooLarge { .. }
                    | CodecError::StringTooLarge { .. }
                    | CodecError::JsonTooDeep { .. } => known_error(
                        codes::PAYLOAD_TOO_LARGE,
                        "response exceeds the negotiated protocol limit",
                        false,
                    ),
                    _ => known_error(
                        codes::CAPABILITY_UNAVAILABLE,
                        "response could not be encoded under the negotiated protocol rules",
                        false,
                    ),
                },
            };
            match codec.encode(FrameRole::Response, &response) {
                Ok(frame) => frame,
                Err(_) => return,
            }
        }
    };
    let mut stream = writer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = stream.write_all(&frame);
}

fn known_error(code: &str, message: &str, retryable: bool) -> ControlError {
    ControlError::new(
        quantick_control::id::ErrorCode::new(code).expect("static error code is valid"),
        message,
        retryable,
    )
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_directory(name: &str) -> PathBuf {
        static NEXT_DIRECTORY: AtomicUsize = AtomicUsize::new(1);
        std::env::temp_dir().join(format!(
            "quantick-control-{name}-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn empty_discovery_returns_a_next_step_without_creating_or_starting_anything() {
        let directory = unique_test_directory("empty-discovery");
        assert!(!directory.exists());

        let discovery = quantick_control_local::client::discover_in(
            &directory,
            &quantick_control_local::client::ConnectOptions::observer(
                "gateway unit test",
                "0.0.0",
                BTreeSet::new(),
            ),
        )
        .unwrap();
        assert!(discovery.clients.is_empty());
        assert!(discovery.issues.is_empty());
        assert!(!discovery.next_steps.is_empty());
        assert!(!directory.exists());

        let error = discovery.select(None).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INSTANCE_GONE);
        assert!(!error.context.next_steps.is_empty());
        assert!(!directory.exists());
    }

    #[test]
    fn one_frame_never_drains_more_than_the_reviewed_request_ceiling() {
        let (sender, receiver) = bounded(CONTROL_UI_MAX_REQUESTS_PER_FRAME + 2);
        for value in 0..(CONTROL_UI_MAX_REQUESTS_PER_FRAME + 2) {
            sender.send(value).unwrap();
        }
        let mut handled = Vec::new();
        // A start in the future reads as zero elapsed: the budget cannot
        // interfere, so this proves the count ceiling alone, whatever the
        // scheduler does to the test thread.
        let observation = drain_bounded_since(
            &receiver,
            Instant::now() + Duration::from_secs(60),
            |value| handled.push(value),
        );
        assert_eq!(handled.len(), CONTROL_UI_MAX_REQUESTS_PER_FRAME);
        assert_eq!(receiver.len(), 2);
        assert!(observation.queue_has_more);
        assert!(
            !observation.budget_exceeded,
            "stopping on the count ceiling is not a budget stop"
        );
    }

    #[test]
    fn a_capture_that_exhausts_the_budget_ends_the_frame_drain() {
        // The count ceiling is the deterministic guard; the elapsed-time
        // budget is the authoritative one (plan §10.2). Prove the latter on
        // its own: captures that each cost the whole budget must not run two
        // to a frame, although the count ceiling alone would allow four. A
        // scheduler that preempts the thread for longer than the budget
        // before the first check makes the drain run nothing, which proves
        // nothing either way — try again, a few times.
        let budget = Duration::from_micros(CONTROL_UI_BUDGET_US);
        let mut last = None;
        for _ in 0..5 {
            let (sender, receiver) = bounded(4);
            for value in 0..4 {
                sender.send(value).unwrap();
            }
            let mut handled = 0usize;
            let observation = drain_bounded_since(&receiver, Instant::now(), |_| {
                handled += 1;
                let until = Instant::now() + budget;
                while Instant::now() < until {
                    std::hint::spin_loop();
                }
            });
            last = Some((handled, observation, receiver.len()));
            if handled == 1 {
                break;
            }
        }
        let (handled, observation, remaining) = last.expect("at least one attempt ran");
        assert_eq!(
            handled, 1,
            "the second capture must wait for the next frame"
        );
        assert_eq!(observation.processed, 1);
        assert!(observation.budget_exceeded);
        assert!(observation.queue_has_more);
        assert_eq!(remaining, 3);
    }

    #[test]
    fn exit_with_access_disabled_costs_nothing() {
        // The default state: no gateway thread exists, none will report, and
        // the application's exit must not wait for one.
        let mut access = ControlAccess::new();
        let started = Instant::now();
        access.shutdown_for_exit();
        assert!(
            started.elapsed() < Duration::from_millis(EXIT_SHUTDOWN_TIMEOUT_MS / 4),
            "a disabled gateway returned at once, not after the exit timeout"
        );
    }

    #[test]
    fn client_rate_limiter_has_a_bounded_burst_and_refills() {
        let started = Instant::now();
        let mut limiter = ClientRateLimiter {
            available_token_nanos: u128::from(CONTROL_CLIENT_BURST)
                * ClientRateLimiter::ONE_TOKEN_NANOS,
            last_refill: started,
        };
        for _ in 0..CONTROL_CLIENT_BURST {
            assert!(limiter.allow(started));
        }
        assert!(!limiter.allow(started));
        assert!(limiter.allow(started + Duration::from_secs(1)));
    }

    #[test]
    fn global_response_slots_enforce_the_reviewed_buffer_bound() {
        assert_eq!(
            CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
                * quantick_control::limits::CONTROL_MAX_RESPONSE_BYTES,
            quantick_control::limits::CONTROL_MAX_BUFFERED_RESPONSE_BYTES
        );
        let in_flight = AtomicUsize::new(0);
        for _ in 0..CONTROL_MAX_BUFFERED_RESPONSE_SLOTS {
            assert!(try_reserve_in_flight(
                &in_flight,
                CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
            ));
        }
        assert!(!try_reserve_in_flight(
            &in_flight,
            CONTROL_MAX_BUFFERED_RESPONSE_SLOTS
        ));
    }

    #[test]
    fn gateway_options_cannot_exceed_reviewed_hard_limits() {
        assert!(GatewayOptions::default().validate().is_ok());

        let mut options = GatewayOptions {
            request_queue_capacity: CONTROL_REQUEST_QUEUE_CAPACITY + 1,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            request_queue_capacity: 0,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_connections: CONTROL_MAX_CONNECTIONS + 1,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_connections: 0,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_in_flight_per_connection: CONTROL_MAX_IN_FLIGHT_PER_CONNECTION + 1,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            max_in_flight_per_connection: 0,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            handshake_timeout: Duration::from_millis(CONTROL_HANDSHAKE_TIMEOUT_MS + 1),
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            handshake_timeout: Duration::ZERO,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            request_timeout: Duration::from_millis(CONTROL_REQUEST_TIMEOUT_MS + 1),
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());

        options = GatewayOptions {
            request_timeout: Duration::ZERO,
            ..GatewayOptions::default()
        };
        assert!(options.validate().is_err());
    }

    #[test]
    fn activity_updates_leave_capacity_for_connection_lifecycle_events() {
        let high_watermark = activity_status_high_watermark(CONTROL_MAX_CONNECTIONS);
        assert_eq!(
            GATEWAY_STATUS_CAPACITY - high_watermark,
            CONTROL_MAX_CONNECTIONS * GATEWAY_CRITICAL_STATUS_SLOTS_PER_CONNECTION
        );
    }
}
