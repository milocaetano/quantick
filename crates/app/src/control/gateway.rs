//! Explicitly enabled authenticated loopback gateway and UI-thread dispatcher.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
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
        CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION, CONTROL_REQUEST_QUEUE_CAPACITY,
        CONTROL_REQUEST_TIMEOUT_MS, CONTROL_RUNTIME_ID_BYTES, CONTROL_TOKEN_BYTES,
        CONTROL_UI_BUDGET_US, CONTROL_UI_MAX_REQUESTS_PER_FRAME,
    },
    wire::{
        ActorContext, ActorKind, ModuleRevision, RequestEnvelope, ResponseEnvelope,
        ResponseOutcome, WireU64,
    },
};
use serde_json::{Value, json};

use crate::{app::QuantickApp, metrics};

use super::{
    actions::{ANNOTATE_PERMISSION_ID, ANNOTATOR_PROFILE_ID, ActionRegistry, standard_actions},
    contract::{COCKPIT_PERMISSION_ID, COCKPIT_PROFILE_ID},
    contract::{
        DeferredActionResult, EventsReadInvocation, OBSERVE_PERMISSION_ID, OBSERVER_PROFILE_ID,
        ObserverContract, ParkedWait, PreparedDispatch, PreparedRequest, UiReadContext,
        UiReadExecution,
    },
    events::EventsReadInput,
    evidence::{EvidenceStore, RawScreenshot, SessionIdentity},
    feed::connection_state,
    interaction::{SelectionIdentity, selection_identity, selection_snapshot},
    journal::{EventJournal, JournalSignal, NewEvent},
    notify::NotificationLimiter,
    registry::ProjectionRegistry,
    trace::{
        ControlTrace, NoTrace, ReplayTraceFile, TRACE_VERSION, TraceEntry, TraceReplay,
        result_digest,
    },
    types::known_error,
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
/// What the annotate launch hooks call themselves. A name, never a
/// disguise: the object they place says an assistant put it there.
const HOOK_ACTOR_CLIENT_NAME: &str = "launch hook (agent)";
const UI_ACTOR_CLIENT_NAME: &str = "quantick-ui";
/// Take a mark of what is under the pointer (`attention.mark.create`).
pub(crate) const MARK_SHORTCUT: eframe::egui::KeyboardShortcut =
    eframe::egui::KeyboardShortcut::new(eframe::egui::Modifiers::CTRL, eframe::egui::Key::M);
const EXIT_SHUTDOWN_TIMEOUT_MS: u64 = 2_000;
/// Captures that may be waiting on one rasterised frame at once.
///
/// Small on purpose: each holds a response worker and its deadline, and the
/// window only ever produces one image per arming, so a queue deeper than the
/// frame budget's own request ceiling would buy nothing. A capture that
/// arrives with the queue full is answered without an image and says so.
const CONTROL_MAX_SCREENSHOT_WAITERS: usize = CONTROL_UI_MAX_REQUESTS_PER_FRAME;
/// What the window tells the trader when a picture of it leaves the process.
///
/// Not optional and not configurable: a screenshot is the one observer read
/// that copies the screen itself, and the person at it is told every time
/// (threat model O-18).
const SCREENSHOT_NOTICE: &str = "Your assistant captured a picture of this window.";
/// How long before its own deadline a waiting capture gives up on the window.
///
/// `execute_on_ui` refuses an expired request before it runs anything, so a
/// capture that waited to the last millisecond could only ever answer
/// `control.timeout`. This is the room the honest answer needs: a bundle with
/// the text, the events and the configuration it could collect, and a coded
/// gap saying the frame never arrived. Generous next to the microseconds a
/// capture costs, and small next to the five-second request timeout.
const CONTROL_SCREENSHOT_GRACE_MS: u64 = 250;

/// The identity a human action in this window is attributed with. One
/// principal and one "connection" per process, generated with the instance
/// identity; it is how the journal tells the human from a remote client.
#[derive(Clone)]
struct UiActorIdentity {
    principal_id: PrincipalId,
    connection_id: ConnectionId,
}

/// Where an action came from: the human at this window, whose action during a
/// replay is recorded in the control trace; the control trace itself replaying
/// that action, which must not be recorded again; or an authorized client on
/// the local gateway, carrying the actor the *connection* proved — never one
/// the payload claimed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ActionOrigin {
    Human,
    /// The trace speaking, carrying the operator the recorded run attributed
    /// the action to. Automation is what *acts* now — that is what a mark
    /// reports as its target source — but what the action produces belongs to
    /// whoever produced it the first time, or to nobody when that was the
    /// trader's own hand. A rerun that stamped every object "automation"
    /// would not be the session it claims to reproduce.
    TraceReplay(Box<RecordedActor>),
    Remote(Box<ActorContext>),
}

/// Who a recorded action belonged to, as its trace line says.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RecordedActor {
    pub actor_kind: ActorKind,
    pub client_name: String,
}

impl ActionOrigin {
    fn actor_kind(&self) -> ActorKind {
        match self {
            Self::Human => ActorKind::HumanUi,
            Self::TraceReplay(_) => ActorKind::Automation,
            Self::Remote(actor) => actor.actor_kind,
        }
    }

    fn is_trace_replay(&self) -> bool {
        matches!(self, Self::TraceReplay(_))
    }
}

/// A replay session whose control trace is being re-injected: the entries
/// still due, in replay-time order.
/// One recording's control trace, loaded once and walked by logical replay
/// time. Keyed by the session path: two tabs on the same recording share one
/// walk, driven by the tab that loaded it.
struct TraceReinjection {
    /// The tab whose playhead drives the walk.
    owner_tab_id: u64,
    /// Completed entries in `(replay_elapsed_ms, sequence)` order — the
    /// sidecar's at load time plus the actions this run recorded since, so
    /// an in-session restart replays exactly what a fresh process would.
    entries: Vec<TraceEntry>,
    /// The first entry not yet injected on this pass over the session.
    next_index: usize,
    /// Where the playhead was last frame; a smaller value now means it moved
    /// backwards and the walk rewinds.
    last_elapsed_ms: i64,
    /// The worker's rewind count last frame; a different value now means a
    /// restart or seek happened, even if the rerun already advanced past
    /// `last_elapsed_ms`.
    last_rewinds: u64,
    /// Sequences of the actions this run took during the current pass: they
    /// joined `entries` for the next rerun and are not injected back on the
    /// spot. Cleared by a rewind.
    executed_this_pass: Vec<u64>,
}

/// What the replay link publishes that the walk reads once per frame.
#[derive(Clone, Copy)]
struct ReplayPosition {
    elapsed_ms: i64,
    rewinds: u64,
    rewind_target_elapsed_ms: i64,
}

impl ReplayPosition {
    fn of(status: &crate::feed::replay::ReplayStatus) -> Self {
        Self {
            elapsed_ms: status.elapsed_ms(),
            rewinds: status.rewinds(),
            rewind_target_elapsed_ms: status.rewind_target_elapsed_ms(),
        }
    }
}

impl TraceReinjection {
    /// Move the entries due at the position into `due`, exactly once per
    /// pass over the session. A rewind — the worker counted a restart or a
    /// seek, or the playhead is behind last frame's sample — moves the walk
    /// back to the first entry at or after where the rerun began, so the
    /// rerun injects the same actions again.
    fn collect_due(&mut self, position: ReplayPosition, due: &mut Vec<TraceEntry>) {
        let rewound_to = if position.rewinds != self.last_rewinds {
            Some(position.rewind_target_elapsed_ms)
        } else if position.elapsed_ms < self.last_elapsed_ms {
            Some(position.elapsed_ms)
        } else {
            None
        };
        if let Some(start_elapsed_ms) = rewound_to {
            self.next_index = self
                .entries
                .partition_point(|entry| entry.replay_elapsed_ms < start_elapsed_ms);
            self.executed_this_pass.clear();
        }
        self.last_rewinds = position.rewinds;
        self.last_elapsed_ms = position.elapsed_ms;
        while let Some(entry) = self.entries.get(self.next_index)
            && entry.replay_elapsed_ms <= position.elapsed_ms
        {
            if !self.executed_this_pass.contains(&entry.sequence.get()) {
                due.push(entry.clone());
            }
            self.next_index += 1;
        }
    }

    /// An action this run just recorded to the sidecar joins the walk in
    /// replay-time order, marked as executed on this pass: the next rerun
    /// replays it, this one does not inject it back.
    fn record_this_pass(&mut self, entry: TraceEntry) {
        let key = (entry.replay_elapsed_ms, entry.sequence.get());
        let position = self
            .entries
            .partition_point(|other| (other.replay_elapsed_ms, other.sequence.get()) < key);
        if position < self.next_index {
            self.next_index += 1;
        }
        self.executed_this_pass.push(entry.sequence.get());
        self.entries.insert(position, entry);
    }
}

