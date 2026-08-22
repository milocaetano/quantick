//! Blocking loopback client for one running Quantick gateway.
//!
//! This is the client half of the handshake and framing that
//! `quantick-control` defines and the application's gateway serves. An adapter
//! (the MCP server first, a CLI later) discovers live instances through
//! [`discover`], selects exactly one through [`LiveInstances::select`], and
//! then exchanges framed envelopes through [`LocalClient`]. Nothing here can
//! start the application: with no descriptor there is no instance, and the
//! report says so with a next step instead of inventing one (ADR 0001 §1).

use std::{
    collections::BTreeSet,
    io::Write as _,
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    path::Path,
    time::Duration,
};

use quantick_control::{
    codec::{BoundedCodec, FrameRole},
    descriptor::InstanceDescriptor,
    error::{ControlError, codes},
    handshake::{
        CURRENT_PROTOCOL_VERSION, HandshakeRequest, HandshakeResponse, ProtocolVersionRange,
    },
    id::{CapabilityId, ErrorCode, InstanceId, PermissionId, ProfileId, RequestId},
    limits::{CONTROL_HANDSHAKE_TIMEOUT_MS, CONTROL_REQUEST_ID_MAX_BYTES},
    wire::{RequestEnvelope, ResponseEnvelope},
};
use serde_json::Value;

use crate::discovery::{DescriptorDiscovery, DiscoveryError, discover_descriptors_in};

/// Capability version every first-generation observer read is registered at.
const FIRST_CAPABILITY_VERSION: u32 = 1;

/// Slack added to the gateway's own request timeout before the client gives up
/// reading, so the gateway's structured `control.timeout` reply is received
/// rather than raced: the gateway times a request out at its deadline and then
/// still has to write the error frame.
const RESPONSE_READ_SLACK_MS: u64 = CONTROL_HANDSHAKE_TIMEOUT_MS;

/// What a client declares about itself and asks for when it connects.
///
/// `client_name` and `client_version` are self-declared metadata the gateway
/// shows a human as such; they are never an identity (contract §6). The
/// requested profile and scopes are a ceiling the gateway intersects with the
/// user's grant, never a claim.
#[derive(Clone, Debug)]
pub struct ConnectOptions {
    pub client_name: String,
    pub client_version: String,
    pub requested_profile: ProfileId,
    pub requested_scopes: BTreeSet<PermissionId>,
}

impl ConnectOptions {
    /// An observer connection: the default and, today, the only profile the
    /// gateway grants.
    pub fn observer(
        client_name: impl Into<String>,
        client_version: impl Into<String>,
        requested_scopes: BTreeSet<PermissionId>,
    ) -> Self {
        Self {
            client_name: client_name.into(),
            client_version: client_version.into(),
            requested_profile: ProfileId::new("observer")
                .expect("static observer profile is valid"),
            requested_scopes,
        }
    }
}

/// One authenticated connection to one running instance.
#[derive(Debug)]
pub struct LocalClient {
    descriptor: InstanceDescriptor,
    handshake: HandshakeResponse,
    stream: TcpStream,
    codec: BoundedCodec,
    request_prefix: String,
    next_request: u64,
}

impl LocalClient {
    /// Connect to the endpoint a descriptor advertises and authenticate with
    /// its bearer token. Every failure before the handshake is accepted is a
    /// structured error: a closed port or stale descriptor is
    /// `control.instance_gone`, and a rejected handshake carries the code the
    /// gateway sent back (`control.auth_failed`, `control.version_unsupported`,
    /// `control.permission_denied`, …).
    pub fn connect(
        descriptor: InstanceDescriptor,
        options: &ConnectOptions,
    ) -> Result<Self, ControlError> {
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, descriptor.port);
        let handshake_timeout = Duration::from_millis(CONTROL_HANDSHAKE_TIMEOUT_MS);
        let mut stream = TcpStream::connect_timeout(&address.into(), handshake_timeout)
            .map_err(|_| instance_gone_error())?;
        stream
            .set_read_timeout(Some(handshake_timeout))
            .map_err(|_| instance_gone_error())?;
        stream
            .set_write_timeout(Some(handshake_timeout))
            .map_err(|_| instance_gone_error())?;

