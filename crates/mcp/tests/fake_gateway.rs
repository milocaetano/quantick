//! The adapter end to end against a second implementation of the gateway
//! side: a loopback listener that publishes a real descriptor, accepts the
//! contract's handshake with the reference registry as its profile authority,
//! and answers framed requests with canned observer documents. The real
//! discovery, the real `LocalClient`, the real `LocalLink` and the real
//! `McpServer` run over it — only the application is replaced.

use std::{
    collections::BTreeSet,
    io::Write as _,
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use quantick_control::{
    codec::{BoundedCodec, CodecError, FrameRole},
    descriptor::{
        INSTANCE_DESCRIPTOR_HOST, INSTANCE_DESCRIPTOR_TRANSPORT, INSTANCE_DESCRIPTOR_VERSION,
        InstanceDescriptor,
    },
    error::{ControlError, codes},
    fake::reference_registry,
    handshake::{
        BearerToken, CURRENT_PROTOCOL_VERSION, HandshakeGrant, HandshakeReply, ProtocolLimits,
        ProtocolVersionRange, accept_handshake,
    },
    id::{ConnectionId, ErrorCode, InstanceId, PermissionId, PrincipalId, ProcessNonce, ProfileId},
    wire::{RequestEnvelope, ResponseEnvelope, ResponseOutcome, WireU64},
};
use quantick_control_local::{
    client::ConnectOptions,
    discovery::{PublishedDescriptor, publish_descriptor_in},
};
use quantick_mcp::{link::LocalLink, server::McpServer, tools};
use serde_json::{Value, json};

/// One fake instance: a listener, its published descriptor, and the thread
/// that serves connections until `stop` is raised.
struct FakeGateway {
    instance_id: InstanceId,
    stop: Arc<AtomicBool>,
    _published: PublishedDescriptor,
    port: u16,
}

impl FakeGateway {
    fn start(directory: &Path, seed: u8, published_at_unix_ms: i64) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let instance_id = InstanceId::from_bytes([seed; 16]);
        let process_nonce = ProcessNonce::from_bytes([seed.wrapping_add(1); 16]);
        let token = BearerToken::from_bytes([seed.wrapping_add(2); 32]);
        let descriptor = InstanceDescriptor {
            descriptor_version: INSTANCE_DESCRIPTOR_VERSION,
            instance_id: instance_id.clone(),
            process_nonce: process_nonce.clone(),
            process_id: std::process::id(),
            process_started_at_unix_ms: published_at_unix_ms - 1_000,
            application_version: "0.1.0-fake".to_owned(),
            application_commit: "fake".to_owned(),
            protocol_versions: ProtocolVersionRange::new(
                CURRENT_PROTOCOL_VERSION,
                CURRENT_PROTOCOL_VERSION,
            )
            .unwrap(),
            transport: INSTANCE_DESCRIPTOR_TRANSPORT.to_owned(),
            host: INSTANCE_DESCRIPTOR_HOST.to_owned(),
            port,
            bearer_token: token.clone(),
            published_at_unix_ms,
        };
        let published = publish_descriptor_in(directory, &descriptor).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let serving_stop = Arc::clone(&stop);
        let serving_id = instance_id.clone();
        thread::spawn(move || {
            while !serving_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let id = serving_id.clone();
                        let nonce = process_nonce.clone();
                        let token = token.clone();
                        thread::spawn(move || serve_connection(stream, id, nonce, token));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
        });
        Self {
            instance_id,
            stop,
            _published: published,
            port,
        }
    }
}

impl Drop for FakeGateway {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        // Nudge a blocked accept loop, then let the descriptor remove itself.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port));
    }
}