/// Read a session's sidecar for re-injection, naming an unreadable or an
/// unfinished trace in the log: either way that run is not a fixture.
fn load_trace_for_reinjection(session_path: &std::path::Path) -> TraceReplay {
    let loaded = match TraceReplay::load(session_path) {
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
    loaded
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

    /// What names this run of the process, for the reads that record it. The
    /// instance identifier travels on every envelope already; this is the
    /// other half — which *session* of that instance answered.
    fn session(&self) -> SessionIdentity {
        SessionIdentity {
            session_id: self.process_nonce.clone(),
            process_started_at_unix_ms: self.process_started_at_unix_ms,
        }
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
    /// The highest profile any connection of this run may hold — what the
    /// human granted in the panel, never what a client asks for.
    profile_ceiling: ProfileId,
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

/// Whether a permission belongs to the annotate tier — the `annotate` floor
/// itself or one of its scopes.
/// Whether a permission belongs to the trade tier — see the access
/// panel's read-scope filter for why it is excluded from every section.
pub(super) fn is_trade_permission(permission: &PermissionId) -> bool {
    permission.as_str() == super::trade::TRADE_PERMISSION_ID
        || permission.as_str().starts_with("trade.")
}

pub(super) fn is_annotate_permission(permission: &PermissionId) -> bool {
    permission.as_str() == ANNOTATE_PERMISSION_ID || is_annotate_scope(permission)
}

/// Any permission of the cockpit tier — the floor or one of its scopes.
pub(super) fn is_cockpit_permission(permission: &PermissionId) -> bool {
    permission.as_str() == COCKPIT_PERMISSION_ID || is_cockpit_scope(permission)
}

/// A scope *of* the cockpit tier, as opposed to the floor they stand on.
fn is_cockpit_scope(permission: &PermissionId) -> bool {
    permission.as_str().starts_with(concat!("cockpit", "."))
}

/// A scope *of* the annotate tier — the ones that actually open a capability,
/// as opposed to the `annotate` floor they all stand on.
fn is_annotate_scope(permission: &PermissionId) -> bool {
    permission.as_str().starts_with(concat!("annotate", "."))
}

/// Who a connection is, as the handshake proved it. Every action that
/// connection asks for is signed with this: the payload never names an actor.
#[derive(Clone, Debug)]
struct RemoteActor {
    principal_id: PrincipalId,
    client_name: String,
    connection_id: ConnectionId,
}

impl RemoteActor {
    /// The actor for one request of this connection.
    fn context(&self, request: &RequestEnvelope) -> ActorContext {
        ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: self.principal_id.clone(),
            client_name: self.client_name.clone(),
            connection_id: self.connection_id.clone(),
            request_id: request.request_id.clone(),
            reason: request.reason.clone(),
            requested_at_unix_ms: metrics::wall_clock_ms(),
        }
    }
}

struct UiRequest {
    prepared: PreparedRequest,
    /// Present when the request is an action: the connection's proved actor.
    actor: Option<Box<ActorContext>>,
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
    actions: Arc<ActionRegistry>,
    /// What the frame emitter last saw; `None` until access is enabled, so a
    /// disabled gateway costs the frame nothing and an enabled one records
    /// changes, not the state it found.
    semantic_baseline: Option<SemanticBaseline>,
    next_ui_request: u64,
    /// One notification budget per connected client, dropped when the client
    /// disconnects so a long session cannot accumulate them.
    notification_limits: BTreeMap<ConnectionId, NotificationLimiter>,
    /// Set for the duration of one replayed action: who the recorded run
    /// attributed it to.
    replayed_author: Option<RecordedActor>,
    next_trace_sequence: u64,
    /// The control trace of every replaying recording, keyed by session
    /// path, loaded once and walked by logical replay time; a tab without a
    /// replay costs the frame one comparison.
    trace_reinjection: BTreeMap<PathBuf, TraceReinjection>,
    /// The retained evidence bundles. The same handle the contract holds, so
    /// a worker paging a bundle and the window emptying the store are talking
    /// about one thing.
    evidence: EvidenceStore,
    /// The pixels of the last frame the window was asked to rasterise, waiting
    /// for the capture that asked for them.
    screenshot: Option<RawScreenshot>,
    /// Whether a rasterise has been asked for and not yet arrived. One at a
    /// time: several captures in the same frame all wait for the same image.
    screenshot_armed: bool,
    /// Requests that asked for an image the frame had not delivered yet. They
    /// hold their own deadline, so a window that never answers costs the
    /// client its request timeout and nothing more.
    awaiting_screenshot: VecDeque<UiRequest>,
}

impl ControlAccess {
    pub fn new() -> Self {
        let projections = super::standard_registry()
            .expect("built-in semantic projection registry must be valid");
        let actions = Arc::new(standard_actions().expect("built-in action registry must be valid"));
        let evidence = EvidenceStore::new();
        let contract = Arc::new(
            ObserverContract::new(&projections, Arc::clone(&actions), evidence.clone())
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
            notification_limits: BTreeMap::new(),
            replayed_author: None,
            next_trace_sequence: 1,
            trace_reinjection: BTreeMap::new(),
            evidence,
            screenshot: None,
            screenshot_armed: false,
            awaiting_screenshot: VecDeque::new(),
        }
    }

    /// Each frame: every recording a tab is playing with a control trace
    /// beside it re-injects the recorded actions at their logical replay
    /// time (contract §11). The trace is loaded once per recording and walked
    /// forward by the tab that loaded it; a restart or seek rewinds the walk
    /// so the rerun injects the same actions again, switching tabs neither
    /// repeats nor skips an injection, and a second tab on the same
    /// recording adds nothing. A live tab costs one comparison. Runs whether
    /// or not local access is enabled: replay determinism does not depend on
    /// a client being connected.
    pub(crate) fn service_replay_trace(&mut self, app: &mut QuantickApp) {
        // The entries that came due this frame. The Vec allocates only when
        // one did, a human gesture's worth of times per session.
        let mut due: Vec<TraceEntry> = Vec::new();
        {
            let tabs = app.control_tabs();
            if !self.trace_reinjection.is_empty() {
                self.trace_reinjection.retain(|path, _| {
                    tabs.iter().any(|tab| {
                        tab.replay
                            .as_ref()
                            .is_some_and(|link| link.session.path == *path)
                    })
                });
            }
            for tab in tabs {
                let Some(link) = tab.replay.as_ref() else {
                    continue;
                };
                let position = ReplayPosition::of(&link.status);
                let path = &link.session.path;
                match self.trace_reinjection.get_mut(path) {
                    Some(state) if state.owner_tab_id == tab.id => {
                        state.collect_due(position, &mut due);
                    }
                    // One walk per recording: the tab that loaded it drives.
                    // Another tab on the same file adopts the walk only once
                    // the owner let go of the session.
                    Some(state) => {
                        let owner_still_plays_it = tabs.iter().any(|other| {
                            other.id == state.owner_tab_id
                                && other
                                    .replay
                                    .as_ref()
                                    .is_some_and(|link| link.session.path == *path)
                        });
                        if !owner_still_plays_it {
                            state.owner_tab_id = tab.id;
                            state.collect_due(position, &mut due);
                        }
                    }
                    None => {
                        let loaded = load_trace_for_reinjection(path);
                        // Trace sequences continue where the sidecar left
                        // off, so a later run appending to the same file
                        // never reuses one.
                        self.next_trace_sequence = self
                            .next_trace_sequence
                            .max(loaded.max_sequence.saturating_add(1));
                        let mut state = TraceReinjection {
                            owner_tab_id: tab.id,
                            entries: loaded.completed,
                            next_index: 0,
                            last_elapsed_ms: i64::MIN,
                            last_rewinds: position.rewinds,
                            executed_this_pass: Vec::new(),
                        };
                        state.collect_due(position, &mut due);
                        self.trace_reinjection.insert(path.clone(), state);
                    }
                }
            }
        }
        for entry in due {
            if let Err(error) = self.invoke_local_action(
                app,
                entry.capability_id.as_str(),
                entry.capability_version,
                entry.canonical_input,
                ActionOrigin::TraceReplay(Box::new(RecordedActor {
                    actor_kind: entry.actor_kind,
                    client_name: entry.client_name.clone(),
                })),
            ) {
                tracing::warn!(
                    target: "quantick::control",
                    event_code = "CONTROL_TRACE_REPLAY_REFUSED",
                    capability = %entry.capability_id,
                    version = entry.capability_version,
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

    /// Hand the gateway a rasterised frame the way the window would.
    ///
    /// A headless `egui::Context` answers no `ViewportCommand::Screenshot`, so
    /// the correlation between an image and the scene captured beside it can
    /// only be proved by supplying the pixels here. Everything downstream —
    /// the stamping, the point-to-pixel scaling, the encoding, the digest — is
    /// the shipped path, unchanged.
    #[cfg(test)]
    pub(crate) fn publish_screenshot_for_test(
        &mut self,
        app: &mut QuantickApp,
        raw: RawScreenshot,
    ) {
        self.accept_screenshot(app, raw);
    }

    /// Whether a capture is still waiting for the window to be rasterised.
    #[cfg(test)]
    pub(crate) fn awaiting_screenshot_for_test(&self) -> usize {
        self.awaiting_screenshot.len()
    }

    /// How many bundles this instance is holding.
    #[cfg(test)]
    pub(crate) fn retained_evidence_for_test(&self) -> usize {
        self.evidence.retained()
    }

    /// Whom an object this call produces belongs to, when the call is a
    /// trace replaying a recorded action. `None` on every ordinary call: the
    /// acting actor is the author.
    pub(crate) fn recorded_author(&self) -> Option<&RecordedActor> {
        self.replayed_author.as_ref()
    }

    /// The actor a launch hook acts as: an agent, named for what it is, so
    /// nothing it places can pass for the trader's own hand and a screenshot
    /// shows exactly what a connected assistant would have produced.
    pub(crate) fn hook_agent_actor(&mut self) -> Option<ActorContext> {
        self.identity.as_ref()?;
        let mut actor = self.local_actor(ActorKind::Agent, Some("launch hook".to_owned()));
        actor.client_name = HOOK_ACTOR_CLIENT_NAME.to_owned();
        Some(actor)
    }

    /// Whether this actor may interrupt the trader once more.
    ///
    /// Budgeted per connection, not per capability: three toasts and three
    /// popups from one client are six interruptions to the person reading the
    /// chart. The trader's own gestures never pass through here.
    pub(crate) fn allow_notification(
        &mut self,
        actor: &ActorContext,
    ) -> Result<(), std::time::Duration> {
        let limiter = self
            .notification_limits
            .entry(actor.connection_id.clone())
            .or_insert_with(NotificationLimiter::new);
        if limiter.allow(Instant::now()) {
            return Ok(());
        }
        Err(limiter.retry_after())
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

    /// Whether this window is holding a rasterised frame a read could use.
    pub(crate) fn has_screenshot(&self) -> bool {
        self.screenshot.is_some()
    }

    /// The registered snapshot scopes the configured grant already reaches.
    pub(crate) fn readable_scopes(&self) -> Vec<quantick_control::id::SnapshotScopeId> {
        self.contract.readable_scopes(&self.configured_scopes)
    }

    /// Whether the configured grant permits rasterising the window.
    ///
    /// Asked *before* anything arms a rasterise, because taking the picture is
    /// what raises the notice: a caller that will be refused the scope one
    /// step later must not first tell the trader their window was captured.
    /// The indicator only means something if it is never wrong.
    pub(crate) fn grants_screenshot(&self) -> bool {
        self.configured_scopes
            .iter()
            .any(|permission| permission.as_str() == super::evidence::SCREENSHOT_PERMISSION_ID)
    }

    /// Ask the window to rasterise itself for the next read that wants it,
    /// and take the frame if one has already arrived.
    ///
    /// The same arming and the same harvest a deferred remote capture gets.
    /// Both halves, because a local read runs whether or not the gateway is
    /// enabled while the frame service that normally harvests runs only when
    /// it is — so a hook that armed but could not harvest would wait out its
    /// whole budget for a frame sitting in the input queue.
    pub(crate) fn service_screenshot(
        &mut self,
        app: &mut QuantickApp,
        ctx: &eframe::egui::Context,
    ) {
        self.harvest_screenshot(app, ctx);
        if self.screenshot.is_none() {
            self.arm_screenshot(ctx);
        }
    }

    /// One request on the application thread: the authority checks the
    /// gateway made when it arrived, re-made at the moment it runs, and then
    /// the read or the action itself.
    fn execute_on_ui(
        &mut self,
        app: &mut QuantickApp,
        current_generation: u64,
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
            || self.revoked_connections.contains(&request.connection_id)
        {
            return Err(known_error(
                codes::PERMISSION_DENIED,
                "connection authority was revoked before dispatch",
                false,
            ));
        }
        let instance_id = match &self.identity {
            Some(identity) => identity.instance_id.clone(),
            None => {
                return Err(known_error(
                    codes::INSTANCE_GONE,
                    "the running instance has no control identity",
                    false,
                ));
            }
        };
        if request.prepared.envelope.instance_id != instance_id {
            return Err(known_error(
                codes::INSTANCE_GONE,
                "request names a different running instance",
                false,
            ));
        }
        if !request
            .prepared
            .required_permissions
            .is_subset(&self.configured_scopes)
        {
            return Err(known_error(
                codes::SCOPE_DENIED,
                "a required scope was removed before dispatch",
                false,
            ));
        }

        if let PreparedDispatch::Action(action) = &request.prepared.dispatch {
            let Some(actor) = request.actor.clone() else {
                return Err(known_error(
                    codes::CAPABILITY_UNAVAILABLE,
                    "an action reached the application thread without the connection's actor",
                    false,
                ));
            };
            let action = action.clone();
            let result = self.invoke_local_action(
                app,
                action.capability_id.as_str(),
                action.capability_version,
                action.input,
                ActionOrigin::Remote(actor),
            )?;
            return Ok(Box::new(DeferredActionResult(result)) as UiReadExecution);
        }

        let session = self
            .identity
            .as_ref()
            .expect("the instance identity was read above")
            .session();
        request.prepared.dispatch.execute_ui(UiReadContext {
            projections: &mut self.projections,
            journal: &self.journal,
            app,
            instance_id: &instance_id,
            session: &session,
            evidence: &self.evidence,
            screenshot: &mut self.screenshot,
        })
    }

    pub fn begin_frame(&mut self, app: &mut QuantickApp, ctx: &eframe::egui::Context) {
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
        // Before the drain, and deliberately: an image that arrived this frame
        // is of the frame just painted, and the projections about to be taken
        // still describe that frame. Harvesting after the drain would pair a
        // scene with a picture of the screen before it.
        self.harvest_screenshot(app, ctx);
        // What the waiters spend, the drain does not get to spend again. They
        // are the same work against the same frame, so the ceiling has to be
        // one number: four captures served here plus four admitted below would
        // be eight projection passes in a frame documented to admit four.
        let already_served = self.serve_awaiting_screenshot(app, generation, ctx, frame_started);

        // The drain owns `self` for the whole pass: an action runs through the
        // same `invoke_local_action` the hotkey uses, which needs the journal
        // and the trace mutably, so no field can stay borrowed across it.
        let drain = drain_bounded_since(&requests, frame_started, already_served, |request| {
            if self.defer_for_screenshot(&request, ctx) {
                self.awaiting_screenshot.push_back(request);
                return;
            }
            let result = self.execute_on_ui(app, generation, &request);
            let _ = request.response.try_send(result);
        });
        self.last_drain = drain;
        // A rasterised frame is worth exactly one frame. Whatever is still
        // holding it here was not claimed by any capture this pass — the
        // capture that asked timed out, or the hook gave up — and a picture
        // kept past its frame is a picture of some other chart, which is the
        // one thing the capture revision promises it is not. Dropping it also
        // returns the framebuffer instead of parking tens of megabytes for the
        // rest of the session.
        self.screenshot = None;
        if statuses_have_more
            || self.last_drain.queue_has_more
            || !self.awaiting_screenshot.is_empty()
        {
            ctx.request_repaint();
        }
    }

    /// Take the rasterised frame the window was asked for, if it has arrived.
    ///
    /// Costs a frame nothing until something arms it: with no capture waiting
    /// the input scan does not run at all. When one does arrive the trader is
    /// told, because a picture of their window leaving the process is not
    /// something that should happen quietly (threat model O-18).
    fn harvest_screenshot(&mut self, app: &mut QuantickApp, ctx: &eframe::egui::Context) {
        if !self.screenshot_armed {
            return;
        }
        // Inside the input lock: clone the image's handle and nothing else.
        // The rows are converted by the closure below, on the response worker
        // — a 4K framebuffer is eight million pixels, and paying for that here
        // would blow a 250 microsecond frame budget by two orders of
        // magnitude every time an agent asks for a picture.
        let taken = ctx.input(|input| {
            let pixels_per_point = input.pixels_per_point();
            input.events.iter().find_map(|event| match event {
                eframe::egui::Event::Screenshot { image, .. } => {
                    let image = Arc::clone(image);
                    Some(RawScreenshot {
                        width_px: u32::try_from(image.size[0]).unwrap_or(0),
                        height_px: u32::try_from(image.size[1]).unwrap_or(0),
                        pixels_per_point,
                        rgba: super::evidence::ScreenshotPixels::new(move || {
                            image
                                .pixels
                                .iter()
                                // Unmultiplied: the toolkit stores colours
                                // premultiplied by alpha and a PNG's are not,
                                // so a translucent surface would darken on the
                                // way out if the raw bytes were copied across.
                                .flat_map(eframe::egui::Color32::to_srgba_unmultiplied)
                                .collect()
                        }),
                    })
                }
                _ => None,
            })
        });
        if let Some(raw) = taken {
            self.accept_screenshot(app, raw);
        }
    }

    /// Take one rasterised frame, and tell the person at the window.
    ///
    /// The single door for pixels entering the control plane, so the notice
    /// cannot be bypassed by whatever hands them over — the window's own
    /// screenshot event today, a test's fixture in the same breath.
    fn accept_screenshot(&mut self, app: &mut QuantickApp, raw: RawScreenshot) {
        self.screenshot_armed = false;
        self.screenshot = Some(raw);
        app.show_agent_toast(SCREENSHOT_NOTICE.to_owned());
    }

    /// Run the captures that were waiting for an image, or give up on them,
    /// and report how much of the frame's request budget they spent.
    ///
    /// Returns before touching anything when none is waiting, which is every
    /// frame of every session where no client asked for a picture.
    fn serve_awaiting_screenshot(
        &mut self,
        app: &mut QuantickApp,
        generation: u64,
        ctx: &eframe::egui::Context,
        frame_started: Instant,
    ) -> usize {
        if self.awaiting_screenshot.is_empty() {
            return 0;
        }
        let now = Instant::now();
        let mut served = 0usize;
        let mut waiting = std::mem::take(&mut self.awaiting_screenshot);
        while let Some(request) = waiting.pop_front() {
            // Revocation applies to work already in flight, and a parked
            // capture is in flight. Answered here rather than left to park,
            // because parking is what keeps asking the window to rasterise —
            // and a revoked connection must not be able to make the trader's
            // window flash a capture notice for a request that will be refused
            // anyway.
            if request.grant_generation != generation
                || self.revoked_connections.contains(&request.connection_id)
            {
                let _ = request.response.try_send(Err(known_error(
                    codes::PERMISSION_DENIED,
                    "connection authority was revoked while the capture waited for a frame",
                    false,
                )));
                continue;
            }
            // These captures spend the same frame budget the drain does, and
            // they run before it. Left uncounted, four waiters plus the
            // drain's own four would put eight projection passes in one frame
            // against a documented ceiling of four, and the drain would find
            // its budget already gone. A waiter this frame cannot serve stays
            // queued and is served by the next one.
            if served >= CONTROL_UI_MAX_REQUESTS_PER_FRAME
                || elapsed_us_since(frame_started) > CONTROL_UI_BUDGET_US
            {
                self.awaiting_screenshot.push_back(request);
                self.awaiting_screenshot.extend(waiting);
                ctx.request_repaint();
                return served;
            }
            // Given up on *before* the deadline, not at it. `execute_on_ui`
            // refuses an expired request with `control.timeout` as its very
            // first act, so waiting to the last moment would answer a window
            // that never presented with a bare timeout — no bundle, no
            // `screenshot: frame_not_delivered`, and none of the text, events
            // and configuration that were collectable all along. The grace is
            // what buys the honest answer room to be built.
            let give_up_at = request
                .deadline
                .checked_sub(Duration::from_millis(CONTROL_SCREENSHOT_GRACE_MS))
                .unwrap_or(request.deadline);
            if self.screenshot.is_none() && now < give_up_at {
                self.arm_screenshot(ctx);
                self.awaiting_screenshot.push_back(request);
                continue;
            }
            let result = self.execute_on_ui(app, generation, &request);
            let _ = request.response.try_send(result);
            served = served.saturating_add(1);
        }
        // Nothing is waiting any more, so nothing is owed a rasterise. Left
        // set, the flag would suppress every future arming for the rest of the
        // session (see `arm_screenshot`).
        if self.awaiting_screenshot.is_empty() {
            self.screenshot_armed = false;
        }
        served
    }

    /// Whether this request has to wait a frame for the window to be
    /// rasterised, arming the rasterise if so.
    fn defer_for_screenshot(&mut self, request: &UiRequest, ctx: &eframe::egui::Context) -> bool {
        if !request.prepared.dispatch.needs_screenshot() || self.screenshot.is_some() {
            return false;
        }
        if self.awaiting_screenshot.len() >= CONTROL_MAX_SCREENSHOT_WAITERS {
            return false;
        }
        self.arm_screenshot(ctx);
        true
    }

    /// Ask the window to rasterise itself, and keep asking while someone is
    /// waiting.
    ///
    /// The command is sent every time rather than once per arming. A window
    /// that is minimised, occluded, or between viewport states can swallow the
    /// request and produce no event; an arming that latched would then park
    /// every future capture behind a command nobody would ever send again, for
    /// the rest of the session. Repeating it costs one viewport command per
    /// frame, and only while a capture is actually waiting for a picture.
    fn arm_screenshot(&mut self, ctx: &eframe::egui::Context) {
        self.screenshot_armed = true;
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Screenshot);
        ctx.request_repaint();
    }

    /// Forget every retained bundle and everything staged around one.
    ///
    /// One function, called by both teardown paths, so the two cannot disagree
    /// about what a withdrawal clears — which is exactly the kind of omission
    /// nobody notices until a screenshot flag outlives the door it came
    /// through.
    fn forget_evidence(&mut self) {
        self.evidence.clear();
        self.screenshot = None;
        self.screenshot_armed = false;
        self.awaiting_screenshot.clear();
    }

    /// Record the semantic changes since the last frame: tab, focus,
    /// selection, feed connection and market, replay state. The baseline is
    /// compared in place — a handful of integer and string comparisons — and
    /// refreshed only where something changed, so a quiet frame allocates
    /// nothing; with access disabled nothing runs at all, the journal starts
    /// when the human opens the door and records changes, not the state it
    /// found.
    fn emit_semantic_changes(&mut self, app: &QuantickApp) {
        let tabs = app.control_tabs();
        let active = &tabs[app
            .control_active_tab_index()
            .min(tabs.len().saturating_sub(1))];
        let active_tab_id = active.id;
        let focused_pane_id = active.pane(active.focused_side()).id;
        let selection = selection_identity(app);
        let Some(mut baseline) = self.semantic_baseline.take() else {
            self.semantic_baseline = Some(SemanticBaseline {
                active_tab_id,
                focused_pane_id,
                selection,
                tabs: tabs.iter().map(tab_key).collect(),
            });
            return;
        };
        let now = metrics::wall_clock_ms();
        if baseline.active_tab_id != active_tab_id {
            baseline.active_tab_id = active_tab_id;
            self.record_observed(
                "workspace",
                "workspace.tab.activated",
                json!({ "tab_id": active_tab_id.to_string() }),
                now,
            );
        }
        if baseline.focused_pane_id != focused_pane_id {
            baseline.focused_pane_id = focused_pane_id;
            self.record_observed(
                "workspace",
                "workspace.focus.changed",
                json!({
                    "tab_id": active_tab_id.to_string(),
                    "pane_id": focused_pane_id.to_string(),
                }),
                now,
            );
        }
        if baseline.selection != selection {
            baseline.selection = selection;
            // The owned snapshot is built only now, for the event: it is the
            // same projection the selection scope publishes, so "changed"
            // means the same thing to the journal and to a capture.
            self.record_observed(
                "interaction",
                "interaction.selection.changed",
                json!({ "selection": selection_snapshot(app) }),
                now,
            );
        }
        for tab in tabs {
            match baseline.tabs.iter_mut().find(|old| old.tab_id == tab.id) {
                None => {
                    let key = tab_key(tab);
                    self.record_observed(
                        "workspace",
                        "workspace.tab.opened",
                        json!({ "tab_id": key.tab_id.to_string(), "feed_id": key.feed_id, "symbol": key.symbol }),
                        now,
                    );
                    baseline.tabs.push(key);
                }
                Some(old) => {
                    if old.feed_id != tab.active.0 || old.symbol != tab.active.1 {
                        old.feed_id.clone_from(&tab.active.0);
                        old.symbol.clone_from(&tab.active.1);
                        self.record_observed(
                            "feed",
                            "feed.market.changed",
                            json!({ "tab_id": tab.id.to_string(), "feed_id": old.feed_id, "symbol": old.symbol }),
                            now,
                        );
                    }
                    let connection = connection_state(tab.feed_connection);
                    if old.connection != connection {
                        old.connection = connection;
                        self.record_observed(
                            "feed",
                            "feed.connection.changed",
                            json!({ "tab_id": tab.id.to_string(), "state": connection }),
                            now,
                        );
                    }
                    let replay = replay_key(tab);
                    if old.replay != replay {
                        old.replay = replay;
                        self.record_observed(
                            "replay",
                            "replay.state.changed",
                            json!({
                                "tab_id": tab.id.to_string(),
                                "active": replay.is_some(),
                                "playing": replay.map(|(playing, _)| playing),
                                "finished": replay.map(|(_, finished)| finished),
                            }),
                            now,
                        );
                    }
                    // One comparison for both analysis scopes: it walks two
                    // slices per pane and allocates only once it has something
                    // to say.
                    if !analysis_matches(tab, &old.panes) {
                        self.record_analysis_changes(tab, &old.panes, now);
                        old.panes = pane_analysis_keys(tab);
                    }
                }
            }
        }
        if baseline.tabs.len() != tabs.len()
            || baseline
                .tabs
                .iter()
                .any(|old| !tabs.iter().any(|tab| tab.id == old.tab_id))
        {
            baseline.tabs.retain(|old| {
                let open = tabs.iter().any(|tab| tab.id == old.tab_id);
                if !open {
                    self.record_observed(
                        "workspace",
                        "workspace.tab.closed",
                        json!({ "tab_id": old.tab_id.to_string() }),
                        now,
                    );
                }
                open
            });
        }
        self.semantic_baseline = Some(baseline);
    }

    /// Journal what changed about one tab's indicators and drawings.
    ///
    /// Reached only from a frame where [`analysis_matches`] already answered
    /// `false`, so the allocation here is paid once per change and never on a
    /// quiet frame. Identity drives the diff: an indicator is its slot, a
    /// drawing is its id, and a list that merely reordered emits nothing.
    ///
    /// A slot is an address, not an identity — the pane hands it out again
    /// after a remove — so a slot whose kind changed is reported as the
    /// detach and the attach it really is, never as one instance mutating.
    fn record_analysis_changes(
        &mut self,
        tab: &crate::tab::Tab,
        stored: &[PaneAnalysisKey],
        now: i64,
    ) {
        let tab_id = tab.id.to_string();
        for (pane, side) in analysis_panes(tab) {
            let was = stored.iter().find(|key| key.pane_id == pane.id);
            let previous_indicators = was.map(|key| key.indicators.as_slice()).unwrap_or(&[]);
            let previous_drawings = was.map(|key| key.drawings.as_slice()).unwrap_or(&[]);
            let side = crate::control::PaneSideDto::from(side);
            let pane_id = pane.id.to_string();

            let indicators = pane.indicators.all();
            let drawings = pane.drawings.items();

            for view in indicators {
                match previous_indicators
                    .iter()
                    .find(|key| key.slot == view.slot.0)
                {
                    None => self.record_indicator_attached(
                        &tab_id,
                        &pane_id,
                        side,
                        view.slot.0,
                        view.kind.as_ref(),
                        now,
                    ),
                    // The pane reused the address for another constructor
                    // between two frames. Two events, because two instances.
                    Some(key) if *key.kind != *view.kind => {
                        self.record_indicator_detached(
                            &tab_id,
                            &pane_id,
                            side,
                            key.slot,
                            key.kind.as_ref(),
                            now,
                        );
                        self.record_indicator_attached(
                            &tab_id,
                            &pane_id,
                            side,
                            view.slot.0,
                            view.kind.as_ref(),
                            now,
                        );
                    }
                    Some(key) => {
                        let failing = view.error.is_some() || view.stale.is_some();
                        if key.failing != failing {
                            self.record_observed(
                                "analysis",
                                "analysis.indicator.compile_state.changed",
                                json!({
                                    "tab_id": tab_id,
                                    "pane_id": pane_id,
                                    "pane_side": side,
                                    "slot_id": view.slot.0.to_string(),
                                    "kind": view.kind.as_ref(),
                                    "failing": failing,
                                }),
                                now,
                            );
                        }
                    }
                }
            }
            for key in previous_indicators {
                if !indicators.iter().any(|view| view.slot.0 == key.slot) {
                    self.record_indicator_detached(
                        &tab_id,
                        &pane_id,
                        side,
                        key.slot,
                        key.kind.as_ref(),
                        now,
                    );
                }
            }

            for drawing in drawings {
                match previous_drawings.iter().find(|key| key.id == drawing.id.0) {
                    None => self.record_observed(
                        "analysis",
                        "analysis.drawing.created",
                        json!({
                            "tab_id": tab_id,
                            "pane_id": pane_id,
                            "pane_side": side,
                            "drawing_id": drawing.id.0.to_string(),
                            "tool_id": drawing.tool.id(),
                            "author": author_payload(drawing),
                        }),
                        now,
                    ),
                    Some(key) if !key.matches(drawing) => self.record_observed(
                        "analysis",
                        "analysis.drawing.edited",
                        json!({
                            "tab_id": tab_id,
                            "pane_id": pane_id,
                            "pane_side": side,
                            "drawing_id": drawing.id.0.to_string(),
                            "tool_id": drawing.tool.id(),
                            "locked": drawing.locked,
                            "hidden": drawing.hidden,
                            // Presence, never the text: the journal is held to
                            // the same rule as the wire.
                            "user_label_present": drawing.name.is_some(),
                            "author": author_payload(drawing),
                        }),
                        now,
                    ),
                    Some(_) => {}
                }
            }
            for key in previous_drawings {
                if !drawings.iter().any(|drawing| drawing.id.0 == key.id) {
                    self.record_drawing_removed(&tab_id, &pane_id, side, key.id, now);
                }
            }
        }

        // A pane the layout closed takes its whole arrangement with it. Walked
        // separately because the loop above only visits panes that still
        // exist, and a client left holding indicators and marks that are gone
        // is exactly what this journal is for.
        for key in stored {
            if analysis_panes(tab).any(|(pane, _side)| pane.id == key.pane_id) {
                continue;
            }
            let side = crate::control::PaneSideDto::from(key.side);
            let pane_id = key.pane_id.to_string();
            for indicator in &key.indicators {
                self.record_indicator_detached(
                    &tab_id,
                    &pane_id,
                    side,
                    indicator.slot,
                    indicator.kind.as_ref(),
                    now,
                );
            }
            for drawing in &key.drawings {
                self.record_drawing_removed(&tab_id, &pane_id, side, drawing.id, now);
            }
        }
    }

    fn record_indicator_attached(
        &mut self,
        tab_id: &str,
        pane_id: &str,
        side: crate::control::PaneSideDto,
        slot: u64,
        kind: &str,
        now: i64,
    ) {
        self.record_observed(
            "analysis",
            "analysis.indicator.attached",
            json!({
                "tab_id": tab_id,
                "pane_id": pane_id,
                "pane_side": side,
                "slot_id": slot.to_string(),
                "kind": kind,
            }),
            now,
        );
    }

    fn record_indicator_detached(
        &mut self,
        tab_id: &str,
        pane_id: &str,
        side: crate::control::PaneSideDto,
        slot: u64,
        kind: &str,
        now: i64,
    ) {
        self.record_observed(
            "analysis",
            "analysis.indicator.detached",
            json!({
                "tab_id": tab_id,
                "pane_id": pane_id,
                "pane_side": side,
                "slot_id": slot.to_string(),
                "kind": kind,
            }),
            now,
        );
    }

    fn record_drawing_removed(
        &mut self,
        tab_id: &str,
        pane_id: &str,
        side: crate::control::PaneSideDto,
        drawing_id: u64,
        now: i64,
    ) {
        self.record_observed(
            "analysis",
            "analysis.drawing.removed",
            json!({
                "tab_id": tab_id,
                "pane_id": pane_id,
                "pane_side": side,
                "drawing_id": drawing_id.to_string(),
            }),
            now,
        );
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

    /// Grant exactly these scopes to the next connection.
    ///
    /// The panel's checkboxes write the same set; this is the named call
    /// behind them, so a scripted run, a test and a later operator reach the
    /// grant without a mouse.
    ///
    /// Two shorthands, and nothing else is special: `all-reads` is the safe
    /// default grant, and `annotate-tier` is every scope of the annotate
    /// tier, because a trader who says "let it answer on the chart" means the
    /// tier rather than a list of IDs. Every other token is a registered
    /// permission ID granting exactly itself — `annotate` included, which is
    /// the tier's floor and opens nothing on its own. An unknown ID is refused
    /// loudly rather than silently dropped: a typo that quietly grants less is
    /// a debugging afternoon.
    pub(crate) fn configure_scopes(&mut self, scopes: &str) -> Result<(), String> {
        if !matches!(self.state, AccessState::Disabled) {
            return Err("scopes change only while access is off".to_owned());
        }
        let known: BTreeMap<&str, PermissionId> = self
            .contract
            .selectable_permissions()
            .map(|descriptor| (descriptor.id.as_str(), descriptor.id.clone()))
            .collect();
        let mut granted = BTreeSet::new();
        for token in scopes.split(',').map(str::trim).filter(|id| !id.is_empty()) {
            match token {
                "all-reads" => granted.extend(self.contract.default_grant()),
                "annotate-tier" => granted.extend(
                    known
                        .values()
                        .filter(|permission| is_annotate_permission(permission))
                        .cloned(),
                ),
                id => match known.get(id) {
                    Some(permission) => {
                        granted.insert(permission.clone());
                    }
                    None => return Err(format!("`{id}` is not a registered scope")),
                },
            }
        }
        // The floor every profile stands on, named rather than taken from the
        // front of a sorted set: a grant of scopes with no `observe` reaches
        // nothing at all, which is never what was meant.
        granted.insert(
            PermissionId::new(OBSERVE_PERMISSION_ID).expect("static permission ID is valid"),
        );
        // The annotate tier's own floor, for the same reason: every annotate
        // capability requires it, so `annotate.chart` on its own would raise
        // the ceiling to `annotator` and then refuse every call made with it.
        if granted.iter().any(is_annotate_scope) {
            granted.insert(
                PermissionId::new(ANNOTATE_PERMISSION_ID).expect("static permission ID is valid"),
            );
        }
        self.configured_scopes = granted;
        Ok(())
    }

    pub fn is_enabled(&self) -> bool {
        matches!(self.state, AccessState::Enabled(_))
    }

    /// Whether any annotate scope is granted for the next connection — the
    /// one question that decides whether a client may answer on the chart.
    pub(crate) fn grants_annotate(&self) -> bool {
        // The floor on its own opens nothing, so it never makes the status
        // line claim the window can be answered on — and a scope without the
        // floor opens nothing either, because every annotate capability
        // requires both. Claiming otherwise would put "answering on the
        // chart" on the panel over a connection that is refused every time.
        self.configured_scopes.iter().any(is_annotate_scope)
            && self
                .configured_scopes
                .iter()
                .any(|permission| permission.as_str() == ANNOTATE_PERMISSION_ID)
    }

    /// The ceiling every connection of the next run is capped at. It follows
    /// the scopes the human ticked: no annotate scope, no annotator profile,
    /// however loudly a client asks for one.
    /// Whether the next connection may rearrange the window.
    ///
    /// Same shape as [`Self::grants_annotate`], and for the same reason: the
    /// floor on its own opens nothing and a scope without the floor opens
    /// nothing either, because every cockpit capability requires both.
    pub(crate) fn grants_cockpit(&self) -> bool {
        self.configured_scopes.iter().any(is_cockpit_scope)
            && self
                .configured_scopes
                .iter()
                .any(|permission| permission.as_str() == COCKPIT_PERMISSION_ID)
    }

    /// The ceiling the next connection is given.
    ///
    /// A profile no code path constructs is a tier nothing can reach: the
    /// seven `layout.*` capabilities shipped registered, catalogued and
    /// refused at the gate because this function knew only two profiles. The
    /// cockpit tier is checked first because it is the higher ceiling — it
    /// inherits the observer's reads, and a connection granted both tiers
    /// needs the one that covers both.
    fn configured_profile(&self) -> ProfileId {
        let id = if self.grants_cockpit() {
            COCKPIT_PROFILE_ID
        } else if self.grants_annotate() {
            ANNOTATOR_PROFILE_ID
        } else {
            OBSERVER_PROFILE_ID
        };
        ProfileId::new(id).expect("static profile ID is valid")
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
            AccessState::Enabled(_) if self.grants_annotate() => {
                "On — reading, and answering on the chart"
            }
            AccessState::Enabled(_) => "On — reading only",
            AccessState::Disabling(_) => "Disabling and revoking clients…",
        };
        ui.horizontal(|ui| {
            ui.strong("Status:");
            ui.label(status);
        });
        // The one trader gesture this surface owns, said where the trader
        // configures it: a newcomer finds it here, not in a manual.
        ui.small(format!(
            "{} marks what is under the pointer; clients read marks through events.",
            ui.ctx().format_shortcut(&MARK_SHORTCUT)
        ));

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
        for descriptor in self.contract.selectable_permissions().filter(|descriptor| {
            !is_annotate_permission(&descriptor.id)
                && !is_cockpit_permission(&descriptor.id)
                // The trade tier is not offered here at all. It is not a
                // read scope — this heading promises reading only, and the
                // cockpit section exists because that same mistake was made
                // once already — and it is not offered anywhere else either,
                // because nothing can currently grant it: no profile the
                // handshake reaches holds it, so a checkbox would take the
                // trader's tick and change nothing. An honest absence beats
                // a control that lies about what it does. The section that
                // grants it belongs to the change that decides some
                // connection may trade.
                && !is_trade_permission(&descriptor.id)
        }) {
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
        // The tier that writes is a separate decision, said in the words a
        // trader would use: everything above lets an assistant *read* the
        // window; everything here lets it put something in it.
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        ui.strong("Let an assistant answer on the chart");
        ui.small(
            "Objects an assistant places are labelled with its name wherever you see them, and \"Remove objects placed for you\" in the object manager takes them all back at once. Nothing here can delete your own drawings or touch a position. Rearranging your charts is the separate grant below.",
        );
        for descriptor in self
            .contract
            .selectable_permissions()
            .filter(|descriptor| is_annotate_permission(&descriptor.id))
        {
            let mut selected = self.configured_scopes.contains(&descriptor.id);
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
        // The cockpit tier, in its own section for the reason it is its own
        // tier: it is a *write* grant, and it was rendering under "Read
        // scopes" — a checkbox that rearranges the trader's window, presented
        // as though it only looked at it.
        ui.add_space(CONTROL_PANEL_SECTION_SPACING_PX);
        ui.strong("Let an assistant rearrange your charts");
        ui.small(
            "Changes which charts are on screen, where they sit and how wide they are — the same things the layout picker does. Nothing here places or removes an object, and nothing here touches a position. A chart put away keeps its drawings, its indicators and its bars, and comes back with them.",
        );
        for descriptor in self
            .contract
            .selectable_permissions()
            .filter(|descriptor| is_cockpit_permission(&descriptor.id))
        {
            let mut selected = self.configured_scopes.contains(&descriptor.id);
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
                let label = if self.grants_cockpit() {
                    "Enable access (reading, answering and rearranging)"
                } else if self.grants_annotate() {
                    "Enable access (reading and answering)"
                } else {
                    "Enable observer access"
                };
                if ui
                    .add_enabled(self.identity.is_some(), eframe::egui::Button::new(label))
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
            profile_ceiling: self.configured_profile(),
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
        // Evidence does not outlive the door it came through: a bundle is
        // every granted scope at once, and the grant is what has just been
        // withdrawn.
        self.forget_evidence();
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
                // A gateway of a generation this run has moved past: shut it
                // down and let it go. Overwriting the state here would throw
                // away the runtime — or the pending enable — that replaced it,
                // and an enable/disable/enable in quick succession would leave
                // access off with no way back short of another enable.
                LifecycleEvent::Started { runtime, .. } => runtime.request_shutdown(),
                LifecycleEvent::Failed {
                    generation,
                    message,
                } if generation == self.grant_generation => {
                    self.active_cancellation = None;
                    self.notice = Some(message);
                    // A gateway that died takes the evidence with it: access
                    // is off either way, and a bundle outliving the door it
                    // came through is the accumulation the bounds exist to
                    // stop, however the door closed.
                    self.forget_evidence();
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
                    // Only the gateway this state actually holds may end it.
                    // `request_disable` bumps the generation before the run it
                    // is stopping reports back, so a `Disabling` state accepts
                    // the stop it asked for; every other generation is an
                    // older run finishing its own cleanup and must not turn
                    // off the one that replaced it.
                    let held = match &self.state {
                        AccessState::Enabled(runtime) | AccessState::Disabling(Some(runtime)) => {
                            Some(runtime.grant_generation)
                        }
                        AccessState::Disabled
                        | AccessState::Enabling
                        | AccessState::Disabling(None) => None,
                    };
                    let ours = held.map_or(expected, |held| held == generation);
                    if ours {
                        self.active_cancellation = None;
                        self.connections.clear();
                        self.revoked_connections.clear();
                        // Expected stop or unexpected, the evidence goes.
                        self.forget_evidence();
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
                    self.notification_limits.remove(&connection_id);
                    self.connections.remove(&connection_id);
                    // The revocation is deliberately *not* lifted here.
                    // Revoking closes the socket, which produces this very
                    // status; clearing the id would let a request the revoked
                    // client had already queued run on the drain that follows
                    // in the same frame. Connection IDs are random per
                    // connection, so a later client can never inherit one,
                    // and the set is emptied when the gateway stops.
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
        // Before the early return below: a disabled gateway holds no bundles,
        // and one that is still running must not leave any behind.
        self.forget_evidence();
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

/// What the frame emitter remembers between frames: owned copies of the
/// values that name a change, refreshed only where one happened, so a quiet
/// frame compares in place and allocates nothing.
struct SemanticBaseline {
    active_tab_id: u64,
    focused_pane_id: u64,
    selection: SelectionIdentity,
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
    /// What the trader has placed on each pane. Rebuilt only when it stops
    /// matching — see [`analysis_matches`].
    panes: Vec<PaneAnalysisKey>,
}

/// One pane's indicators and drawings, as the journal compares them.
///
/// This is a baseline, not a projection: it holds the least that can tell a
/// change apart, so the per-frame comparison touches integers and booleans and
/// allocates nothing.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PaneAnalysisKey {
    pane_id: u64,
    /// Which half of the split this was. Kept so a pane the layout *closed*
    /// can still name its side in the events that retire what it held — by
    /// then there is no pane left to ask.
    side: crate::pane::PaneSide,
    indicators: Vec<IndicatorKey>,
    drawings: Vec<DrawingKey>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct IndicatorKey {
    slot: u64,
    kind: std::sync::Arc<str>,
    /// An evaluation error or a failed hot reload. Either one is what a client
    /// waiting on a compile is waiting for.
    failing: bool,
}

impl IndicatorKey {
    fn matches(&self, view: &crate::indicators::IndicatorView) -> bool {
        self.slot == view.slot.0
            && *self.kind == *view.kind
            && self.failing == (view.error.is_some() || view.stale.is_some())
    }

    fn of(view: &crate::indicators::IndicatorView) -> Self {
        Self {
            slot: view.slot.0,
            kind: std::sync::Arc::clone(&view.kind),
            failing: view.error.is_some() || view.stale.is_some(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DrawingKey {
    id: u64,
    locked: bool,
    hidden: bool,
    /// Whether the trader gave it a name — never the name, which is their own
    /// text and does not belong in the journal any more than on the wire.
    named: bool,
    /// `(actor kind, client name)` when something other than the trader's hand
    /// placed it.
    author: Option<(String, String)>,
}

impl DrawingKey {
    fn matches(&self, drawing: &crate::drawings::Drawing) -> bool {
        self.id == drawing.id.0
            && self.locked == drawing.locked
            && self.hidden == drawing.hidden
            && self.named == drawing.name.is_some()
            && match (&self.author, &drawing.author) {
                (None, None) => true,
                (Some((kind, name)), Some(author)) => {
                    kind == &author.actor_kind && name == &author.client_name
                }
                (None, Some(_)) | (Some(_), None) => false,
            }
    }

    fn of(drawing: &crate::drawings::Drawing) -> Self {
        Self {
            id: drawing.id.0,
            locked: drawing.locked,
            hidden: drawing.hidden,
            named: drawing.name.is_some(),
            author: drawing
                .author
                .as_ref()
                .map(|author| (author.actor_kind.clone(), author.client_name.clone())),
        }
    }
}

/// A drawing's author as the journal carries it. Absent is the trader's own
/// hand; anything else names who acted, under the same rule that labels an
/// inferred side.
fn author_payload(drawing: &crate::drawings::Drawing) -> Value {
    drawing.author.as_ref().map_or(Value::Null, |author| {
        json!({
            "actor_kind": author.actor_kind,
            "client_name": author.client_name,
        })
    })
}

fn tab_key(tab: &crate::tab::Tab) -> TabKey {
    TabKey {
        tab_id: tab.id,
        feed_id: tab.active.0.clone(),
        symbol: tab.active.1.clone(),
        connection: connection_state(tab.feed_connection),
        replay: replay_key(tab),
        panes: pane_analysis_keys(tab),
    }
}

/// The panes of one tab, in the order both analysis scopes publish them.
///
/// [`crate::tab::Tab::panes`] and not a local `Vec`: this is walked by
/// [`analysis_matches`] on every frame, and the version of this comparison
/// #223's review removed was removed for allocating exactly one vector per
/// frame. One walk shared with the scopes also keeps the journal's order and
/// the wire's order from drifting apart.
fn analysis_panes(
    tab: &crate::tab::Tab,
) -> impl Iterator<Item = (&crate::pane::ChartPane, crate::pane::PaneSide)> {
    tab.panes()
}

fn pane_analysis_keys(tab: &crate::tab::Tab) -> Vec<PaneAnalysisKey> {
    analysis_panes(tab)
        .map(|(pane, side)| PaneAnalysisKey {
            pane_id: pane.id,
            side,
            indicators: pane.indicators.all().iter().map(IndicatorKey::of).collect(),
            drawings: pane.drawings.items().iter().map(DrawingKey::of).collect(),
        })
        .collect()
}

/// Whether the stored baseline still describes this tab's panes.
///
/// Compared in place and allocating nothing: a quiet frame walks two slices
/// and returns. Only a frame that answers `false` pays for
/// [`pane_analysis_keys`] and the events below.
fn analysis_matches(tab: &crate::tab::Tab, stored: &[PaneAnalysisKey]) -> bool {
    let mut stored = stored.iter();
    let matched = analysis_panes(tab).all(|(pane, _side)| {
        let Some(key) = stored.next() else {
            return false;
        };
        {
            let indicators = pane.indicators.all();
            let drawings = pane.drawings.items();
            pane.id == key.pane_id
                && indicators.len() == key.indicators.len()
                && drawings.len() == key.drawings.len()
                && indicators
                    .iter()
                    .zip(&key.indicators)
                    .all(|(view, stored)| stored.matches(view))
                && drawings
                    .iter()
                    .zip(&key.drawings)
                    .all(|(drawing, stored)| stored.matches(drawing))
        }
    });
    // Every stored pane must have been consumed too: a closed pane leaves the
    // iterator short, and a tab that lost one has changed.
    matched && stored.next().is_none()
}

fn replay_key(tab: &crate::tab::Tab) -> Option<(bool, bool)> {
    tab.replay
        .as_ref()
        .map(|link| (link.status.is_playing(), link.status.is_finished()))
}

/// Microseconds spent since `started`, saturating.
///
/// One reading for everything that shares the frame budget, so the drain and
/// the captures that run before it are measuring the same thing.
fn elapsed_us_since(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

fn drain_bounded_since<T>(
    receiver: &Receiver<T>,
    started: Instant,
    already_processed: usize,
    mut handle: impl FnMut(T),
) -> DrainObservation {
    // Requests this frame already ran elsewhere (the screenshot waiters) count
    // against the same ceiling: one frame, one budget.
    let mut processed = already_processed;
    // Why the drain stopped is recorded where it stops, not re-derived from
    // a later clock reading: the count ceiling, the budget, or an empty queue
    // are three different diagnoses.
    let mut stopped_on_budget = false;
    loop {
        if processed >= CONTROL_UI_MAX_REQUESTS_PER_FRAME {
            break;
        }
        if elapsed_us_since(started) >= CONTROL_UI_BUDGET_US {
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
    let elapsed_us = elapsed_us_since(started);
    DrainObservation {
        processed,
        elapsed_us,
        budget_exceeded: stopped_on_budget,
        queue_has_more: !receiver.is_empty(),
    }
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
            // The descriptor is already on disk with this run's bearer token
            // and port. Leaving it there would advertise a token for a port
            // the operating system is about to hand to somebody else.
            let _ = published.remove();
            return Err(
                "Could not start the gateway's waiter manager; access remains off.".to_owned(),
            );
        }
    }
    let authority = Arc::new(ConnectionAuthority {
        identity: start.identity.clone(),
        bearer_token: token,
        profile_ceiling: start.profile_ceiling.clone(),
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
    /// The connection that parked it: a closed one releases the wait at the
    /// manager's next pass instead of holding its slots to the deadline.
    connection: Arc<ConnectionSlots>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WakeReason {
    Woken,
    TimedOut,
    Shutdown,
    Disconnected,
}

/// What one connection's reader and its response threads share: the
/// in-flight count and request IDs (contract §5.2), the parked waits it
/// holds, and whether its socket is still open.
struct ConnectionSlots {
    in_flight: AtomicUsize,
    in_flight_ids: Mutex<BTreeSet<quantick_control::id::RequestId>>,
    parked: AtomicUsize,
    closed: AtomicBool,
}

impl ConnectionSlots {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            in_flight: AtomicUsize::new(0),
            in_flight_ids: Mutex::new(BTreeSet::new()),
            parked: AtomicUsize::new(0),
            closed: AtomicBool::new(false),
        })
    }

    /// A poisoned lock reads as "in flight": refusing a request is the safe
    /// side of a broken invariant.
    fn is_in_flight(&self, request_id: &quantick_control::id::RequestId) -> bool {
        self.in_flight_ids
            .lock()
            .map(|ids| ids.contains(request_id))
            .unwrap_or(true)
    }

    fn track(&self, request_id: &quantick_control::id::RequestId) {
        if let Ok(mut ids) = self.in_flight_ids.lock() {
            ids.insert(request_id.clone());
        }
    }

    fn forget(&self, request_id: &quantick_control::id::RequestId) {
        if let Ok(mut ids) = self.in_flight_ids.lock() {
            ids.remove(request_id);
        }
    }
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
            // A disconnected receiver is *always* ready: without this the
            // journal going away would spin this thread at full speed
            // instead of ending it.
            recv(ticks) -> tick => if tick.is_err() { break },
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
            if waiter.connection.closed.load(Ordering::Acquire) {
                let _ = waiter.wake.send(WakeReason::Disconnected);
                false
            } else if next > waiter.target_sequence {
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
    profile_ceiling: ProfileId,
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
    // Capacity rejections answer off this thread; this counts the ones still
    // in flight so a reconnect loop cannot spawn threads without bound.
    let rejecting = Arc::new(AtomicUsize::new(0));
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
                        // may wait up to the handshake timeout. Bounded, for
                        // the same reason the connections are: a peer that
                        // reconnects in a loop would otherwise spawn a thread
                        // per attempt, each living to the handshake timeout.
                        if !try_reserve_in_flight(&rejecting, authority.options.max_connections) {
                            let _ = stream.shutdown(Shutdown::Both);
                            continue;
                        }
                        let options = authority.options.clone();
                        let in_flight = Arc::clone(&rejecting);
                        let spawned = thread::Builder::new()
                            .name("quantick-control-reject".to_owned())
                            .spawn(move || {
                                reject_connection_capacity(stream, &options);
                                in_flight.fetch_sub(1, Ordering::AcqRel);
                            });
                        if spawned.is_err() {
                            rejecting.fetch_sub(1, Ordering::AcqRel);
                        }
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
    let remote_actor = RemoteActor {
        principal_id: principal_id.clone(),
        client_name: handshake.client_name.clone(),
        connection_id: connection_id.clone(),
    };
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
        profile_ceiling: authority.profile_ceiling.clone(),
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
    // Request IDs in flight on this connection (contract §5.2: a duplicate is
    // rejected while the first is still executing). Requests that leave for
    // the application thread and parked waits are in flight from the
    // reader's point of view; a worker-side read is answered before the next
    // frame is read.
    let slots = ConnectionSlots::new();
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
        if slots.is_in_flight(&request.request_id) {
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
            &remote_actor,
            &accepted,
            &codec,
            &writer,
            &slots,
            authority,
        );
    }
    // The socket is gone: this connection's parked waits release their slots
    // at the manager's next pass instead of holding them to the deadline.
    slots.closed.store(true, Ordering::Release);

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
    remote_actor: &RemoteActor,
    handshake: &quantick_control::handshake::HandshakeResponse,
    codec: &BoundedCodec,
    writer: &Arc<Mutex<TcpStream>>,
    slots: &Arc<ConnectionSlots>,
    authority: &Arc<ConnectionAuthority>,
) {
    if let PreparedDispatch::Parked(wait) = &prepared.dispatch {
        let wait = wait.clone();
        dispatch_parked_wait(
            prepared,
            wait,
            connection_id,
            remote_actor,
            handshake,
            codec,
            writer,
            slots,
            authority,
        );
        return;
    }
    // Every terminal path below forgets the request ID: a wait that parked
    // under this ID is in flight until its read is answered or refused.
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
        slots.forget(&prepared.envelope.request_id);
        return;
    }
    if let Some(result) = prepared.dispatch.execute_worker(
        &authority.contract,
        &authority.identity.instance_id,
        &handshake.effective_profile,
        &handshake.effective_scopes,
        &handshake.effective_limits,
    ) {
        let response = serialize_worker_result(&authority.contract, &prepared.envelope, result);
        send_response(writer, codec, response);
        authority.global_in_flight.fetch_sub(1, Ordering::AcqRel);
        slots.forget(&prepared.envelope.request_id);
        return;
    }
    if !try_reserve_in_flight(
        &slots.in_flight,
        authority.options.max_in_flight_per_connection,
    ) {
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
        slots.forget(&prepared.envelope.request_id);
        return;
    }

    let envelope = prepared.envelope.clone();
    slots.track(&envelope.request_id);
    let (response_tx, response_rx) = bounded(1);
    let deadline = Instant::now() + authority.options.request_timeout;
    let response_writer = Arc::clone(writer);
    let response_codec = codec.clone();
    let response_slots = Arc::clone(slots);
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
            response_slots.forget(&wait_envelope.request_id);
            response_slots.in_flight.fetch_sub(1, Ordering::AcqRel);
            response_global_in_flight.fetch_sub(1, Ordering::AcqRel);
        });
    if spawn.is_err() {
        slots.forget(&envelope.request_id);
        slots.in_flight.fetch_sub(1, Ordering::AcqRel);
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

    let actor = matches!(prepared.dispatch, PreparedDispatch::Action(_))
        .then(|| Box::new(remote_actor.context(&prepared.envelope)));
    let ui_request = UiRequest {
        prepared,
        actor,
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
/// waiter manager — holding one global and one per-connection parked slot
/// and its request ID, nothing else — and run the bounded read through the
/// ordinary UI path when woken or timed out.
#[allow(clippy::too_many_arguments)]
fn dispatch_parked_wait(
    prepared: PreparedRequest,
    wait: ParkedWait,
    connection_id: &ConnectionId,
    remote_actor: &RemoteActor,
    handshake: &quantick_control::handshake::HandshakeResponse,
    codec: &BoundedCodec,
    writer: &Arc<Mutex<TcpStream>>,
    slots: &Arc<ConnectionSlots>,
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
    let dropped_before = position.dropped_before;
    let read_input = EventsReadInput {
        cursor: Some(EventCursor {
            instance_id: instance_id.clone(),
            next_sequence: WireU64::new(target),
        }),
        start: None,
        limit: wait.input.limit,
    };
    let envelope = prepared.envelope.clone();
    let to_read = move |timed_out: bool| PreparedRequest {
        envelope: prepared.envelope,
        required_permissions: prepared.required_permissions,
        dispatch: PreparedDispatch::Ui(Box::new(EventsReadInvocation {
            input: read_input,
            timed_out,
            dropped_before,
        })),
    };
    if authority.journal_signal.next_sequence() > target {
        // Already behind the journal: no parking, just the read.
        dispatch_prepared(
            to_read(false),
            connection_id,
            remote_actor,
            handshake,
            codec,
            writer,
            slots,
            authority,
        );
        return;
    }
    if !try_reserve_in_flight(&slots.parked, CONTROL_MAX_PARKED_WAITERS_PER_CONNECTION) {
        send_response(
            writer,
            codec,
            failure_response(
                &envelope,
                known_error(
                    codes::BACKPRESSURE,
                    "this connection's parked waiter capacity is full",
                    true,
                ),
            ),
        );
        return;
    }
    if !try_reserve_in_flight(&authority.parked_waiters, CONTROL_MAX_PARKED_WAITERS) {
        slots.parked.fetch_sub(1, Ordering::AcqRel);
        send_response(
            writer,
            codec,
            failure_response(
                &envelope,
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
            connection: Arc::clone(slots),
        })
        .is_err()
    {
        authority.parked_waiters.fetch_sub(1, Ordering::AcqRel);
        slots.parked.fetch_sub(1, Ordering::AcqRel);
        send_response(
            writer,
            codec,
            failure_response(
                &envelope,
                known_error(codes::BACKPRESSURE, "parked waiter capacity is full", true),
            ),
        );
        return;
    }
    // Parked: the ID is in flight until the read is answered (contract §5.2).
    slots.track(&envelope.request_id);
    let thread_authority = Arc::clone(authority);
    let thread_connection_id = connection_id.clone();
    let thread_remote_actor = remote_actor.clone();
    let thread_handshake = handshake.clone();
    let thread_codec = codec.clone();
    let thread_writer = Arc::clone(writer);
    let thread_slots = Arc::clone(slots);
    let thread_envelope = envelope.clone();
    let spawned = thread::Builder::new()
        .name("quantick-control-wait".to_owned())
        .spawn(move || {
            let reason = wake_rx.recv().unwrap_or(WakeReason::Shutdown);
            thread_authority
                .parked_waiters
                .fetch_sub(1, Ordering::AcqRel);
            thread_slots.parked.fetch_sub(1, Ordering::AcqRel);
            match reason {
                // Nobody is listening: release the ID and write nothing.
                WakeReason::Disconnected => thread_slots.forget(&thread_envelope.request_id),
                _ if thread_slots.closed.load(Ordering::Acquire) => {
                    thread_slots.forget(&thread_envelope.request_id);
                }
                WakeReason::Shutdown => {
                    send_response(
                        &thread_writer,
                        &thread_codec,
                        failure_response(
                            &thread_envelope,
                            known_error(
                                codes::INSTANCE_GONE,
                                "local access was disabled while the wait was parked",
                                true,
                            ),
                        ),
                    );
                    thread_slots.forget(&thread_envelope.request_id);
                }
                WakeReason::Woken | WakeReason::TimedOut => dispatch_prepared(
                    to_read(reason == WakeReason::TimedOut),
                    &thread_connection_id,
                    &thread_remote_actor,
                    &thread_handshake,
                    &thread_codec,
                    &thread_writer,
                    &thread_slots,
                    &thread_authority,
                ),
            }
        });
    if spawned.is_err() {
        // The closure and its wake receiver are gone, so the manager's wake
        // will fail harmlessly; the slots and the ID are released here and
        // the client hears the refusal instead of waiting for a reply nobody
        // would write.
        authority.parked_waiters.fetch_sub(1, Ordering::AcqRel);
        slots.parked.fetch_sub(1, Ordering::AcqRel);
        slots.forget(&envelope.request_id);
        tracing::warn!(
            target: "quantick::control",
            event_code = "CONTROL_WAIT_THREAD_FAILED",
            "could not create a parked-wait thread"
        );
        send_response(
            writer,
            codec,
            failure_response(
                &envelope,
                known_error(
                    codes::BACKPRESSURE,
                    "parked-wait worker could not be created",
                    true,
                ),
            ),
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
            // The reason travels: work that happens after the application
            // thread can refuse for a reason the client can act on — a bundle
            // that does not fit its store answers `control.backpressure`, and
            // whether to retry is a different answer from "serialization
            // failed".
            Err(error) => failure_response(request, error),
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
    // A write that fails part-way has already put a truncated frame on the
    // wire: every byte after it would be read as that frame's payload. The
    // connection cannot be recovered, so it is closed rather than left
    // writing garbage the client will parse as answers.
    if stream.write_all(&frame).is_err() {
        let _ = stream.shutdown(Shutdown::Both);
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

/// Entropy for one runtime identifier, as a control failure rather than a
/// string.
///
/// The evidence reads mint identifiers of their own, and an operating system
/// that cannot supply entropy has to refuse the request in the vocabulary the
/// client already handles — never with a guessable identifier.
pub(crate) fn runtime_id_bytes() -> Result<[u8; CONTROL_RUNTIME_ID_BYTES], ControlError> {
    random_bytes::<CONTROL_RUNTIME_ID_BYTES>().map_err(|error| {
        known_error(
            codes::CAPABILITY_UNAVAILABLE,
            format!("secure identifier generation failed: {error}"),
            true,
        )
    })
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

    /// The panel may not promise what the contract will refuse.
    ///
    /// Every annotate capability requires the `annotate` floor *and* its own
    /// scope. Granting a scope alone used to raise the ceiling to `annotator`
    /// and put "answering on the chart" on the status line over a connection
    /// that was then denied every call it made.
    #[test]
    fn granting_an_annotate_scope_grants_the_floor_every_capability_also_needs() {
        let mut access = ControlAccess::new();
        access
            .configure_scopes("all-reads,annotate.chart")
            .expect("the test grants registered scopes");

        assert!(
            access.grants_annotate(),
            "the panel says the window can be answered on"
        );
        assert!(
            access
                .configured_scopes
                .iter()
                .any(|permission| permission.as_str() == ANNOTATE_PERMISSION_ID),
            "so the floor every annotate capability requires has to be there too"
        );

        // The contrast: the floor on its own opens nothing, so it must not
        // make the status line claim the window can be answered on.
        let mut floor_only = ControlAccess::new();
        floor_only
            .configure_scopes("all-reads,annotate")
            .expect("the test grants registered scopes");
        assert!(
            !floor_only.grants_annotate(),
            "a floor with no scope under it answers on nothing"
        );
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
            0,
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
            let observation = drain_bounded_since(&receiver, Instant::now(), 0, |_| {
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
    fn a_revoked_connection_stays_revoked_when_its_socket_closes() {
        // Revoking closes the socket, so the disconnect it causes lands on the
        // same frame as any request the revoked client had already queued.
        // `poll_statuses` runs before `drain_bounded_since`, so lifting the
        // revocation here would hand that request a clean bill of health and
        // let it act. Connection IDs are random per connection, so keeping the
        // id costs nothing: no later client can inherit it.
        let mut access = ControlAccess::new();
        let (_requests_tx, requests) = bounded(1);
        let (statuses_tx, statuses) = bounded(1);
        let (commands, _commands_rx) = bounded(1);
        let connection_id = ConnectionId::from_bytes([7; 16]);
        access.state = AccessState::Enabled(GatewayRuntime {
            grant_generation: 0,
            requests,
            statuses,
            commands,
            cancellation: Arc::new(AtomicBool::new(false)),
            public: GatewayPublicInfo {
                instance_id: InstanceId::from_bytes([1; 16]),
                port: 0,
                descriptor_path: PathBuf::new(),
                published_at_unix_ms: 0,
            },
        });
        access.revoked_connections.insert(connection_id.clone());
        statuses_tx
            .try_send(ConnectionStatus::Disconnected(connection_id.clone()))
            .unwrap();
        access.poll_statuses(Instant::now());
        assert!(
            access.revoked_connections.contains(&connection_id),
            "the revocation outlives the disconnect it caused"
        );
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

#[cfg(test)]
mod cockpit_tier_tests {
    use super::*;

    /// A profile no code path constructs is a tier nothing can reach.
    ///
    /// The seven `layout.*` capabilities shipped registered, catalogued and
    /// refused at the gate, because `configured_profile` knew only the
    /// observer and the annotator. Nothing failed: the registry was valid, the
    /// schema published, the catalog complete — and every call was denied. A
    /// capability set is not delivered until a connection can hold the ceiling
    /// it needs, so that is what this asserts.
    #[test]
    fn granting_the_cockpit_scopes_reaches_the_cockpit_profile() {
        let mut access = ControlAccess::new();
        access
            .configure_scopes("cockpit,cockpit.layout")
            .expect("the cockpit scopes are registered permissions");
        assert!(
            access.grants_cockpit(),
            "the floor and a scope together open the tier"
        );
        assert_eq!(
            access.configured_profile().as_str(),
            COCKPIT_PROFILE_ID,
            "the layout capabilities are refused under any lower ceiling"
        );
    }

    /// The floor alone opens nothing, and a scope without the floor opens
    /// nothing either — every cockpit capability requires both, so claiming
    /// the tier on half of it would put a grant on the panel that is refused
    /// on every call.
    #[test]
    fn half_the_cockpit_grant_is_not_the_cockpit_tier() {
        for scopes in ["cockpit", "cockpit.layout"] {
            let mut access = ControlAccess::new();
            access
                .configure_scopes(scopes)
                .expect("registered permission");
            assert!(
                !access.grants_cockpit(),
                "{scopes} alone must not open the tier"
            );
            assert_ne!(access.configured_profile().as_str(), COCKPIT_PROFILE_ID);
        }
    }

    /// The annotate tier is untouched by the new one: a trader who granted
    /// only the chart scopes gets exactly what that consent text describes.
    #[test]
    fn the_annotate_tier_does_not_pick_up_the_cockpit_ceiling() {
        let mut access = ControlAccess::new();
        access
            .configure_scopes("annotate,annotate.chart")
            .expect("registered permissions");
        assert!(access.grants_annotate());
        assert!(
            !access.grants_cockpit(),
            "annotate must not carry a grant to rearrange the window"
        );
        assert_eq!(access.configured_profile().as_str(), ANNOTATOR_PROFILE_ID);
    }

    /// The tiers have to **nest**, not merely coexist.
    ///
    /// `handshake::authorize` intersects the requested and the granted ceiling
    /// and then demands that one of the two *be* that intersection — "profiles
    /// are built nested" is the comment it says so under. Two sibling write
    /// tiers overlap without nesting, and an incomparable pair is refused
    /// outright: a client asking for `--profile annotator` against a cockpit
    /// grant could not connect at all.
    #[test]
    fn the_cockpit_ceiling_contains_the_annotator_ceiling() {
        use quantick_control::handshake::ProfileAuthority;

        let access = ControlAccess::new();
        let registry = access.contract.registry();
        let ceiling = |id: &str| {
            registry
                .permission_ceiling(&ProfileId::new(id).expect("static profile ID is valid"))
                .expect("the profile is registered")
        };
        assert!(
            ceiling(ANNOTATOR_PROFILE_ID).is_subset(&ceiling(COCKPIT_PROFILE_ID)),
            "the two write tiers are siblings, so the handshake refuses any \
             connection that names one against a grant holding the other"
        );
    }

    /// A trader who grants both tiers keeps both.
    ///
    /// The connection takes one ceiling, and every scope the human ticked has
    /// to survive it — `authorize` drops a granted scope the ceiling does not
    /// hold. Under a cockpit ceiling that was a sibling of the annotator, that
    /// meant ticking "rearrange my charts" silently switched off "answer on
    /// the chart".
    #[test]
    fn granting_both_tiers_keeps_every_scope_the_trader_ticked() {
        use quantick_control::handshake::ProfileAuthority;

        let mut access = ControlAccess::new();
        access
            .configure_scopes("annotate,annotate.chart,cockpit,cockpit.layout")
            .expect("registered permissions");
        assert!(access.grants_annotate());
        assert!(access.grants_cockpit());

        let profile = access.configured_profile();
        let ceiling = access
            .contract
            .registry()
            .permission_ceiling(&profile)
            .expect("the configured profile is registered");
        for scope in &access.configured_scopes {
            assert!(
                ceiling.contains(scope),
                "the {profile} ceiling drops the granted scope {scope}"
            );
        }
    }
}
