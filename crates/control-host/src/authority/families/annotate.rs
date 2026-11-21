use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] =
    &[("observe.drawings", "Drawing state and references", false)];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(// The annotate tier's floor. Every scope below it is off by
        // default and reaches a client only when the trader ticks it in
        // the access panel, which is also what raises the connection's
        // ceiling to the `annotator` profile (contract §7.1).
        PermissionDescriptor {
            id: permission(ANNOTATE_PERMISSION_ID),
            label: "Annotate".to_owned(),
            description: "Add reversible state or bounded notifications; never remove existing work or affect a position.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
        });
    tables.permissions.push(PermissionDescriptor {
        id: permission(ANNOTATE_CHART_PERMISSION_ID),
        label: "Answer on the chart".to_owned(),
        description: "Place labels, arrows and zones, attributed and removable in one action."
            .to_owned(),
        sensitive: false,
        default_grant: DefaultGrant::Prompt,
        profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
    });
    tables.modules.push(ModuleDescriptor {
        id: module(ANNOTATE_MODULE_ID),
        title: "Annotate".to_owned(),
        description: "Objects an operator places on the chart, attributed and removable."
            .to_owned(),
    });
    tables.effects.push(EffectPolicy {
        id: effect(ANNOTATE_EFFECT_ID),
        permission_floor: permission(ANNOTATE_PERMISSION_ID),
        profile_ceilings: BTreeSet::from([profile(ANNOTATOR_PROFILE_ID)]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        risk_reducing_confirmation_class: None,
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
        },
        required_risk_flags: BTreeSet::new(),
        constraints: EffectConstraints {
            required_read_only: Some(false),
            allows_destructive: false,
            durable_requires_reversible: true,
            irreversible_transient_risk: None,
            allows_risk_reducing: false,
        },
    });
}
