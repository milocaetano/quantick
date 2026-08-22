use quantick_control::{
    codec::{BoundedCodec, CodecError, FrameRole},
    fake::COUNTER_READ,
    id::{CapabilityId, ConnectionId, InstanceId, PrincipalId, RequestId},
    schema::{generated_schema, validate_instance},
    wire::{
        ActorContext, ActorKind, AuthorizedRequest, RESERVED_ACTOR_FIELDS, RequestEnvelope,
        ResponseEnvelope, ResponseOutcome,
    },
};
use serde_json::json;

fn request() -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: 1,
        request_id: RequestId::new("codec-test").unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        capability_id: CapabilityId::new(COUNTER_READ).unwrap(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run: false,
        reason: None,
        payload: json!({}),
    }
}

#[test]
fn response_requires_exactly_one_result_or_error() {
    let base = json!({
        "protocol_version": 1,
        "request_id": "response-test",
        "instance_id": InstanceId::from_bytes([1; 16]),
        "module_revisions": [],
        "warnings": [],
        "result": {},
        "error": {
            "code": "control.invalid_request",
            "message": "invalid",
            "retryable": false
        }
    });
    assert!(serde_json::from_value::<ResponseEnvelope>(base.clone()).is_err());
    assert!(
        validate_instance(&generated_schema::<ResponseEnvelope>(), &base).is_err(),
        "the committed contract schema must enforce the same exclusivity"
    );
}

#[test]
fn external_actor_fields_are_rejected_instead_of_ignored() {
    let codec = BoundedCodec::default();
    let mut value = serde_json::to_value(request()).unwrap();
    value["principal_id"] = json!(InstanceId::from_bytes([9; 16]).as_str());
    let frame = codec.encode(FrameRole::Request, &value).unwrap();
    assert_eq!(
        codec.decode_request_frame(&frame),
        Err(CodecError::ReservedActorField)
    );
}

#[test]
fn exact_frame_roundtrip_rejects_truncation_and_trailing_data() {
    let codec = BoundedCodec::default();
    let frame = codec.encode(FrameRole::Request, &request()).unwrap();
    assert_eq!(codec.decode_request_frame(&frame).unwrap(), request());
    assert!(matches!(
        codec.decode_request_frame(&frame[..frame.len() - 1]),
        Err(CodecError::TruncatedPayload { .. })
    ));
    let mut trailing = frame;
    trailing.push(0);
    assert_eq!(
        codec.decode_request_frame(&trailing),
        Err(CodecError::TrailingBytes(1))
    );
}

#[test]
fn a_client_rejects_a_valid_response_for_a_different_request() {
    let request = request();
    let response = ResponseEnvelope {
        protocol_version: request.protocol_version,
        request_id: RequestId::new("another-request").unwrap(),
        instance_id: request.instance_id.clone(),
        capture_revision: None,
        module_revisions: Vec::new(),
        outcome: ResponseOutcome::Success { result: json!({}) },
        warnings: Vec::new(),
    };
    assert!(response.validate_for(&request).is_err());
}

#[test]
fn direct_envelope_validation_enforces_the_integer_only_json_model() {
    let mut request = request();
    request.payload = json!({"unsupported_float": 1.5});
    assert!(request.validate().is_err());

    let response = ResponseEnvelope {
        protocol_version: 1,
        request_id: RequestId::new("float-response").unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        capture_revision: None,
        module_revisions: Vec::new(),
        outcome: ResponseOutcome::Success {
            result: json!({"unsupported_float": 1.5}),
        },
        warnings: Vec::new(),
    };
    assert!(response.validate().is_err());
}

#[test]
fn authorized_request_rejects_mismatched_trusted_actor_context() {
    let envelope = request();
    let authorized = AuthorizedRequest {
        actor: ActorContext {
            actor_kind: ActorKind::Agent,
            principal_id: PrincipalId::from_bytes([2; 16]),
            client_name: "integration client".to_owned(),
            connection_id: ConnectionId::from_bytes([3; 16]),
            request_id: RequestId::new("different-request").unwrap(),
            reason: None,
            requested_at_unix_ms: 1_700_000_000_000,
        },
        envelope,
    };
    assert!(authorized.validate().is_err());
}

