//! The gateway thread: the socket, the accept loop, and the dispatchers.
//!
//! Everything in this file runs away from the UI thread. It owns the listener,
//! authenticates each connection, rate-limits it, turns a request into either a
//! worker-thread answer or a bounded hand-off to the UI, parks the waits that
//! cannot be answered yet, and encodes every response. It never draws, and the
//! host never touches a socket: that thread boundary is the seam this
//! file was cut along, and an auditor asking what a client can put on the wire
//! reads this file whole.

use std::io::Write as _;
use std::net::TcpListener;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;

use crossbeam_channel::{TryRecvError, TrySendError};

// The host's scope, in one hop. Everything this file names -- the options and
// runtime structs, the protocol limits, the codec, the journal -- is already
// bound there, and a glob keeps the moved bodies byte-identical to what they
// were when they lived in `gateway.rs`.
use super::*;

/// Microseconds spent since `started`, saturating.
///
/// One reading for everything that shares the frame budget, so the drain and
/// the captures that run before it are measuring the same thing.
pub(super) fn elapsed_us_since(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX)
}

pub(super) fn drain_bounded_since<T>(
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

pub(super) fn gateway_main(
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

pub(super) fn activity_status_high_watermark(max_connections: usize) -> usize {
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

pub(super) fn try_reserve_in_flight(counter: &AtomicUsize, limit: usize) -> bool {
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

pub(super) fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
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