        let handshake_codec = BoundedCodec::handshake();
        // The range this client implements — not the one the instance
        // advertises, or negotiation could pick a version this build does
        // not speak and the range check would be vacuous. An instance that
        // advertises no common version is refused before the handshake, with
        // the same code the gateway would answer.
        let own_versions =
            ProtocolVersionRange::new(CURRENT_PROTOCOL_VERSION, CURRENT_PROTOCOL_VERSION)
                .expect("the current protocol range is valid");
        if descriptor
            .protocol_versions
            .negotiate(own_versions)
            .is_none()
        {
            return Err(known_error(
                codes::VERSION_UNSUPPORTED,
                "the instance advertises protocol versions this client does not implement",
                false,
            ));
        }
        let request = HandshakeRequest {
            protocol_versions: own_versions,
            instance_id: descriptor.instance_id.clone(),
            client_name: options.client_name.clone(),
            client_version: options.client_version.clone(),
            bearer_token: descriptor.bearer_token.clone(),
            requested_profile: options.requested_profile.clone(),
            requested_scopes: options.requested_scopes.clone(),
        };
        let frame = handshake_codec
            .encode(FrameRole::Request, &request)
            .map_err(|_| ControlError::invalid_request("handshake request encoding failed"))?;
        stream
            .write_all(&frame)
            .map_err(|_| instance_gone_error())?;
        let reply = handshake_codec
            .read_handshake_reply(&mut stream)
            .map_err(|_| instance_gone_error())?;
        let handshake = reply.into_accepted()?;
        handshake.validate_for(&request, &descriptor.process_nonce)?;

        let response_timeout = Duration::from_millis(
            handshake
                .effective_limits
                .request_timeout_ms
                .saturating_add(RESPONSE_READ_SLACK_MS),
        );
        stream
            .set_read_timeout(Some(response_timeout))
            .map_err(|_| instance_gone_error())?;
        stream
            .set_write_timeout(Some(response_timeout))
            .map_err(|_| instance_gone_error())?;

        // Request IDs are opaque to the server but must be unique per
        // connection while in flight; a per-connection prefix keeps them
        // readable in a log and bounded by the contract's byte limit.
        let request_prefix = format!("{}-", handshake.connection_id);
        Ok(Self {
            descriptor,
            handshake,
            stream,
            codec: BoundedCodec::default(),
            request_prefix,
            next_request: 1,
        })
    }

    /// The descriptor this connection was opened from.
    pub fn descriptor(&self) -> &InstanceDescriptor {
        &self.descriptor
    }

    /// The accepted handshake: negotiated protocol version, effective profile,
    /// scopes and limits.
    pub fn handshake(&self) -> &HandshakeResponse {
        &self.handshake
    }

    /// The scopes the gateway actually granted, which may be fewer than asked.
    pub fn effective_scopes(&self) -> &BTreeSet<PermissionId> {
        &self.handshake.effective_scopes
    }

    /// Send one request without waiting for its reply. Returns the request ID
    /// to correlate the reply with; a connection multiplexes bounded in-flight
    /// work, so replies may arrive in any order.
    pub fn send(&mut self, capability_id: &str, payload: Value) -> Result<RequestId, ControlError> {
        self.send_versioned(capability_id, FIRST_CAPABILITY_VERSION, payload)
    }

    /// [`Self::send`] for an explicit capability version.
    pub fn send_versioned(
        &mut self,
        capability_id: &str,
        capability_version: u32,
        payload: Value,
    ) -> Result<RequestId, ControlError> {
        let mut raw = format!("{}{}", self.request_prefix, self.next_request);
        raw.truncate(CONTROL_REQUEST_ID_MAX_BYTES);
        let request_id = RequestId::new(raw)
            .map_err(|error| ControlError::invalid_request(error.to_string()))?;
        self.next_request = self.next_request.saturating_add(1);
        self.send_with_request_id(request_id, capability_id, capability_version, payload)
    }

    /// Send one request under a correlation ID the caller chose. The ID is
    /// opaque to the gateway, but it must be unique while in flight on this
    /// connection: the gateway refuses a duplicate with
    /// `control.invalid_request` until the first reply has been sent.
    pub fn send_with_request_id(
        &mut self,
        request_id: RequestId,
        capability_id: &str,
        capability_version: u32,
        payload: Value,
    ) -> Result<RequestId, ControlError> {
        let request = RequestEnvelope {
            protocol_version: self.handshake.protocol_version,
            request_id: request_id.clone(),
            instance_id: self.descriptor.instance_id.clone(),
            capability_id: CapabilityId::new(capability_id)
                .map_err(|error| ControlError::invalid_request(error.to_string()))?,
            capability_version,
            expected_revisions: Vec::new(),
            idempotency_key: None,
            dry_run: false,
            reason: None,
            payload,
        };
        // A malformed envelope closes the connection on the gateway's side
        // (a framed request that fails validation is not a request); refuse
        // it here, structured, before it leaves.
        request.validate()?;
        let frame = self
            .codec
            .encode(FrameRole::Request, &request)
            .map_err(|_| ControlError::invalid_request("request encoding failed"))?;
        self.stream
            .write_all(&frame)
            .map_err(|_| instance_gone_error())?;
        Ok(request_id)
    }

    /// Read the next reply on the connection, whichever request it answers.
    pub fn read(&mut self) -> Result<ResponseEnvelope, ControlError> {
        self.codec
            .read_response(&mut self.stream)
            .map_err(|_| instance_gone_error())
    }

    /// Send one request and wait for its own reply. Use this when nothing
    /// else is in flight on the connection.
    pub fn invoke(
        &mut self,
        capability_id: &str,
        payload: Value,
    ) -> Result<ResponseEnvelope, ControlError> {
        let request_id = self.send(capability_id, payload)?;
        let response = self.read()?;
        if response.request_id != request_id {
            return Err(ControlError::invalid_request(
                "multiplexed response correlated to a different request",
            ));
        }
        Ok(response)
    }
}

