use quantick_control::{
    codec::{BoundedCodec, CodecError, FrameRole},
    fake::COUNTER_READ,
    id::{CapabilityId, ConnectionId, InstanceId, PrincipalId, RequestId},
    schema::{generated_schema, validate_instance},
    wire::{
        ActorContext, ActorKind, AuthorizedRequest, RequestEnvelope, ResponseEnvelope,
        ResponseOutcome,
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
