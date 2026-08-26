use std::collections::BTreeSet;

use quantick_control::{
    codec::{BoundedCodec, FrameRole},
    error::codes,
    fake::{
        COUNTER_READ, COUNTER_SET, EnvelopeTransport, FakeClient, FakeConnection, FakeHost,
        FakeInvocation, TrustedSession,
    },
    id::{ConnectionId, IdempotencyKey, InstanceId, ModuleId, PermissionId, PrincipalId},
    limits::CONTROL_IDEMPOTENCY_MAX_ENTRIES,
    wire::{ModuleRevision, RequestEnvelope, ResponseEnvelope, ResponseOutcome, WireU64},
};
use serde_json::{Value, json};

struct FramedTransport<T> {
    inner: T,
    codec: BoundedCodec,
}

impl<T: EnvelopeTransport> EnvelopeTransport for FramedTransport<T> {
    fn exchange(&mut self, request: RequestEnvelope) -> ResponseEnvelope {
        let encoded = self.codec.encode(FrameRole::Request, &request).unwrap();
        let decoded = self.codec.decode_request_frame(&encoded).unwrap();
        let response = self.inner.exchange(decoded);
        let encoded = self.codec.encode(FrameRole::Response, &response).unwrap();
        let response = self.codec.decode_response_frame(&encoded).unwrap();
        response.validate_for(&request).unwrap();
        response
    }
}

fn session() -> TrustedSession {
    TrustedSession {
        principal_id: PrincipalId::from_bytes([2; 16]),
        connection_id: ConnectionId::from_bytes([3; 16]),
        client_name: "framed fake client".to_owned(),
        granted_permissions: BTreeSet::from([
            PermissionId::new("observe").unwrap(),
            PermissionId::new("fake.write").unwrap(),
        ]),
    }
}

fn client<'host>(
    instance: InstanceId,
    connection: FakeConnection<'host>,
) -> FakeClient<FramedTransport<FakeConnection<'host>>> {
    FakeClient::new(
        instance,
        FramedTransport {
            inner: connection,
            codec: BoundedCodec::default(),
        },
    )
}

#[test]
fn fake_host_and_client_exchange_the_real_bounded_envelopes() {
    let instance = InstanceId::from_bytes([1; 16]);
    let mut host = FakeHost::new(instance.clone()).unwrap();
    let connection = host.connect(session());
    let mut client = client(instance, connection);
    let response = client.invoke(FakeInvocation::try_new(COUNTER_READ, json!({})).unwrap());
    assert!(matches!(
        response.outcome,
        ResponseOutcome::Success { result }
            if result == json!({"value": 0, "revision": "0"})
    ));
    assert!(response.capture_revision.is_some());
}

#[test]
fn retry_returns_original_result_and_key_reuse_with_new_payload_changes_nothing() {
    let instance = InstanceId::from_bytes([1; 16]);
    let mut host = FakeHost::new(instance.clone()).unwrap();
    {
        let connection = host.connect(session());
        let mut client = client(instance, connection);
        let mut set = FakeInvocation::try_new(COUNTER_SET, json!({"value": 5})).unwrap();
        set.expected_revisions = vec![ModuleRevision {
            module_id: ModuleId::new("fake").unwrap(),
            revision: WireU64::new(0),
        }];
        set.idempotency_key = Some(IdempotencyKey::new("counter-set-5").unwrap());

        let first = client.invoke(set.clone());
        let retry = client.invoke(set.clone());
        assert_eq!(successful_result(&first), successful_result(&retry));
        assert_eq!(
            successful_result(&retry),
            json!({"previous": 0, "current": 5, "actor_kind": "agent"})
        );

        set.payload = json!({"value": 6});
        let conflict = client.invoke(set);
        assert!(matches!(
            conflict.outcome,
            ResponseOutcome::Failure { error }
                if error.code.as_str() == codes::IDEMPOTENCY_CONFLICT
        ));
    }
    assert_eq!(host.counter(), 5);
    assert_eq!(host.fake_revision(), 1);
}

