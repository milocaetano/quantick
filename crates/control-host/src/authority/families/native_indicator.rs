use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
            id: permission(COCKPIT_INDICATORS_PERMISSION_ID),
            label: "Change saved native indicators".to_owned(),
            description: "Add, configure, hide or remove native indicators, including indicators you created and their saved settings. Changes also affect panes that mirror this layout. Removed indicators and overwritten settings have no undo through this interface. Does not place orders or change positions.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        });
    tables.effects.push(EffectPolicy {
        id: effect(INDICATORS_EFFECT_ID),
        permission_floor: permission(COCKPIT_INDICATORS_PERMISSION_ID),
        profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        risk_reducing_confirmation_class: None,
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            // Like recovery, loss is declared through consent and risk:
            // destructive requires a revision guard this host cannot supply.
            destructive: false,
            idempotent: false,
            open_world: false,
        },
        required_risk_flags: BTreeSet::from([
            RiskFlagId::new(INDICATORS_CHANGED_RISK_FLAG).expect("static risk flag is valid")
        ]),
        constraints: EffectConstraints {
            required_read_only: Some(false),
            allows_destructive: true,
            durable_requires_reversible: false,
            irreversible_transient_risk: None,
            allows_risk_reducing: false,
        },
    });
}
