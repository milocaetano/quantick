use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
            id: permission(COCKPIT_JOURNAL_DELETE_PERMISSION_ID),
            label: "Delete trading sessions".to_owned(),
            description: "Move a saved trading session out of the trades journal into `.deleted` under the trades folder: the ledger, the report and every listing stop reading it, and only moving the file back by hand restores it. The session being traded is always refused.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        });
    tables.effects.push(
        // Deleting a journal session: `cockpit.recover`'s shape, for the
        // same reasons, with its own consent text and risk flag.
        EffectPolicy {
            id: effect(JOURNAL_DELETE_EFFECT_ID),
            permission_floor: permission(COCKPIT_JOURNAL_DELETE_PERMISSION_ID),
            profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
            confirmation_class: confirmation(NO_CONFIRMATION_ID),
            risk_reducing_confirmation_class: None,
            mcp_hint_floor: McpHintFloor {
                read_only: false,
                // As under `cockpit.recover`: the registry couples
                // `destructive` to an expected-revision check this host
                // refuses. The loss is declared by the required risk flag,
                // the sensitive permission and `reversible: false`.
                destructive: false,
                // A repeated delete finds the session gone and removes
                // nothing more, so a dropped call may be retried.
                idempotent: true,
                open_world: false,
            },
            required_risk_flags: BTreeSet::from([
                RiskFlagId::new(SESSION_REMOVED_RISK_FLAG).expect("static risk flag is valid")
            ]),
            constraints: EffectConstraints {
                required_read_only: Some(false),
                allows_destructive: true,
                // The file moves aside rather than vanishing, but nothing in
                // the contract moves it back.
                durable_requires_reversible: false,
                irreversible_transient_risk: None,
                allows_risk_reducing: false,
            },
        },
    );
}
