use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
            id: permission(COCKPIT_ALERT_DELETE_PERMISSION_ID),
            label: "Delete alerts and alert history".to_owned(),
            description: "Permanently delete local alert definitions or clear recorded alert history. No command restores removed definitions or events. Deleting a definition preserves its recorded history; clearing history preserves definitions.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        });
    tables.modules.push(ModuleDescriptor {
        id: module("alerts"),
        title: "Alerts".to_owned(),
        description: "Persistent local price alerts and immutable firing history.".to_owned(),
    });
    tables.effects.push(EffectPolicy {
        id: effect(ALERT_DELETE_EFFECT_ID),
        permission_floor: permission(COCKPIT_ALERT_DELETE_PERMISSION_ID),
        profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        risk_reducing_confirmation_class: None,
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            // Journal-delete policy: destructive couples to envelope revisions
            // this host refuses. Loss is explicit in sensitive consent, risk,
            // and reversible:false. Alert payload revisions are separate.
            destructive: false,
            idempotent: true,
            open_world: false,
        },
        required_risk_flags: BTreeSet::from([
            RiskFlagId::new(ALERT_REMOVED_RISK_FLAG).expect("static risk flag is valid")
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