#[test]
fn the_streaming_readers_enforce_what_the_frame_decoders_enforce() {
    // `read_request` and `read_response` are the entry points a transport
    // actually calls, and they were the only ones with no coverage. A generic
    // `read::<RequestEnvelope>` used to reach the same type without the
    // reserved-actor rejection or the envelope's `validate()`; both readers are
    // now the only public way in, so the guarantees have to hold here.
    let codec = BoundedCodec::default();

    let mut value = serde_json::to_value(request()).unwrap();
    value["actor"] = json!({"actor_kind": "agent"});
    let frame = codec.encode(FrameRole::Request, &value).unwrap();
    assert_eq!(
        codec.read_request(&mut frame.as_slice()),
        Err(CodecError::ReservedActorField)
    );

    let frame = codec.encode(FrameRole::Request, &request()).unwrap();
    assert_eq!(
        codec.read_request(&mut frame.as_slice()).unwrap(),
        request()
    );

    // An envelope that parses but fails its own invariants must not survive the
    // streaming path either: protocol version zero is rejected by validate().
    let mut invalid = serde_json::to_value(request()).unwrap();
    invalid["protocol_version"] = json!(0);
    let frame = codec.encode(FrameRole::Request, &invalid).unwrap();
    assert!(matches!(
        codec.read_request(&mut frame.as_slice()),
        Err(CodecError::Envelope(_))
    ));
}

#[test]
fn the_published_request_schema_refuses_what_the_codec_refuses() {
    // The snapshot test compares the generated document against the committed
    // file, which catches drift between those two and nothing else. It cannot
    // see the case that actually breaks a client: a published contract that
    // sanctions an input the host rejects. A generated client would build the
    // request, validate it locally, send it, and be refused.
    let codec = BoundedCodec::default();
    let schema = generated_schema::<RequestEnvelope>();

    for reserved in RESERVED_ACTOR_FIELDS {
        let mut value = serde_json::to_value(request()).unwrap();
        value[*reserved] = json!("supplied by the client");

        let frame = codec.encode(FrameRole::Request, &value).unwrap();
        assert_eq!(
            codec.decode_request_frame(&frame),
            Err(CodecError::ReservedActorField),
            "the codec must refuse `{reserved}`"
        );
        assert!(
            validate_instance(&schema, &value).is_err(),
            "the published schema must refuse `{reserved}` too, not merely allow it"
        );
    }

    // The refusal is for the reserved names alone. Both sides stay tolerant
    // readers of an additive field a newer client may send, as the contract
    // requires of every wire DTO: closing the envelope entirely would have
    // broken the first client built from a later schema.
    let mut additive = serde_json::to_value(request()).unwrap();
    additive["trace_hint"] = json!("a field this host does not know");
    let frame = codec.encode(FrameRole::Request, &additive).unwrap();
    assert_eq!(codec.decode_request_frame(&frame).unwrap(), request());
    validate_instance(&schema, &additive).unwrap();

    // And the agreement holds the other way: a well-formed request both sides
    // accept, so the schema is not simply refusing everything.
    let valid = serde_json::to_value(request()).unwrap();
    assert!(
        codec
            .decode_request_frame(&codec.encode(FrameRole::Request, &valid).unwrap())
            .is_ok()
    );
    validate_instance(&schema, &valid).unwrap();
}

#[test]
fn the_handshake_frames_have_typed_doors_of_their_own() {
    // The generic decoder went private with the bypass it was; the first
    // frames of a connection still need a way in, and it is typed like the
    // envelopes' so nothing reaches a handshake type unread by its own door.
    use quantick_control::handshake::{
        BearerToken, HandshakeRequest, HandshakeResponse, ProtocolLimits, ProtocolVersionRange,
    };
    use quantick_control::id::{InstanceId, PermissionId, ProcessNonce, ProfileId};
    use std::collections::BTreeSet;

    let codec = BoundedCodec::default();
    let request = HandshakeRequest {
        protocol_versions: ProtocolVersionRange::new(1, 1).unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        client_name: "typed door".to_owned(),
        client_version: "1.0.0".to_owned(),
        bearer_token: BearerToken::from_bytes([7; 32]),
        requested_profile: ProfileId::new("observer").unwrap(),
        requested_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
    };
    let frame = codec.encode(FrameRole::Request, &request).unwrap();
    assert_eq!(
        codec.read_handshake_request(&mut frame.as_slice()).unwrap(),
        request
    );

    let response = HandshakeResponse {
        protocol_version: 1,
        instance_id: InstanceId::from_bytes([1; 16]),
        process_nonce: ProcessNonce::from_bytes([2; 16]),
        connection_id: ConnectionId::from_bytes([3; 16]),
        application_version: "0.1.0".to_owned(),
        application_commit: "abc123".to_owned(),
        effective_profile: ProfileId::new("observer").unwrap(),
        effective_scopes: BTreeSet::from([PermissionId::new("observe").unwrap()]),
        effective_limits: ProtocolLimits::default(),
    };
    let frame = codec.encode(FrameRole::Response, &response).unwrap();
    assert_eq!(
        codec
            .read_handshake_response(&mut frame.as_slice())
            .unwrap(),
        response
    );
}
