use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
            id: permission(COCKPIT_PERMISSION_ID),
            label: "Rearrange the window".to_owned(),
            description: "Change which charts are on screen, where they sit, how wide they are, and their appearance configuration. Never places or removes an object, and never touches a position.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        });
    tables.effects.push(EffectPolicy {
        id: effect(COCKPIT_EFFECT_ID),
        permission_floor: permission(COCKPIT_PERMISSION_ID),
        profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        risk_reducing_confirmation_class: None,
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            destructive: false,
            // Applying the same layout twice leaves the same layout, which
            // is what lets a client retry a dropped call without wondering
            // what it did the first time.
            idempotent: true,
            open_world: false,
        },
        required_risk_flags: BTreeSet::new(),
        constraints: EffectConstraints {
            required_read_only: Some(false),
            // Nothing here removes the trader's work. A layout that hides
            // a pane keeps its drawings and its indicators, which is why
            // rearranging is not destructive even when it takes a chart
            // off the screen.
            allows_destructive: false,
            durable_requires_reversible: true,
            irreversible_transient_risk: None,
            allows_risk_reducing: false,
        },
    });
}