fn serve_connection(
    mut stream: TcpStream,
    instance_id: InstanceId,
    process_nonce: ProcessNonce,
    token: BearerToken,
) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let handshake_codec = BoundedCodec::handshake();
    let Ok(request) = handshake_codec.read_handshake_request(&mut stream) else {
        return;
    };
    let registry = reference_registry().unwrap();
    let grant = HandshakeGrant {
        protocol_versions: ProtocolVersionRange::new(
            CURRENT_PROTOCOL_VERSION,
            CURRENT_PROTOCOL_VERSION,
        )
        .unwrap(),
        instance_id: instance_id.clone(),
        process_nonce,
        bearer_token: token,
        connection_id: ConnectionId::from_bytes([7; 16]),
        principal_id: PrincipalId::from_bytes([8; 16]),
        application_version: "0.1.0-fake".to_owned(),
        application_commit: "fake".to_owned(),
        profile_ceiling: ProfileId::new("observer").unwrap(),
        granted_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
        limits: ProtocolLimits::default(),
    };
    let reply = match accept_handshake(&request, &grant, &registry) {
        Ok(accepted) => HandshakeReply::Accepted(accepted),
        Err(error) => HandshakeReply::Rejected { error },
    };
    let accepted = matches!(reply, HandshakeReply::Accepted(_));
    let frame = handshake_codec.encode(FrameRole::Response, &reply).unwrap();
    if stream.write_all(&frame).is_err() || !accepted {
        return;
    }
    let codec = BoundedCodec::default();
    loop {
        let request = match codec.read_request(&mut stream) {
            Ok(request) => request,
            Err(CodecError::IdleTimeout) => continue,
            Err(_) => return,
        };
        let outcome = answer(&instance_id, &request);
        let response = ResponseEnvelope {
            protocol_version: request.protocol_version,
            request_id: request.request_id.clone(),
            instance_id: instance_id.clone(),
            capture_revision: Some(WireU64::new(1)),
            module_revisions: Vec::new(),
            outcome,
            warnings: Vec::new(),
        };
        let frame = codec.encode(FrameRole::Response, &response).unwrap();
        if stream.write_all(&frame).is_err() {
            return;
        }
    }
}

fn answer(instance_id: &InstanceId, request: &RequestEnvelope) -> ResponseOutcome {
    match request.capability_id.as_str() {
        tools::DESCRIBE_CAPABILITY => ResponseOutcome::Success {
            result: json!({
                "instance_id": instance_id,
                "application_version": "0.1.0-fake",
                "capabilities": [
                    { "id": tools::DESCRIBE_CAPABILITY, "version": 1, "title": "Describe", "description": "Report the instance", "module": "control", "read_only": true, "availability": {"status": "available"} },
                    { "id": tools::SNAPSHOT_CAPABILITY, "version": 1, "title": "Snapshot", "description": "Coherent capture", "module": "snapshot", "read_only": true, "availability": {"status": "available"} }
                ],
                "snapshot_scopes": [
                    { "scope_id": "system.info", "module_id": "system", "title": "System", "description": "Build identity" }
                ]
            }),
        },
        tools::SNAPSHOT_CAPABILITY
        | tools::DIAGNOSTICS_CAPABILITY
        | tools::CHART_WINDOW_CAPABILITY => ResponseOutcome::Success {
            result: json!({ "echo": request.payload, "capability": request.capability_id }),
        },
        _ => ResponseOutcome::Failure {
            error: ControlError::new(
                ErrorCode::new(codes::CAPABILITY_UNKNOWN).unwrap(),
                "capability ID or version is not registered",
                false,
            ),
        },
    }
}

fn scratch_directory(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "quantick-mcp-gateway-{name}-{}-{}",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_dir_all(&directory);
    directory
}

fn server_over(directory: &Path) -> McpServer {
    let options = ConnectOptions::observer(
        "quantick-mcp test",
        "0",
        BTreeSet::from([PermissionId::new("observe").unwrap()]),
    );
    let link = LocalLink::new(options, Some(directory.to_path_buf()), None);
    let mut server = McpServer::new(Box::new(link), "observer");
    let reply = server
        .handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}).to_string())
        .unwrap();
    assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
    server
}

fn call(server: &mut McpServer, id: u64, name: &str, arguments: Value) -> Value {
    let reply = server
        .handle_line(
            &json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}})
                .to_string(),
        )
        .unwrap();
    assert!(
        reply.get("error").is_none(),
        "tool call {name} was a protocol error: {reply}"
    );
    reply["result"].clone()
}

