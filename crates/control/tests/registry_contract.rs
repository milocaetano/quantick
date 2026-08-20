use std::collections::BTreeSet;

use quantick_control::{
    fake::{COUNTER_READ, COUNTER_SET, reference_registry, reference_registry_with},
    handshake::ProfileAuthority,
    id::{CapabilityId, ConfirmationClassId, EffectId, ModuleId, PermissionId, ProfileId},
    limits::CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES,
    registry::{
        CapabilityDescriptor, ControlModule, ControlRegistry, DefaultGrant, EffectConstraints,
        EffectPolicy, McpHintFloor, ModuleDescriptor, PermissionDescriptor, RegistryError,
    },
};
use serde_json::json;

fn counter_read(registry: &ControlRegistry) -> CapabilityDescriptor {
    registry
        .capability(&CapabilityId::new(COUNTER_READ).unwrap(), 1)
        .unwrap()
        .clone()
}

#[test]
fn duplicate_ids_invalid_schemas_and_invalid_examples_are_rejected() {
    let mut registry = reference_registry().unwrap();
    let descriptor = counter_read(&registry);
    assert!(matches!(
        registry.register_capability(descriptor.clone()),
        Err(RegistryError::Duplicate {
            kind: "capability",
            ..
        })
    ));

    let mut invalid_schema = descriptor.clone();
    invalid_schema.id = CapabilityId::new("fake.invalid_schema").unwrap();
    invalid_schema.input_schema = json!({"type": 7});
    assert!(matches!(
        registry.register_capability(invalid_schema),
        Err(RegistryError::Schema(_))
    ));

    let mut invalid_example = descriptor;
    invalid_example.id = CapabilityId::new("fake.invalid_example").unwrap();
    invalid_example.examples[0].input = json!({"unexpected": true});
    assert!(matches!(
        registry.register_capability(invalid_example),
        Err(RegistryError::Schema(_))
    ));

    let mut floating_schema = counter_read(&registry);
    floating_schema.id = CapabilityId::new("fake.floating_schema").unwrap();
    floating_schema.input_schema = json!({"type": "number", "minimum": 0.5});
    assert!(matches!(
        registry.register_capability(floating_schema),
        Err(RegistryError::InvalidDescriptor(_))
    ));

    let mut oversized = counter_read(&registry);
    oversized.id = CapabilityId::new("fake.oversized_descriptor").unwrap();
    oversized.description = "x".repeat(CONTROL_CAPABILITY_DESCRIPTOR_MAX_BYTES);
    assert!(matches!(
        registry.register_capability(oversized),
        Err(RegistryError::InvalidDescriptor(_))
    ));
}

#[test]
fn duplicate_module_permission_profile_and_effect_ids_are_rejected() {
    let mut registry = reference_registry().unwrap();
    let module = registry
        .modules()
        .find(|module| module.id.as_str() == "fake")
        .unwrap()
        .clone();
    assert!(matches!(
        registry.register_module(module),
        Err(RegistryError::Duplicate { kind: "module", .. })
    ));
    let effect = registry
        .effect(&EffectId::new("observe").unwrap())
        .unwrap()
        .clone();
    assert!(matches!(
        registry.register_effect(effect),
        Err(RegistryError::Duplicate { kind: "effect", .. })
    ));

    let mut authority = ControlRegistry::new();
    let profile = quantick_control::registry::ProfileDescriptor {
        id: ProfileId::new("custom").unwrap(),
        label: "Custom".to_owned(),
        inherits: BTreeSet::new(),
        permissions: BTreeSet::new(),
    };
    authority.register_profile(profile.clone()).unwrap();
    assert!(matches!(
        authority.register_profile(profile.clone()),
        Err(RegistryError::Duplicate {
            kind: "profile",
            ..
        })
    ));
    let permission = PermissionDescriptor {
        id: PermissionId::new("custom.read").unwrap(),
        label: "Read custom data".to_owned(),
        description: "Read a bounded custom projection.".to_owned(),
        sensitive: false,
        default_grant: DefaultGrant::Denied,
        profile_ceilings: BTreeSet::from([profile.id]),
    };
    authority.register_permission(permission.clone()).unwrap();
    assert!(matches!(
        authority.register_permission(permission),
        Err(RegistryError::Duplicate {
            kind: "permission",
            ..
        })
    ));
}

#[test]
fn unknown_effects_permissions_and_contradictory_metadata_fail_closed() {
    let mut registry = reference_registry().unwrap();
    let descriptor = counter_read(&registry);

    let mut unknown_effect = descriptor.clone();
    unknown_effect.id = CapabilityId::new("fake.unknown_effect").unwrap();
    unknown_effect.effect = EffectId::new("plugin.missing").unwrap();
    assert!(matches!(
        registry.register_capability(unknown_effect),
        Err(RegistryError::Unknown {
            kind: "effect policy",
            ..
        })
    ));

    let mut unknown_permission = descriptor.clone();
    unknown_permission.id = CapabilityId::new("fake.unknown_permission").unwrap();
    unknown_permission
        .required_permissions
        .insert(PermissionId::new("plugin.undeclared").unwrap());
    assert!(matches!(
        registry.register_capability(unknown_permission),
        Err(RegistryError::Unknown {
            kind: "permission",
            ..
        })
    ));

    let mut contradictory = descriptor;
    contradictory.id = CapabilityId::new("fake.contradictory").unwrap();
    contradictory.destructive = true;
    assert!(matches!(
        registry.register_capability(contradictory),
        Err(RegistryError::InvalidDescriptor(_))
    ));
}

