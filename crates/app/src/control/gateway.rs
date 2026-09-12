//! Explicitly enabled authenticated loopback gateway and UI-thread dispatcher.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    net::TcpStream,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, bounded};
use quantick_control::{
    error::{ControlError, codes},
    id::{ConnectionId, InstanceId, PermissionId, PrincipalId, ProcessNonce, ProfileId, RequestId},
    limits::{
        CONTROL_CLIENT_BURST, CONTROL_CLIENT_RATE_PER_SECOND, CONTROL_HANDSHAKE_TIMEOUT_MS,
        CONTROL_MAX_CONNECTIONS, CONTROL_MAX_IN_FLIGHT_PER_CONNECTION,
        CONTROL_REQUEST_QUEUE_CAPACITY, CONTROL_REQUEST_TIMEOUT_MS, CONTROL_RUNTIME_ID_BYTES,
        CONTROL_UI_BUDGET_US, CONTROL_UI_MAX_REQUESTS_PER_FRAME,
    },
    wire::{ActorContext, ActorKind, RequestEnvelope},
};

use crate::{app::QuantickApp, metrics};

use super::{
    actions::{ANNOTATE_PERMISSION_ID, ANNOTATOR_PROFILE_ID, ActionRegistry, standard_actions},
    contract::{COCKPIT_PERMISSION_ID, COCKPIT_PROFILE_ID},
    contract::{
        DeferredActionResult, OBSERVE_PERMISSION_ID, OBSERVER_PROFILE_ID, ObserverContract,
        PreparedDispatch, PreparedRequest, UiReadContext, UiReadExecution,
    },
    evidence,
    evidence::{EvidenceStore, RawScreenshot, SessionIdentity},
    journal::{EventJournal, JournalSignal},
    notify::NotificationLimiter,
    registry::ProjectionRegistry,
    types::known_error,
};

mod idempotency;
mod local_action;
mod panel;
mod screenshot;
mod semantic;
mod server;
mod trace_replay;

use semantic::SemanticBaseline;
use trace_replay::TraceReinjection;
// `runtime_id_bytes` is re-exported: `control/mod.rs` reaches it by path.
pub(crate) use server::runtime_id_bytes;
use server::{drain_bounded_since, gateway_main, random_bytes};

const GATEWAY_COMMAND_CAPACITY: usize = 64;
const GATEWAY_STATUS_CAPACITY: usize = 256;
const GATEWAY_LIFECYCLE_CAPACITY: usize = 8;
const GATEWAY_CRITICAL_STATUS_SLOTS_PER_CONNECTION: usize = 2;
const CONTROL_UI_MAX_STATUS_UPDATES_PER_FRAME: usize = 32;
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
    /// Set once this request is past every pre-dispatch refusal, so the
    /// response worker can tell "never ran" from "may have run" when it stops
    /// hearing back. `gateway/idempotency.rs` is what needs the difference.
    started: Arc<AtomicBool>,
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

    #[cfg(test)]
    pub fn journal(&self) -> &EventJournal {
        &self.journal
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
        // Past every refusal that can happen without touching the
        // application: from here the request may really act.
        request.started.store(true, Ordering::Release);
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

#[cfg(test)]
mod tests;
