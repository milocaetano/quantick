use super::*;

pub(super) const SCOPES: &[(&str, &str, bool)] = &[];

pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
        id: permission(STRATEGY_DESCRIBE_PERMISSION_ID),
        label: "Describe a strategy".into(),
        description: "Read a strategy conversation, send its chart context to the configured provider, and save an alarm-only strategy.".into(),
        sensitive: true,
        default_grant: DefaultGrant::Prompt,
        profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
    });
    tables.effects.push(EffectPolicy {
        id: effect(STRATEGY_PROVIDER_EFFECT_ID),
        permission_floor: permission(STRATEGY_DESCRIBE_PERMISSION_ID),
        profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        risk_reducing_confirmation_class: None,
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: true,
        },
        required_risk_flags: BTreeSet::from([
            RiskFlagId::new(STRATEGY_PROVIDER_RISK).expect("static risk flag"),
        ]),
        constraints: EffectConstraints {
            required_read_only: Some(false),
            allows_destructive: false,
            durable_requires_reversible: true,
            irreversible_transient_risk: Some(
                RiskFlagId::new(STRATEGY_PROVIDER_RISK).expect("static risk flag"),
            ),
            allows_risk_reducing: false,
        },
    });
}
