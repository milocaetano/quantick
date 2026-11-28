//! The adapter's own decisions: what a docked call publishes, and how its
//! one erasure parses, journals and encodes. What any call runs is its
//! family's.

use quantick_control::error::codes;
use quantick_control::registry::IdempotencyPolicy;
use quantick_control_host::actions::{ActionRegistry, ReplayTrace};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeWindow};

const EVENT: &str = "test.call.ran";

/// A host the probe counts its runs on.
#[derive(Default)]
struct Host {
    runs: u32,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Probe {
    refuse: bool,
}

#[derive(Serialize, JsonSchema)]
struct Ran {
    runs: u32,
}

fn posture(spec: &CallSpec) -> CapabilityDescriptor {
    let descriptor = crate::trade::descriptor(
        "test.posture.only",
        "posture title",
        "posture description",
        true,
        "stale",
    );
    assert!(
        spec.id.starts_with("test.call."),
        "the posture sees the spec"
    );
    CapabilityDescriptor {
        idempotency: IdempotencyPolicy::Optional,
        ..descriptor
    }
}

impl Call<Host> for Probe {
    type Input = Self;
    type Answer = Ran;

    const SPEC: CallSpec = CallSpec {
        id: "test.call.probe",
        module: "test",
        title: "Probe",
        description: "Counts a run.",
        permissions: &["test.read", "test.write"],
        event: Some(EVENT),
        replay: ReplayTrace::Excluded,
        posture,
    };

    fn run<A: JournalAccess>(
        input: Self,
        host: &mut Host,
        _: &mut CallCx<'_, A>,
    ) -> Result<Ran, ControlError> {
        if input.refuse {
            return Err(ControlError::invalid_request("refused"));
        }
        host.runs += 1;
        Ok(Ran { runs: host.runs })
    }
}

/// A call that journals nothing.
#[derive(Deserialize, JsonSchema)]
struct Quiet {}

impl Call<Host> for Quiet {
    type Input = Self;
    type Answer = Ran;

    const SPEC: CallSpec = CallSpec {
        id: "test.call.quiet",
        event: None,
        replay: ReplayTrace::Recorded,
        ..Probe::SPEC
    };

    fn run<A: JournalAccess>(
        _input: Self,
        host: &mut Host,
        _: &mut CallCx<'_, A>,
    ) -> Result<Ran, ControlError> {
        Ok(Ran { runs: host.runs })
    }
}

fn registry() -> ActionRegistry<Host, FakeAccess> {
    let mut registry = ActionRegistry::new();
    dock_call::<Probe, _, _, _>(&mut registry).unwrap();
    dock_call::<Quiet, _, _, _>(&mut registry).unwrap();
    registry
}

#[test]
fn the_descriptor_is_the_spec_and_both_schemas_over_the_posture() {
    let registry = registry();
    let action = registry
        .lookup(
            "test.call.probe",
            quantick_control_host::authority::CAPABILITY_VERSION,
        )
        .unwrap();
    let descriptor = &action.descriptor;
    assert_eq!(descriptor.id.as_str(), "test.call.probe");
    assert_eq!(descriptor.module.as_str(), "test");
    assert_eq!(
        (descriptor.title.as_str(), descriptor.description.as_str()),
        ("Probe", "Counts a run.")
    );
    let permissions: Vec<_> = descriptor
        .required_permissions
        .iter()
        .map(|id| id.as_str())
        .collect();
    assert_eq!(permissions, ["test.read", "test.write"]);
    assert_eq!(descriptor.input_schema, generated_schema::<Probe>());
    assert_eq!(descriptor.output_schema, generated_schema::<Ran>());
    assert_eq!(descriptor.idempotency, IdempotencyPolicy::Optional);
    assert!(descriptor.risk_reducing, "the rest is the posture's");
    assert_eq!(action.replay_trace, ReplayTrace::Excluded);
    let quiet = registry
        .lookup(
            "test.call.quiet",
            quantick_control_host::authority::CAPABILITY_VERSION,
        )
        .unwrap();
    assert_eq!(quiet.replay_trace, ReplayTrace::Recorded);
}

#[test]
fn a_run_is_encoded_and_journaled_and_a_refusal_or_bad_input_is_not() {
    let registry = registry();
    let probe = registry
        .lookup(
            "test.call.probe",
            quantick_control_host::authority::CAPABILITY_VERSION,
        )
        .unwrap();
    let actor = FakeWindow::assistant();
    let (mut host, mut access) = (Host::default(), FakeAccess::default());

    let bad = (probe.handler)(&mut host, &mut access, &actor, &json!({ "refuse": 1 }));
    assert_eq!(bad.unwrap_err().code.as_str(), codes::INVALID_REQUEST);
    let refused = (probe.handler)(&mut host, &mut access, &actor, &json!({ "refuse": true }));
    assert!(refused.is_err());
    assert!(access.events.is_empty(), "only a run journals");

    let ran = (probe.handler)(&mut host, &mut access, &actor, &json!({ "refuse": false }));
    assert_eq!(ran.unwrap(), json!({ "runs": 1 }));
    let [event] = access.events.as_slice() else {
        panic!("one event, not {}", access.events.len());
    };
    assert_eq!(
        (event.module_id.as_str(), event.kind.as_str()),
        ("test", EVENT)
    );
    assert_eq!(event.payload, json!({ "runs": 1 }), "the answer by default");

    let quiet = registry
        .lookup(
            "test.call.quiet",
            quantick_control_host::authority::CAPABILITY_VERSION,
        )
        .unwrap();
    (quiet.handler)(&mut host, &mut access, &actor, &json!({})).unwrap();
    assert_eq!(access.events.len(), 1, "no event kind, no event");
}
