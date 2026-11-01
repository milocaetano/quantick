use quantick_control_host::actions::ActionRegistry;
use serde_json::json;

use super::*;
use crate::test_support::{FakeAccess, FakeTab, FakeWindow};

fn docked() -> ActionRegistry<FakeWindow, FakeAccess> {
    let mut registry = ActionRegistry::new();
    register(&mut registry).unwrap();
    registry
}

#[test]
fn borrowed_schemas_name_the_same_registered_action_and_refuse_other_versions() {
    let registry = docked();
    let action = registry.lookup(MARK_CAPABILITY_ID, 1).unwrap();
    let schemas = registry.schemas(&action.descriptor.id, 1).unwrap();
    assert!(std::ptr::eq(schemas.capability_id, &action.descriptor.id));
    assert!(std::ptr::eq(schemas.input, &action.input));
    assert!(std::ptr::eq(schemas.output, &action.output));
    assert_eq!(schemas.version, action.descriptor.version);
    assert!(registry.schemas(&action.descriptor.id, 2).is_none());
    assert!(
        registry
            .schemas(&CapabilityId::new("action.unknown").unwrap(), 1)
            .is_none()
    );
}

#[test]
fn the_mark_registers_and_validates_its_schemas() {
    let registry = docked();
    let action = registry.lookup(MARK_CAPABILITY_ID, 1).unwrap();
    let (descriptor, input, output) = (&action.descriptor, &action.input, &action.output);
    assert_eq!(descriptor.effect.as_str(), ANNOTATE_EFFECT_ID);
    assert!(!descriptor.read_only);
    assert!(
        descriptor
            .required_permissions
            .iter()
            .any(|permission| permission.as_str() == ANNOTATE_ATTENTION_PERMISSION_ID)
    );
    input.validate(&json!({})).unwrap();
    input
        .validate(&json!({ "note": "this absorption" }))
        .unwrap();
    assert!(input.validate(&json!({ "note": 1 })).is_err());
    assert!(input.validate(&json!({ "unexpected": true })).is_err());
    assert!(
        output.validate(&json!({})).is_err(),
        "a result needs its fields"
    );
    assert!(registry.lookup("attention.mark.create", 2).is_none());
    assert!(registry.lookup("not an id", 1).is_none());
}

#[test]
fn a_second_registration_of_the_same_action_is_refused() {
    let mut registry = docked();
    assert!(matches!(
        registry.register(mark_descriptor(), create_mark::<FakeWindow, FakeAccess>),
        Err(RegistryError::Duplicate { .. })
    ));
}

/// A caller that passes no target is given the pointer; a replayed entry
/// says so whatever the trace recorded.
#[test]
fn a_mark_resolves_the_pointer_and_a_replay_names_its_source() {
    let window = FakeWindow::new(vec![FakeTab::new(3, "WIN")]);
    let resolved = resolve_mark(&window, &FakeWindow::trader(), json!({ "note": "here" }))
        .expect("resolved");
    assert_eq!(resolved["target_source"], "pointer");
    assert_eq!(resolved["target"]["active_tab_id"], "3");

    let mut window = window;
    let mut access = FakeAccess::with_budget(0);
    let result = create_mark(
        &mut window,
        &mut access,
        &FakeWindow::trader(),
        &resolved,
    )
    .expect("marked");
    assert_eq!(result["target_source"], "pointer");
    assert_eq!(result["sequence"], "1");
    assert_eq!(access.events.len(), 1);

    let mut automation = FakeWindow::trader();
    automation.actor_kind = ActorKind::Automation;
    let replayed = create_mark(&mut window, &mut access, &automation, &resolved).expect("replayed");
    assert_eq!(replayed["target_source"], "replayed");
}

#[test]
fn a_note_past_the_reason_limit_is_refused_before_it_journals() {
    let mut window = FakeWindow::new(vec![FakeTab::new(3, "WIN")]);
    let mut access = FakeAccess::with_budget(0);
    let resolved = resolve_mark(
        &window,
        &FakeWindow::trader(),
        json!({ "note": "x".repeat(CONTROL_REASON_MAX_BYTES + 1) }),
    )
    .expect("the resolver does not judge the note");
    create_mark(&mut window, &mut access, &FakeWindow::trader(), &resolved)
        .expect_err("too long");
    assert!(access.events.is_empty());
}