/// The live instances a discovery pass authenticated against, in the ADR's
/// deterministic `(published_at_unix_ms, instance_id)` order, plus the
/// descriptors that were found but could not be used.
#[derive(Debug)]
pub struct LiveInstances {
    pub clients: Vec<LocalClient>,
    pub issues: Vec<ControlError>,
    pub next_steps: Vec<String>,
}

impl LiveInstances {
    /// The IDs of the live instances, in report order.
    pub fn instance_ids(&self) -> Vec<InstanceId> {
        self.clients
            .iter()
            .map(|client| client.descriptor.instance_id.clone())
            .collect()
    }

    /// Pick one connection. With exactly one live instance and no preference
    /// it is selected; with none the result is `control.instance_gone` carrying
    /// the report's next steps; with more than one and no preference it is
    /// `control.instance_ambiguous` listing the choices — never the newest
    /// window by default (ADR 0001 §5).
    pub fn select(mut self, instance_id: Option<&InstanceId>) -> Result<LocalClient, ControlError> {
        if let Some(instance_id) = instance_id {
            let position = self
                .clients
                .iter()
                .position(|client| &client.descriptor.instance_id == instance_id)
                .ok_or_else(instance_gone_error)?;
            return Ok(self.clients.remove(position));
        }
        match self.clients.len() {
            0 => {
                let mut error = instance_gone_error();
                error.context.next_steps = self.next_steps;
                Err(error)
            }
            1 => Ok(self.clients.remove(0)),
            _ => {
                let choices = self
                    .instance_ids()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                let mut error = known_error(
                    codes::INSTANCE_AMBIGUOUS,
                    "more than one live Quantick instance is available",
                    false,
                );
                error.context.details = Some(serde_json::json!({ "instance_ids": choices }));
                error.context.next_steps =
                    vec!["Choose one instance_id explicitly and retry discovery.".to_owned()];
                Err(error)
            }
        }
    }
}

/// Discover and authenticate against every live instance in the platform's
/// private runtime directory. Creates nothing and starts nothing.
pub fn discover(options: &ConnectOptions) -> Result<LiveInstances, DiscoveryError> {
    let report = crate::discovery::discover_descriptors()?;
    Ok(connect_candidates(report, options))
}

/// [`discover`] over an explicit directory, for tests and tools that point at
/// a descriptor directory of their own.
pub fn discover_in(
    directory: &Path,
    options: &ConnectOptions,
) -> Result<LiveInstances, DiscoveryError> {
    let report = discover_descriptors_in(directory)?;
    Ok(connect_candidates(report, options))
}

