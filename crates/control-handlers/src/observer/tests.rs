use super::*;
use quantick_control::{
    id::{RequestId, SnapshotScopeId},
    registry::ModuleDescriptor,
};
use quantick_control_host::{clock::HostClock, projection::CaptureContext};
use std::time::Duration;

struct Clock;
impl HostClock for Clock {
    fn unix_ms(&self) -> i64 {
        123
    }
    fn monotonic(&self) -> Duration {
        Duration::ZERO
    }
}

fn contract() -> ObserverContract<(), ()> {
    let mut projections = ProjectionRegistry::new(Arc::new(Clock));
    for name in ["chart", "interaction"] {
        projections
            .register_module(
                ModuleDescriptor {
                    id: module(name),
                    title: name.to_owned(),
                    description: name.to_owned(),
                },
                |_: &()| 0_u64,
            )
            .unwrap();
    }
    for (name, owner, permissions) in [
        ("chart.summary", "chart", &["observe.chart"][..]),
        (
            "interaction.cursor",
            "interaction",
            &["observe.interaction"][..],
        ),
        (
            "interaction.selection",
            "interaction",
            &["observe.paper"][..],
        ),
    ] {
        projections
            .register_scope(
                SnapshotScopeId::new(name).unwrap(),
                module(owner),
                1,
                name,
                name,
                permissions,
                |_: &(), _: CaptureContext| 0_u64,
            )
            .unwrap();
    }
    ObserverContract::new(
        &projections,
        Arc::new(ActionRegistry::new()),
        EvidenceStore::new(),
        BuildIdentity {
            application_version: "fixture",
            git_commit: Some("fixed"),
        },
    )
    .unwrap()
}

fn request(capability: &str, payload: Value) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        request_id: RequestId::new("contract-test").unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        capability_id: CapabilityId::new(capability).unwrap(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run: false,
        reason: None,
        payload,
    }
}

#[test]
fn sensitive_cross_scope_reference_fails_closed() {
    let contract = contract();
    let grant = contract.default_grant();
    let error = contract
        .prepare(
            request(
                SNAPSHOT_CAPABILITY_ID,
                json!({ "scopes": ["interaction.selection"] }),
            ),
            &grant,
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), codes::SCOPE_DENIED);
    assert!(
        error.context.details.unwrap()["missing_permissions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|permission| permission == "observe.paper")
    );
}

#[test]
fn default_grant_can_read_chart_but_not_sensitive_scopes() {
    let contract = contract();
    let grant = contract.default_grant();
    contract
        .prepare(
            request(
                SNAPSHOT_CAPABILITY_ID,
                json!({ "scopes": ["chart.summary", "interaction.cursor"] }),
            ),
            &grant,
        )
        .unwrap();
    assert!(!grant.contains(&permission("observe.paper")));
    assert!(!grant.contains(&permission("observe.evidence")));
    assert!(!grant.contains(&permission("observe.user_text")));
}

#[test]
fn a_second_registered_handler_docks_without_changing_gateway_dispatch() {
    let mut contract = contract();
    contract
        .contract
        .register_read(
            authority::read_descriptor::<EmptyInput, DescribeResult>(&authority::ReadSpec {
                id: "control.second",
                module: "control",
                title: "Second observer handler",
                description: "Exercises the registered capability handler port.",
                permissions: &[OBSERVE_PERMISSION_ID],
                pagination: None,
            }),
            reads::prepare_describe,
        )
        .unwrap();

    let prepared = contract
        .prepare(
            request("control.second", json!({})),
            &contract.default_grant(),
        )
        .unwrap();
    assert!(
        prepared
            .dispatch
            .execute_worker(
                &contract,
                &InstanceId::from_bytes([1; 16]),
                &profile(OBSERVER_PROFILE_ID),
                &contract.default_grant(),
                &ProtocolLimits::default(),
                123,
            )
            .unwrap()
            .is_ok()
    );
}

#[test]
fn snapshot_and_evidence_scope_validation_preserves_errors_and_permissions() {
    let contract = contract();
    let all = contract
        .capabilities()
        .permissions()
        .iter()
        .map(|p| p.id.clone())
        .collect();
    for capability in ["snapshot.read", "evidence.capture"] {
        for scopes in [
            json!(["chart.summary", "chart.summary"]),
            json!(["missing.scope"]),
        ] {
            let error = contract
                .prepare(request(capability, json!({"scopes": scopes})), &all)
                .unwrap_err();
            assert_eq!(error.code.as_str(), codes::INVALID_REQUEST);
        }
        let prepared = contract
            .prepare(
                request(capability, json!({"scopes": ["interaction.selection"]})),
                &all,
            )
            .unwrap();
        assert!(
            prepared
                .required_permissions
                .contains(&permission("observe.paper"))
        );
    }
    let prepared = contract
        .prepare(
            request(
                "evidence.capture",
                json!({"scopes": ["chart.summary"], "screenshot": true}),
            ),
            &all,
        )
        .unwrap();
    assert!(
        prepared
            .required_permissions
            .contains(&permission("observe.screenshot"))
    );
    assert!(prepared.dispatch.needs_screenshot());
    assert!(prepared.dispatch.needs_paint());
    assert_eq!(prepared.dispatch.window_target(), Some(None));
}

#[test]
fn event_cursor_and_wait_timeout_boundaries_are_validated_before_dispatch() {
    let contract = contract();
    let grant = contract.default_grant();
    for capability in ["events.read", "events.wait"] {
        let mut payload = json!({});
        if capability == "events.wait" {
            payload["timeout_ms"] = json!(1);
        }
        let error = contract
            .prepare(request(capability, payload), &grant)
            .unwrap_err();
        assert_eq!(error.code.as_str(), codes::CURSOR_INVALID);
    }
    for timeout in [0, quantick_control::limits::CONTROL_WAIT_TIMEOUT_MAX_MS + 1] {
        assert!(
            contract
                .prepare(
                    request(
                        "events.wait",
                        json!({"start": "now", "timeout_ms": timeout})
                    ),
                    &grant
                )
                .is_err()
        );
    }
}
