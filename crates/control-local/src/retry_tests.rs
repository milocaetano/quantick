use super::*;
use quantick_control::id::{CapabilityId, InstanceId};
use serde_json::json;

fn request(id: &str, capability: &str) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: 1,
        request_id: RequestId::new(id).unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        capability_id: CapabilityId::new(capability).unwrap(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run: false,
        reason: None,
        payload: json!({}),
    }
}

fn answer(request: &RequestEnvelope, outcome: ResponseOutcome) -> ResponseEnvelope {
    ResponseEnvelope {
        protocol_version: 1,
        request_id: request.request_id.clone(),
        instance_id: request.instance_id.clone(),
        capture_revision: None,
        module_revisions: Vec::new(),
        outcome,
        warnings: Vec::new(),
    }
}

fn describe(state: &mut RetryState) {
    let request = request("describe", DESCRIBE);
    state.begin(&request).unwrap();
    state.answered(&answer(
        &request,
        ResponseOutcome::Success {
            result: json!({
                "capabilities": [{ "id": "example.read", "version": 1, "read_only": true }],
            }),
        },
    ));
}

#[test]
fn failed_or_malformed_refresh_cannot_preserve_old_read_only_claims() {
    for outcome in [
        ResponseOutcome::Failure {
            error: ControlError::invalid_request("describe refused"),
        },
        ResponseOutcome::Success { result: json!({}) },
    ] {
        let mut state = RetryState::default();
        describe(&mut state);
        assert!(state.knows("example.read", 1));
        let refresh = request("refresh", DESCRIBE);
        state.begin(&refresh).unwrap();
        state.answered(&answer(&refresh, outcome));
        state.begin(&request("call", "example.read")).unwrap();
        assert!(!state.transport_error().retryable);
    }
}

#[test]
fn duplicate_id_refusal_does_not_forget_the_original_mutation() {
    let mut state = RetryState::default();
    let mutation = request("same-id", "example.mutate");
    state.begin(&mutation).unwrap();
    state.begin(&mutation).unwrap();
    state.answered(&answer(
        &mutation,
        ResponseOutcome::Failure {
            error: ControlError::invalid_request("duplicate id"),
        },
    ));
    assert!(!state.transport_error().retryable);
    state.answered(&answer(
        &mutation,
        ResponseOutcome::Success { result: json!({}) },
    ));
    assert!(state.transport_error().retryable);
}

#[test]
fn one_pending_mutation_prevents_retry_advice_for_a_multiplexed_transport_failure() {
    let mut state = RetryState::default();
    describe(&mut state);
    state.begin(&request("read", "example.read")).unwrap();
    assert!(state.transport_error().retryable);
    state.begin(&request("mutation", "example.mutate")).unwrap();
    let error = state.transport_error();
    assert!(!error.retryable);
    assert_eq!(
        error.context.details.as_ref().unwrap()["outcome"],
        "unknown"
    );
}
