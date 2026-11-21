use super::*;
use crate::test_support::{FakeAccess, FakeWindow};
use quantick_control::{
    id::*,
    wire::{RequestEnvelope, ResponseEnvelope, ResponseOutcome},
};
use quantick_control_host::{
    actions::ActionRegistry,
    admission::TierPolicy,
    authority,
    contract::{ExternalSchemas, ReadPreparation},
    idempotency::IdempotencyStore,
};
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};

#[test]
fn fake_mutating_family_reaches_every_manifest_consumer() {
    let contract = crate::generation::composition::contract().unwrap();
    let mut actions = ActionRegistry::<FakeWindow, FakeAccess>::new();
    super::register_actions(&mut actions).unwrap();
    let action = actions.lookup(wire::FIXTURE_CAPABILITY_ID, 1).unwrap();
    assert_eq!(
        serde_json::to_value(&action.descriptor).unwrap(),
        serde_json::to_value(
            contract
                .registry()
                .capability(&action.descriptor.id, 1)
                .unwrap()
        )
        .unwrap()
    );
    assert!(
        contract
            .snapshot_scopes()
            .iter()
            .any(|scope| scope.id.as_str() == "fixture.available")
    );
    assert!(
        crate::generation::documents()
            .iter()
            .any(|document| document.file_name == "fixture-input-v1.schema.json")
    );
    let row = quantick_control_schema::retry_matrix::readback(wire::FIXTURE_CAPABILITY_ID).unwrap();
    assert_eq!(row.event, Some("fixture.changed"));
    assert_eq!(row.policy, action.descriptor.idempotency);
    assert!(
        crate::generation::retry_matrix_markdown()
            .unwrap()
            .contains(wire::FIXTURE_CAPABILITY_ID)
    );
    assert!(quantick_control_schema::zz_fixture::behaviour_is_docked());
    let request = RequestEnvelope {
        protocol_version: 1,
        request_id: RequestId::new("first").unwrap(),
        instance_id: InstanceId::from_bytes([0; 16]),
        capability_id: action.descriptor.id.clone(),
        capability_version: 1,
        expected_revisions: vec![],
        idempotency_key: Some(IdempotencyKey::new("fixture-key").unwrap()),
        dry_run: false,
        reason: None,
        payload: json!({"enabled":true}),
    };
    let admit = |request: RequestEnvelope, grant: &BTreeSet<PermissionId>| {
        contract.admit(
            request,
            grant,
            TierPolicy::STRICT,
            |_, _| {
                Some(ExternalSchemas {
                    capability_id: &action.descriptor.id,
                    version: 1,
                    input: &action.input,
                    output: &action.output,
                })
            },
            |_, _, _| {
                Ok(ReadPreparation {
                    prepared: (),
                    dynamic_permissions: BTreeSet::new(),
                })
            },
        )
    };
    assert!(admit(request.clone(), &authority::default_grant()).is_err());
    let mut grant = authority::default_grant();
    grant.insert(PermissionId::new("cockpit").unwrap());
    assert!(
        admit(request.clone(), &grant).is_err(),
        "the new permission is required"
    );
    grant.insert(PermissionId::new("cockpit.fixture").unwrap());
    admit(request.clone(), &grant).unwrap();
    let mut invalid = request.clone();
    invalid.payload = json!({"enabled":"yes"});
    assert!(admit(invalid, &grant).is_err());
    let actor = FakeWindow::assistant();
    let mut host = FakeWindow::new(vec![]);
    let mut access = FakeAccess::default();
    let store = Arc::new(IdempotencyStore::default());
    let principal = PrincipalId::from_bytes([1; 16]);
    let mut ticket = IdempotencyStore::ticket(&request.instance_id, &principal, &request)
        .unwrap()
        .unwrap();
    assert!(IdempotencyStore::admit(&store, &mut ticket, &request, 100).is_none());
    let result = (action.handler)(&mut host, &mut access, &actor, &request.payload).unwrap();
    assert!(host.rail_magnet);
    assert_eq!(access.events.len(), 1);
    assert_eq!(access.events[0].kind.as_str(), row.event.unwrap());
    assert_eq!(access.events[0].payload["enabled"], true);
    let response = ResponseEnvelope {
        protocol_version: 1,
        request_id: request.request_id.clone(),
        instance_id: request.instance_id.clone(),
        capture_revision: None,
        module_revisions: vec![],
        outcome: ResponseOutcome::Success { result },
        warnings: vec![],
    };
    store.record(&ticket, &response, 100);
    drop(ticket);
    let mut retry = request.clone();
    retry.request_id = RequestId::new("retry").unwrap();
    let mut ticket = IdempotencyStore::ticket(&retry.instance_id, &principal, &retry)
        .unwrap()
        .unwrap();
    let replay = IdempotencyStore::admit(&store, &mut ticket, &retry, 101).unwrap();
    assert_eq!(replay.outcome, response.outcome);
    assert_eq!(
        access.events.len(),
        1,
        "retry did not dispatch another mutation"
    );
    let mut conflicting = retry;
    conflicting.payload = json!({"enabled":false});
    let mut ticket = IdempotencyStore::ticket(&conflicting.instance_id, &principal, &conflicting)
        .unwrap()
        .unwrap();
    assert!(matches!(
        IdempotencyStore::admit(&store, &mut ticket, &conflicting, 102)
            .unwrap()
            .outcome,
        ResponseOutcome::Failure { .. }
    ));
}