#[test]
fn revision_conflict_and_dry_run_leave_state_unchanged() {
    let instance = InstanceId::from_bytes([1; 16]);
    let mut host = FakeHost::new(instance.clone()).unwrap();
    {
        let connection = host.connect(session());
        let mut client = client(instance, connection);
        let mut stale = FakeInvocation::try_new(COUNTER_SET, json!({"value": 9})).unwrap();
        stale.expected_revisions = vec![ModuleRevision {
            module_id: ModuleId::new("fake").unwrap(),
            revision: WireU64::new(4),
        }];
        stale.idempotency_key = Some(IdempotencyKey::new("stale-set").unwrap());
        assert!(matches!(
            client.invoke(stale).outcome,
            ResponseOutcome::Failure { error }
                if error.code.as_str() == codes::REVISION_CONFLICT
        ));

        let mut dry_run = FakeInvocation::try_new(COUNTER_SET, json!({"value": 9})).unwrap();
        dry_run.expected_revisions = vec![ModuleRevision {
            module_id: ModuleId::new("fake").unwrap(),
            revision: WireU64::new(0),
        }];
        dry_run.dry_run = true;
        assert!(matches!(
            client.invoke(dry_run).outcome,
            ResponseOutcome::Success { .. }
        ));
    }
    assert_eq!(host.counter(), 0);
    assert_eq!(host.fake_revision(), 0);
}

#[test]
fn descriptor_revision_policy_is_enforced_before_dispatch() {
    let instance = InstanceId::from_bytes([1; 16]);
    let mut host = FakeHost::new(instance.clone()).unwrap();
    let connection = host.connect(session());
    let mut client = client(instance, connection);
    let mut read = FakeInvocation::try_new(COUNTER_READ, json!({})).unwrap();
    read.expected_revisions = vec![ModuleRevision {
        module_id: ModuleId::new("fake").unwrap(),
        revision: WireU64::new(0),
    }];
    assert!(matches!(
        client.invoke(read).outcome,
        ResponseOutcome::Failure { error }
            if error.code.as_str() == codes::INVALID_REQUEST
    ));
}

fn successful_result(response: &ResponseEnvelope) -> Value {
    match &response.outcome {
        ResponseOutcome::Success { result } => result.clone(),
        ResponseOutcome::Failure { error } => panic!("unexpected error: {error:?}"),
    }
}

#[test]
fn the_idempotency_store_reclaims_instead_of_saturating_forever() {
    let instance = InstanceId::from_bytes([1; 16]);
    let mut host = FakeHost::new(instance.clone()).unwrap();
    let connection = host.connect(session());
    let mut client = client(instance, connection);

    fn fake_revision(response: &ResponseEnvelope) -> u64 {
        response
            .module_revisions
            .iter()
            .find(|revision| revision.module_id.as_str() == "fake")
            .map(|revision| revision.revision.get())
            .expect("the fake module reports a revision")
    }

    // One distinct key per call, well past the entry cap. Nothing evicted and
    // nothing expired, so every call past the cap used to return
    // `control.backpressure` — reported as retryable, and never able to
    // succeed however long the client waited.
    let mut revision = 0;
    let mut last_key = String::new();
    for index in 0..(CONTROL_IDEMPOTENCY_MAX_ENTRIES + 64) {
        last_key = format!("set-{index}");
        let mut set = FakeInvocation::try_new(COUNTER_SET, json!({"value": 5})).unwrap();
        set.expected_revisions = vec![ModuleRevision {
            module_id: ModuleId::new("fake").unwrap(),
            revision: WireU64::new(revision),
        }];
        set.idempotency_key = Some(IdempotencyKey::new(&last_key).unwrap());
        let response = client.invoke(set);
        assert!(
            matches!(response.outcome, ResponseOutcome::Success { .. }),
            "call {index} was refused: {:?}",
            response.outcome
        );
        revision = fake_revision(&response);
    }

    // Reclaiming must not weaken the guarantee for a key still in the window:
    // the most recent one replays rather than re-executing.
    let mut replay = FakeInvocation::try_new(COUNTER_SET, json!({"value": 5})).unwrap();
    replay.expected_revisions = vec![ModuleRevision {
        module_id: ModuleId::new("fake").unwrap(),
        revision: WireU64::new(revision - 1),
    }];
    replay.idempotency_key = Some(IdempotencyKey::new(&last_key).unwrap());
    let replayed = client.invoke(replay);
    assert!(matches!(replayed.outcome, ResponseOutcome::Success { .. }));
    assert_eq!(
        fake_revision(&replayed),
        revision,
        "a replay returns the recorded revisions instead of advancing state"
    );
}