fn connect_candidates(report: DescriptorDiscovery, options: &ConnectOptions) -> LiveInstances {
    let mut clients = Vec::new();
    let mut issues = report
        .issues
        .into_iter()
        .map(|issue| {
            ControlError::invalid_request(format!(
                "descriptor {} was rejected: {}",
                issue.file_name, issue.message
            ))
        })
        .collect::<Vec<_>>();
    for candidate in report.candidates {
        match LocalClient::connect(candidate.descriptor, options) {
            Ok(client) => clients.push(client),
            Err(error) => issues.push(error),
        }
    }
    // The report is already ordered; connecting preserves it, but a client
    // that was appended out of order by a failed predecessor must not change
    // the contract's deterministic listing.
    clients.sort_by(|left, right| {
        (
            left.descriptor.published_at_unix_ms,
            &left.descriptor.instance_id,
        )
            .cmp(&(
                right.descriptor.published_at_unix_ms,
                &right.descriptor.instance_id,
            ))
    });
    LiveInstances {
        clients,
        issues,
        next_steps: report.next_steps,
    }
}

/// The error every unreachable, closed or stale endpoint collapses to. A
/// connection failure alone never says *why*; the next step tells the user
/// what to check.
pub fn instance_gone_error() -> ControlError {
    let mut error = known_error(
        codes::INSTANCE_GONE,
        "the advertised Quantick instance is no longer reachable",
        true,
    );
    error.context.next_steps = vec![
        "Verify that Quantick is open and local agent access is enabled, then rediscover instances."
            .to_owned(),
    ];
    error
}

fn known_error(code: &str, message: &str, retryable: bool) -> ControlError {
    ControlError::new(
        ErrorCode::new(code).expect("static error code is valid"),
        message,
        retryable,
    )
}

#[cfg(test)]
mod tests {
    use std::{io::Read as _, net::TcpListener, sync::mpsc, thread};

    use quantick_control::{
        handshake::{
            BearerToken, CURRENT_PROTOCOL_VERSION, HandshakeReply, ProtocolLimits,
            ProtocolVersionRange,
        },
        id::{ConnectionId, ProcessNonce},
    };

    use super::*;

    fn options() -> ConnectOptions {
        ConnectOptions::observer(
            "client under test",
            "0.0.0",
            BTreeSet::from([PermissionId::new("observe").unwrap()]),
        )
    }

    fn descriptor_for(port: u16) -> InstanceDescriptor {
        InstanceDescriptor {
            descriptor_version: quantick_control::descriptor::INSTANCE_DESCRIPTOR_VERSION,
            instance_id: InstanceId::from_bytes([1; 16]),
            process_nonce: ProcessNonce::from_bytes([2; 16]),
            process_id: 4242,
            process_started_at_unix_ms: 1_700_000_000_000,
            application_version: "0.1.0".to_owned(),
            application_commit: "abc123".to_owned(),
            protocol_versions: ProtocolVersionRange::new(
                CURRENT_PROTOCOL_VERSION,
                CURRENT_PROTOCOL_VERSION,
            )
            .unwrap(),
            transport: quantick_control::descriptor::INSTANCE_DESCRIPTOR_TRANSPORT.to_owned(),
            host: quantick_control::descriptor::INSTANCE_DESCRIPTOR_HOST.to_owned(),
            port,
            bearer_token: BearerToken::from_bytes([7; 32]),
            published_at_unix_ms: 1_700_000_000_500,
        }
    }

