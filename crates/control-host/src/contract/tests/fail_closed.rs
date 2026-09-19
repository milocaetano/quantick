//! Defensive checks for states the public atomic registration API cannot make.
use super::*;
use quantick_control::{
    fake::{COUNTER_READ, reference_registry},
    id::{InstanceId, RequestId},
};
use serde_json::json;

#[test]
fn missing_internal_binding_or_validator_cannot_dispatch() {
    let mut contract = CapabilityContract::<()> {
        registry: reference_registry().unwrap(),
        bindings: BTreeMap::new(),
        input_validators: BTreeMap::new(),
        output_validators: BTreeMap::new(),
        profiles: Vec::new(),
        permissions: Vec::new(),
        snapshot_scopes: Vec::new(),
        scope_permissions: BTreeMap::new(),
    };
    let id = CapabilityId::new(COUNTER_READ).unwrap();
    let request = RequestEnvelope {
        protocol_version: 1,
        request_id: RequestId::new("defensive-test").unwrap(),
        instance_id: InstanceId::from_bytes([1; 16]),
        capability_id: id.clone(),
        capability_version: 1,
        expected_revisions: Vec::new(),
        idempotency_key: None,
        dry_run: false,
        reason: None,
        payload: json!({}),
    };
    let grant = BTreeSet::from([PermissionId::new("observe").unwrap()]);
    let admit = |contract: &CapabilityContract<()>| {
        contract.admit(
            request.clone(),
            &grant,
            TierPolicy::STRICT,
            |_, _| None,
            |_, _, _| -> Result<ReadPreparation<()>, ControlError> {
                panic!("incomplete state must fail before preparation")
            },
        )
    };
    let error = admit(&contract).unwrap_err();
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert_eq!(
        error.message,
        "registered observer capability has no handler"
    );
    contract.bindings.insert((id.clone(), 1), Binding::Read(()));
    let error = admit(&contract).unwrap_err();
    assert_eq!(error.code.as_str(), codes::CAPABILITY_UNAVAILABLE);
    assert_eq!(
        error.message,
        "registered observer capability has no input validator"
    );
    assert!(!contract.validate_output(&id, 1, &json!({}), |_, _| None));
}