#[test]
fn the_adapter_discovers_authenticates_and_reads_through_the_real_transport() {
    let directory = scratch_directory("one");
    let gateway = FakeGateway::start(&directory, 0x11, 1_700_000_000_000);
    let mut server = server_over(&directory);

    let listed = call(&mut server, 2, tools::DESCRIBE, json!({}));
    assert_eq!(listed["isError"], false);
    let instances = listed["structuredContent"]["instances"].as_array().unwrap();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0]["instance_id"], gateway.instance_id.to_string());
    assert_eq!(instances[0]["application_version"], "0.1.0-fake");

    let described = call(
        &mut server,
        3,
        tools::DESCRIBE,
        json!({ "instance_id": gateway.instance_id.to_string() }),
    );
    assert_eq!(described["isError"], false);
    assert_eq!(
        described["structuredContent"]["result"]["capabilities"][0]["id"],
        tools::DESCRIBE_CAPABILITY
    );
    assert_eq!(
        described["structuredContent"]["instance_id"],
        gateway.instance_id.to_string()
    );

    // One live instance: no routing id needed, and the routing id never
    // reaches the gateway's payload.
    let snapshot = call(
        &mut server,
        4,
        tools::GET_SNAPSHOT,
        json!({ "scopes": ["system.info"] }),
    );
    assert_eq!(snapshot["isError"], false);
    assert_eq!(
        snapshot["structuredContent"]["result"]["echo"],
        json!({ "scopes": ["system.info"] })
    );
    assert_eq!(snapshot["structuredContent"]["capture_revision"], "1");

    let searched = call(
        &mut server,
        5,
        tools::SEARCH_CAPABILITIES,
        json!({ "query": "snapshot" }),
    );
    assert_eq!(searched["structuredContent"]["capability_count"], 1);
    assert_eq!(
        searched["structuredContent"]["capabilities"][0]["id"],
        tools::SNAPSHOT_CAPABILITY
    );

    let refused = call(
        &mut server,
        6,
        tools::INVOKE,
        json!({ "capability_id": "paper.order.place", "payload": {} }),
    );
    assert_eq!(refused["isError"], true);
    assert_eq!(
        refused["structuredContent"]["error"]["code"],
        "control.capability_unknown"
    );

    let elsewhere = call(
        &mut server,
        7,
        tools::GET_DIAGNOSTICS,
        json!({ "instance_id": InstanceId::from_bytes([0x99; 16]).to_string() }),
    );
    assert_eq!(elsewhere["isError"], true);
    assert_eq!(
        elsewhere["structuredContent"]["error"]["code"],
        "control.instance_gone"
    );

    drop(gateway);
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn two_live_instances_are_listed_in_order_and_never_chosen_silently() {
    let directory = scratch_directory("two");
    let later = FakeGateway::start(&directory, 0x22, 1_700_000_000_500);
    let earlier = FakeGateway::start(&directory, 0x33, 1_700_000_000_100);
    let mut server = server_over(&directory);

    let listed = call(&mut server, 2, tools::DESCRIBE, json!({}));
    let instances = listed["structuredContent"]["instances"].as_array().unwrap();
    assert_eq!(instances.len(), 2);
    assert_eq!(
        instances[0]["instance_id"],
        earlier.instance_id.to_string(),
        "published_at_unix_ms orders the list, not the file system"
    );
    assert_eq!(instances[1]["instance_id"], later.instance_id.to_string());

    let ambiguous = call(&mut server, 3, tools::GET_DIAGNOSTICS, json!({}));
    assert_eq!(ambiguous["isError"], true);
    assert_eq!(
        ambiguous["structuredContent"]["error"]["code"],
        "control.instance_ambiguous"
    );
    assert_eq!(
        ambiguous["structuredContent"]["error"]["details"]["instance_ids"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    let chosen = call(
        &mut server,
        4,
        tools::GET_DIAGNOSTICS,
        json!({ "instance_id": later.instance_id.to_string() }),
    );
    assert_eq!(chosen["isError"], false);
    assert_eq!(
        chosen["structuredContent"]["instance_id"],
        later.instance_id.to_string()
    );

    drop(later);
    drop(earlier);
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn a_pinned_adapter_refuses_a_contradicting_routing_id() {
    let directory = scratch_directory("pinned");
    let gateway = FakeGateway::start(&directory, 0x44, 1_700_000_000_000);
    let options = ConnectOptions::observer(
        "quantick-mcp test",
        "0",
        BTreeSet::from([PermissionId::new("observe").unwrap()]),
    );
    let link = LocalLink::new(
        options,
        Some(directory.clone()),
        Some(gateway.instance_id.clone()),
    );
    let mut server = McpServer::new(Box::new(link), "observer");
    server
        .handle_line(&json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}).to_string())
        .unwrap();
    let pinned = call(&mut server, 2, tools::GET_DIAGNOSTICS, json!({}));
    assert_eq!(pinned["isError"], false);
    let other = call(
        &mut server,
        3,
        tools::GET_DIAGNOSTICS,
        json!({ "instance_id": InstanceId::from_bytes([0x55; 16]).to_string() }),
    );
    assert_eq!(other["isError"], true);
    assert_eq!(
        other["structuredContent"]["error"]["code"],
        "control.invalid_request"
    );
    drop(gateway);
    let _ = std::fs::remove_dir_all(&directory);
}