    /// A second implementation of the server side of the handshake: a loopback
    /// listener that accepts one connection, reads the handshake frame, and
    /// answers with the reply the test chose. It proves the client speaks the
    /// contract's framing against something that is not the application.
    fn fake_gateway(reply: HandshakeReply) -> (u16, mpsc::Receiver<HandshakeRequest>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (seen_tx, seen_rx) = mpsc::channel();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let codec = BoundedCodec::handshake();
            let request = codec.read_handshake_request(&mut stream).unwrap();
            let frame = codec.encode(FrameRole::Response, &reply).unwrap();
            stream.write_all(&frame).unwrap();
            seen_tx.send(request).unwrap();
            // Hold the socket open until the client is done with it.
            let mut sink = [0u8; 1];
            let _ = stream.read(&mut sink);
        });
        (port, seen_rx)
    }

    fn accepted_reply(request_nonce: &ProcessNonce) -> HandshakeReply {
        HandshakeReply::Accepted(HandshakeResponse {
            protocol_version: CURRENT_PROTOCOL_VERSION,
            instance_id: InstanceId::from_bytes([1; 16]),
            process_nonce: request_nonce.clone(),
            connection_id: ConnectionId::from_bytes([3; 16]),
            application_version: "0.1.0".to_owned(),
            application_commit: "abc123".to_owned(),
            effective_profile: ProfileId::new("observer").unwrap(),
            effective_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
            effective_limits: ProtocolLimits::default(),
        })
    }

    #[test]
    fn the_client_sends_the_descriptor_token_and_keeps_the_accepted_handshake() {
        let (port, seen) = fake_gateway(accepted_reply(&ProcessNonce::from_bytes([2; 16])));
        let client = LocalClient::connect(descriptor_for(port), &options()).unwrap();
        let request = seen.recv().unwrap();
        assert_eq!(request.bearer_token, BearerToken::from_bytes([7; 32]));
        assert_eq!(request.client_name, "client under test");
        assert_eq!(request.requested_profile.as_str(), "observer");
        assert_eq!(
            client.effective_scopes(),
            &BTreeSet::from([PermissionId::new("observe").unwrap()])
        );
        assert_eq!(
            client.handshake().protocol_version,
            CURRENT_PROTOCOL_VERSION
        );
    }

    #[test]
    fn a_rejected_handshake_surfaces_the_gateway_error_code() {
        let (port, _seen) = fake_gateway(HandshakeReply::Rejected {
            error: known_error(codes::AUTH_FAILED, "no", false),
        });
        let error = LocalClient::connect(descriptor_for(port), &options()).unwrap_err();
        assert_eq!(error.code.as_str(), codes::AUTH_FAILED);
    }

    #[test]
    fn a_reply_for_another_process_is_refused_before_any_request() {
        // The descriptor promised nonce [2; 16]; the gateway answers as [9; 16].
        let (port, _seen) = fake_gateway(accepted_reply(&ProcessNonce::from_bytes([9; 16])));
        assert!(LocalClient::connect(descriptor_for(port), &options()).is_err());
    }

    #[test]
    fn a_closed_port_is_instance_gone_with_a_next_step() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let error = LocalClient::connect(descriptor_for(port), &options()).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INSTANCE_GONE);
        assert!(error.retryable);
        assert!(!error.context.next_steps.is_empty());
    }

    #[test]
    fn selection_never_picks_silently_among_many_and_says_so_when_there_are_none() {
        let none = LiveInstances {
            clients: Vec::new(),
            issues: Vec::new(),
            next_steps: vec!["start it".to_owned()],
        };
        let error = none.select(None).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INSTANCE_GONE);
        assert_eq!(error.context.next_steps, vec!["start it".to_owned()]);

        let (first_port, _a) = fake_gateway(accepted_reply(&ProcessNonce::from_bytes([2; 16])));
        let (second_port, _b) = fake_gateway(accepted_reply(&ProcessNonce::from_bytes([2; 16])));
        let first = LocalClient::connect(descriptor_for(first_port), &options()).unwrap();
        let mut second_descriptor = descriptor_for(second_port);
        second_descriptor.instance_id = InstanceId::from_bytes([5; 16]);
        second_descriptor.published_at_unix_ms += 1;
        let mut second_options = options();
        second_options.client_name = "second".to_owned();
        // The fake gateway answers for instance [1; 16]; skip the identity
        // check here by connecting with a matching descriptor and relabelling
        // afterwards — selection is what is under test, not the handshake.
        let mut second =
            LocalClient::connect(descriptor_for(second_port), &second_options).unwrap();
        second.descriptor = second_descriptor;
        let many = LiveInstances {
            clients: vec![first, second],
            issues: Vec::new(),
            next_steps: Vec::new(),
        };
        assert_eq!(many.instance_ids().len(), 2);
        let error = many.select(None).unwrap_err();
        assert_eq!(error.code.as_str(), codes::INSTANCE_AMBIGUOUS);
        assert_eq!(
            error.context.details.unwrap()["instance_ids"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn an_empty_directory_discovers_nothing_and_creates_nothing() {
        let directory = std::env::temp_dir().join(format!(
            "quantick-control-local-empty-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let live = discover_in(&directory, &options()).unwrap();
        assert!(live.clients.is_empty());
        assert!(!live.next_steps.is_empty());
        assert!(
            !directory.exists(),
            "discovery must not create the directory"
        );
    }
}
