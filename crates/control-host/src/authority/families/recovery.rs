use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(PermissionDescriptor {
            id: permission(COCKPIT_RECOVER_PERMISSION_ID),
            label: "Rebuild a stalled chart".to_owned(),
            description: "Throw a stalled feed's timeline away and rebuild it, which closes any open paper position (journaled, with its reason) and disarms every strategy.".to_owned(),
            // Marked, and off until ticked: this is the one cockpit act
            // that ends something the trader started. Reconnecting — which
            // keeps the timeline, the position and the strategies — needs
            // none of this and stays under plain `cockpit`.
            sensitive: true,
            default_grant: DefaultGrant::Prompt,
            profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        });
    tables.effects.push(EffectPolicy {
        id: effect(RECOVER_EFFECT_ID),
        permission_floor: permission(COCKPIT_RECOVER_PERMISSION_ID),
        profile_ceilings: BTreeSet::from([profile(COCKPIT_PROFILE_ID)]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        risk_reducing_confirmation_class: None,
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            // Follows the capabilities under it, which cannot claim
            // `destructive` while this host refuses the expected-revision
            // check the registry couples to it — see the note on the
            // descriptor in `super::recovery`. The irreversibility is
            // declared through the required risk flag below and through
            // each capability's `reversible: false`.
            destructive: false,
            // Rebuilding twice rebuilds twice. Each call really does throw
            // a timeline away, so a client must not be told a retry is
            // free.
            idempotent: false,
            open_world: false,
        },
        // Every capability here says, in its own descriptor, that it can
        // cost the trader their timeline.
        required_risk_flags: BTreeSet::from([
            RiskFlagId::new(TIMELINE_REBUILT_RISK_FLAG).expect("static risk flag is valid")
        ]),
        constraints: EffectConstraints {
            required_read_only: Some(false),
            // The one effect in this contract that may. It exists because
            // the honest alternative was worse: a capability that destroys
            // while declaring it does not, so that it could sit under the
            // `cockpit` effect whose own words are "nothing here removes
            // the trader's work".
            allows_destructive: true,
            // A rebuilt chart is durable and cannot be put back. Saying so
            // here is what lets the descriptor say `reversible: false`
            // instead of claiming a reversal it cannot perform.
            durable_requires_reversible: false,
            irreversible_transient_risk: None,
            allows_risk_reducing: false,
        },
    });
}
