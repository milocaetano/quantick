use super::*;
pub(super) const SCOPES: &[(&str, &str, bool)] = &[(
    "observe.paper",
    "Paper positions, orders, and performance",
    true,
)];
pub(super) fn bind(tables: &mut Tables) {
    tables.permissions.push(// The trade tier, ceilinged at the `trader` profile — which
        // nothing hands out. A permission with no ceiling at all is not
        // representable (`register_permission` refuses it), so the gate
        // is not the ceiling: it is that `configured_profile` never
        // returns `trader` and the access panel never offers the scope.
        // Say that here rather than something tidier, because the next
        // person hardening this tier will read this comment and go
        // looking for the gate it names. `annotate` promises it never
        // affects a position, so a trade cannot borrow it, and deciding
        // which profile *may* trade is a decision about a real account
        // rather than a detail of the change that carved this out.
        PermissionDescriptor {
            id: permission(TRADE_PERMISSION_ID),
            label: "Trade".to_owned(),
            description: "Place, bracket and cancel orders on the charted symbol. Fills are simulated today; the permission exists so that the day they are not, nothing has to be re-decided in a hurry.".to_owned(),
            sensitive: true,
            default_grant: DefaultGrant::Denied,
            profile_ceilings: BTreeSet::from([profile(TRADER_PROFILE_ID)]),
        });
    tables.modules.push(ModuleDescriptor {
        id: module(TRADE_MODULE_ID),
        title: "Trade".to_owned(),
        description:
            "Placing, bracketing and cancelling orders on the charted symbol. Fills are simulated."
                .to_owned(),
    });
    tables.effects.push(EffectPolicy {
        id: effect(TRADE_EFFECT_ID),
        permission_floor: permission(TRADE_PERMISSION_ID),
        profile_ceilings: BTreeSet::from([profile(TRADER_PROFILE_ID)]),
        confirmation_class: confirmation(NO_CONFIRMATION_ID),
        // Cancelling is risk-reducing and crosses no extra gate: taking
        // an order off the book is the direction a trader must never be
        // slowed down in — the same reason flatten is instant.
        risk_reducing_confirmation_class: Some(confirmation(NO_CONFIRMATION_ID)),
        mcp_hint_floor: McpHintFloor {
            read_only: false,
            destructive: false,
            // Placing the same order twice places two orders; there is
            // no key that could make a retry safe.
            idempotent: false,
            open_world: false,
        },
        required_risk_flags: BTreeSet::new(),
        constraints: EffectConstraints {
            required_read_only: Some(false),
            allows_destructive: false,
            durable_requires_reversible: true,
            irreversible_transient_risk: None,
            allows_risk_reducing: true,
        },
    });
}
