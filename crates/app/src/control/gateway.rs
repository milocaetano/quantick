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
    descriptor::{
        INSTANCE_DESCRIPTOR_HOST, INSTANCE_DESCRIPTOR_TRANSPORT, INSTANCE_DESCRIPTOR_VERSION,
        InstanceDescriptor,
    },
    error::{ControlError, codes},
    handshake::{
        BearerToken, CURRENT_PROTOCOL_VERSION, HandshakeGrant, HandshakeReply, ProtocolLimits,
        ProtocolVersionRange, accept_handshake,
    },
    id::{ConnectionId, InstanceId, PermissionId, PrincipalId, ProcessNonce, ProfileId},
    limits::{
        CONTROL_CLIENT_BURST, CONTROL_CLIENT_RATE_PER_SECOND, CONTROL_HANDSHAKE_TIMEOUT_MS,
        CONTROL_MAX_BUFFERED_RESPONSE_SLOTS, CONTROL_MAX_CONNECTIONS,
        CONTROL_MAX_IN_FLIGHT_PER_CONNECTION, CONTROL_REQUEST_QUEUE_CAPACITY,
        CONTROL_REQUEST_TIMEOUT_MS, CONTROL_RUNTIME_ID_BYTES, CONTROL_TOKEN_BYTES,
        CONTROL_UI_BUDGET_US, CONTROL_UI_MAX_REQUESTS_PER_FRAME,
    },
    wire::{ModuleRevision, RequestEnvelope, ResponseEnvelope, ResponseOutcome, WireU64},
};

use crate::{app::QuantickApp, metrics};

use super::{
    contract::{OBSERVER_PROFILE_ID, ObserverContract, PreparedRequest, UiReadExecution},
    registry::ProjectionRegistry,
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
const EXIT_SHUTDOWN_TIMEOUT_MS: u64 = 2_000;

#[derive(Clone)]
struct ProcessIdentity {
    instance_id: InstanceId,
    process_nonce: ProcessNonce,
    process_started_at_unix_ms: i64,
}

impl ProcessIdentity {
    fn generate() -> Result<Self, String> {
        Ok(Self {
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
            || self.request_timeout.is_zero()
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
}

impl ControlAccess {
    pub fn new() -> Self {
        let projections = super::standard_registry()
            .expect("built-in semantic projection registry must be valid");
        let contract = Arc::new(
            ObserverContract::new(&projections)
                .expect("built-in observer capability registry must be valid"),
        );
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
        }
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

        let configured_scopes = &self.configured_scopes;
        let revoked = &self.revoked_connections;
        let identity = self
            .identity
            .as_ref()
            .expect("an enabled gateway always has a process identity");
        let projections = &mut self.projections;
        self.last_drain = drain_bounded_since(&requests, frame_started, |request| {
            let result = execute_on_ui(
                projections,
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
            if self.last_drain.budget_exceeded {
                " (budget exceeded by one non-preemptible capture)"
            } else {
                ""
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
            AccessState::Disabled | AccessState::Enabling | AccessState::Disabling(None) => {}
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
fn drain_bounded<T>(receiver: &Receiver<T>, handle: impl FnMut(T)) -> DrainObservation {
    drain_bounded_since(receiver, Instant::now(), handle)
}

fn drain_bounded_since<T>(
    receiver: &Receiver<T>,
    started: Instant,
    mut handle: impl FnMut(T),
) -> DrainObservation {
    let mut processed = 0usize;
    while processed < CONTROL_UI_MAX_REQUESTS_PER_FRAME
        && u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX) < CONTROL_UI_BUDGET_US
    {
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
        budget_exceeded: elapsed_us > CONTROL_UI_BUDGET_US,
        queue_has_more: !receiver.is_empty(),
    }
}

fn execute_on_ui(
    projections: &mut ProjectionRegistry,
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
        .execute_ui(projections, app, &identity.instance_id)
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
                        reject_connection_capacity(stream, &authority.options);
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
    let _ = stream.set_write_timeout(Some(options.handshake_timeout));
    let reply = HandshakeReply::Rejected {
        error: known_error(
            codes::BACKPRESSURE,
            "local gateway connection capacity is full",
            true,
        ),
    };
    let codec = BoundedCodec::handshake();
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
    let frame = handshake_codec
        .encode(
            FrameRole::Response,
            &HandshakeReply::Accepted(accepted.clone()),
        )
        .map_err(|_| codes::AUTH_FAILED)?;
    stream.write_all(&frame).map_err(|_| codes::INSTANCE_GONE)?;
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
    if authority
        .commands
        .try_send(GatewayCommand::Identified {
            socket_key,
            connection_id: connection_id.clone(),
        })
        .is_err()
    {
        return Err(codes::BACKPRESSURE);
    }
    if authority.statuses.len() >= activity_status_high_watermark(authority.options.max_connections)
        || authority
            .statuses
            .try_send(ConnectionStatus::Connected(client))
            .is_err()
    {
        return Err(codes::BACKPRESSURE);
    }
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
        let observation = drain_bounded(&receiver, |value| handled.push(value));
        assert_eq!(handled.len(), CONTROL_UI_MAX_REQUESTS_PER_FRAME);
        assert_eq!(receiver.len(), 2);
        assert!(observation.queue_has_more);
    }

    #[test]
    fn a_capture_that_exhausts_the_budget_ends_the_frame_drain() {
        // The count ceiling is the deterministic guard; the elapsed-time
        // budget is the authoritative one (plan §10.2). Prove the latter on
        // its own: captures that each cost the whole budget must not run two
        // to a frame, although the count ceiling alone would allow four.
        let (sender, receiver) = bounded(4);
        for value in 0..4 {
            sender.send(value).unwrap();
        }
        let budget = Duration::from_micros(CONTROL_UI_BUDGET_US);
        let mut handled = 0usize;
        let observation = drain_bounded_since(&receiver, Instant::now(), |_| {
            handled += 1;
            let until = Instant::now() + budget;
            while Instant::now() < until {
                std::hint::spin_loop();
            }
        });
        assert_eq!(
            handled, 1,
            "the second capture must wait for the next frame"
        );
        assert_eq!(observation.processed, 1);
        assert!(observation.budget_exceeded);
        assert!(observation.queue_has_more);
        assert_eq!(receiver.len(), 3);
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
