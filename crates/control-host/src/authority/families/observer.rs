use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.user_text",
    "User-authored labels, notes, and scripts",
    true,
)];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
        id: permission(OBSERVE_PERMISSION_ID),
        label: "Observe".to_owned(),
        description: "Invoke read-only observer capabilities.".to_owned(),
        sensitive: false,
        default_grant: DefaultGrant::Granted,
        profile_ceilings: BTreeSet::from([profile(OBSERVER_PROFILE_ID)]),
    });
    tables.effects.push(EffectPolicy {
        id: effect(OBSERVE_EFFECT_ID),
        permission_floor: permission(OBSERVE_PERMISSION_ID),
        // Both read-only tiers: a read whose scopes are all ordinary is
        // reachable from the floor, and one that names a private scope
        // only from the analyst. The registry checks that *some* listed
        // ceiling grants every permission a capability declares, so
        // leaving the analyst out would fail `evidence.capture` at
        // startup rather than at the gate.
        profile_ceilings: BTreeSet::from([
            profile(OBSERVER_PROFILE_ID),
            profile(ANALYST_PROFILE_ID),
        ]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
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
    });
}
