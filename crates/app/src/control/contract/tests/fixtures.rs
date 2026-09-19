use super::super::*;
use quantick_control::id::RequestId;

pub(super) const CASES: [&str; 7] = [
    "small_read",
    "dynamic_snapshot",
    "static_denial",
    "malformed_payload",
    "external_action",
    "output_validation",
    "startup",
];

pub(super) fn contract() -> ObserverContract {
    ObserverContract::new(
        &crate::control::standard_registry().unwrap(),
        Arc::new(crate::control::actions::standard_actions().unwrap()),
        EvidenceStore::new(),
    )
    .unwrap()
}

pub(super) fn request(capability: &str, payload: Value) -> RequestEnvelope {
    RequestEnvelope {
        protocol_version: CURRENT_PROTOCOL_VERSION,
        request_id: RequestId::new("a1c-frozen-request").unwrap(),
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

pub(super) fn mark_output() -> Value {
    serde_json::to_value(crate::control::actions::MarkResult {
        sequence: WireU64::new(1),
        target: crate::control::interaction::CursorSnapshot {
            active_tab_id: WireU64::new(1),
            focused_pane_id: WireU64::new(1),
            focused_pane_side: quantick_control::annotation::PaneSideDto::Flow,
            pointer: None,
            pointer_availability: crate::control::types::unavailable("fixture_no_pointer"),
            semantic_scene: crate::control::types::available(),
        },
        target_source: "supplied".to_owned(),
        note: None,
        actor: crate::control::journal::EventActor {
            kind: quantick_control::wire::ActorKind::HumanUi,
            client_name: "a1c-fixture".to_owned(),
        },
    })
    .unwrap()
}

pub(super) struct Corpus {
    pub contract: ObserverContract,
    pub requests: Vec<RequestEnvelope>,
    pub grants: Vec<BTreeSet<PermissionId>>,
    pub describe: Value,
    pub mark: Value,
    pub invalid_output: Value,
    pub describe_id: CapabilityId,
    pub mark_id: CapabilityId,
}

impl Corpus {
    pub fn new() -> Self {
        let contract = contract();
        let default = contract.default_grant();
        let dynamic = [
            "observe",
            "observe.system",
            "observe.health",
            "observe.indicators",
            "observe.orderflow",
        ]
        .into_iter()
        .map(permission)
        .collect();
        let annotate = ["annotate", "annotate.attention"]
            .into_iter()
            .map(permission)
            .collect();
        let describe = serde_json::to_value(contract.describe(
            InstanceId::from_bytes([1; 16]),
            profile("observer"),
            default.clone(),
            ProtocolLimits::default(),
        ))
        .unwrap();
        Self {
            contract,
            requests: vec![
                request("control.describe", json!({})),
                request(
                    "snapshot.read",
                    json!({"scopes": ["system.info", "health.summary"]}),
                ),
                request("snapshot.read", json!({"scopes": 42})),
                request("snapshot.read", json!({"scopes": 42})),
                request("attention.mark.create", json!({})),
            ],
            grants: vec![default.clone(), dynamic, BTreeSet::new(), default, annotate],
            describe,
            mark: mark_output(),
            invalid_output: json!({}),
            describe_id: CapabilityId::new("control.describe").unwrap(),
            mark_id: CapabilityId::new("attention.mark.create").unwrap(),
        }
    }

    pub fn operation(&self, case: usize) {
        match case {
            0..=4 => drop(std::hint::black_box(
                self.contract
                    .prepare(self.requests[case].clone(), &self.grants[case]),
            )),
            5 => {
                std::hint::black_box(self.contract.validate_output(
                    &self.describe_id,
                    1,
                    &self.describe,
                ));
                std::hint::black_box(self.contract.validate_output(
                    &self.describe_id,
                    1,
                    &self.invalid_output,
                ));
            }
            6 => drop(std::hint::black_box(contract())),
            _ => panic!("unknown frozen observation case"),
        }
    }

    pub fn assert_cases(&self) {
        assert!(matches!(
            self.contract
                .prepare(self.requests[0].clone(), &self.grants[0])
                .unwrap()
                .dispatch,
            PreparedDispatch::Worker(_)
        ));
        let dynamic = self
            .contract
            .prepare(self.requests[1].clone(), &self.grants[1])
            .unwrap();
        assert!(matches!(dynamic.dispatch, PreparedDispatch::Ui(_)));
        assert_eq!(dynamic.required_permissions, self.grants[1]);
        assert_eq!(
            self.contract
                .prepare(self.requests[2].clone(), &self.grants[2])
                .unwrap_err()
                .code
                .as_str(),
            codes::PERMISSION_DENIED
        );
        assert_eq!(
            self.contract
                .prepare(self.requests[3].clone(), &self.grants[3])
                .unwrap_err()
                .code
                .as_str(),
            codes::INVALID_REQUEST
        );
        let action = self
            .contract
            .prepare(self.requests[4].clone(), &self.grants[4])
            .unwrap();
        assert!(matches!(action.dispatch, PreparedDispatch::Action(_)));
        assert_eq!(action.required_permissions, self.grants[4]);
        assert!(
            self.contract
                .validate_output(&self.describe_id, 1, &self.describe)
        );
        assert!(
            !self
                .contract
                .validate_output(&self.describe_id, 1, &self.invalid_output)
        );
        assert!(self.contract.validate_output(&self.mark_id, 1, &self.mark));
        assert!(
            !self
                .contract
                .validate_output(&self.mark_id, 1, &self.invalid_output)
        );
    }
}