#[test]
fn effect_shape_reversibility_destruction_and_risk_contradictions_are_rejected() {
    let mut registry = reference_registry().unwrap();
    let read = counter_read(&registry);
    let set = registry
        .capability(&CapabilityId::new(COUNTER_SET).unwrap(), 1)
        .unwrap()
        .clone();

    let mut cases = Vec::new();
    let mut writable_observe = read.clone();
    writable_observe.id = CapabilityId::new("fake.bad_effect_shape").unwrap();
    writable_observe.read_only = false;
    cases.push(writable_observe);

    let mut reversible_read = read.clone();
    reversible_read.id = CapabilityId::new("fake.bad_reversibility").unwrap();
    reversible_read.reversible = true;
    cases.push(reversible_read);

    let mut destructive_read = read;
    destructive_read.id = CapabilityId::new("fake.bad_destruction").unwrap();
    destructive_read.destructive = true;
    cases.push(destructive_read);

    let mut irreversible_durable = set.clone();
    irreversible_durable.id = CapabilityId::new("fake.bad_durable_reversal").unwrap();
    irreversible_durable.reversible = false;
    cases.push(irreversible_durable);

    let mut missing_risk = set;
    missing_risk.id = CapabilityId::new("fake.bad_risk").unwrap();
    missing_risk.risk_flags.clear();
    cases.push(missing_risk);

    for descriptor in cases {
        assert!(
            matches!(
                registry.register_capability(descriptor),
                Err(RegistryError::InvalidDescriptor(_))
            ),
            "contradictory descriptor was accepted"
        );
    }
}

struct ThirdPartyModule;

impl ControlModule for ThirdPartyModule {
    fn register_permissions(&self, registry: &mut ControlRegistry) -> Result<(), RegistryError> {
        registry.register_permission(PermissionDescriptor {
            id: PermissionId::new("addon.inspect").unwrap(),
            label: "Inspect add-on state".to_owned(),
            description: "Read the bounded third-party projection.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Denied,
            profile_ceilings: BTreeSet::from([ProfileId::new("observer").unwrap()]),
        })
    }

    fn register(&self, registry: &mut ControlRegistry) -> Result<(), RegistryError> {
        registry.register_module(ModuleDescriptor {
            id: ModuleId::new("addon").unwrap(),
            title: "Third-party fake".to_owned(),
            description: "A module implemented outside the contract crate source.".to_owned(),
        })?;
        registry.register_effect(EffectPolicy {
            id: EffectId::new("addon.observe").unwrap(),
            permission_floor: PermissionId::new("addon.inspect").unwrap(),
            profile_ceilings: BTreeSet::from([
                ProfileId::new("observer").unwrap(),
                ProfileId::new("developer").unwrap(),
            ]),
            confirmation_class: ConfirmationClassId::new("none").unwrap(),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: true,
                destructive: false,
                idempotent: false,
                open_world: false,
            },
            required_risk_flags: BTreeSet::new(),
            constraints: EffectConstraints {
                required_read_only: Some(true),
                allows_destructive: false,
                durable_requires_reversible: false,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        })?;

        let mut capability = registry
            .capability(&CapabilityId::new(COUNTER_READ).unwrap(), 1)
            .unwrap()
            .clone();
        capability.id = CapabilityId::new("addon.inspect").unwrap();
        capability.module = ModuleId::new("addon").unwrap();
        capability.effect = EffectId::new("addon.observe").unwrap();
        capability.title = "Inspect add-on".to_owned();
        capability.description = "Read a third-party projection.".to_owned();
        capability.required_permissions =
            BTreeSet::from([PermissionId::new("addon.inspect").unwrap()]);
        registry.register_capability(capability)
    }
}

#[test]
fn external_module_docks_with_its_own_permission_and_effect_without_contract_edits() {
    let module = ThirdPartyModule;
    let registry = reference_registry_with(&[&module]).unwrap();
    assert!(
        registry
            .capability(&CapabilityId::new("addon.inspect").unwrap(), 1)
            .is_some()
    );
    assert!(
        registry
            .effect(&EffectId::new("addon.observe").unwrap())
            .is_some()
    );
    assert!(
        registry
            .permission(&PermissionId::new("addon.inspect").unwrap())
            .is_some()
    );
    assert!(
        registry
            .permission_ceiling(&ProfileId::new("observer").unwrap())
            .is_some_and(|permissions| {
                permissions.contains(&PermissionId::new("addon.inspect").unwrap())
            })
    );
    assert!(
        registry
            .modules()
            .any(|module| module.id.as_str() == "addon")
    );
}
