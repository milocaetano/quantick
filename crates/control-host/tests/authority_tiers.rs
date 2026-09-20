//! The profile chain as authority, read straight from the published tables.
//!
//! The handshake resolves a connection's authority by intersecting two
//! ceilings and then naming the result, which only works while the profiles
//! are a chain. These tests pin that chain, and pin what the `analyst` tier
//! is for: the private reads, and nothing that writes.

use std::collections::BTreeSet;

use quantick_control::{
    handshake::ProfileAuthority,
    id::{PermissionId, ProfileId},
    registry::{ControlRegistry, DefaultGrant},
};
use quantick_control_host::authority::{
    ANALYST_PROFILE_ID, ANNOTATOR_PROFILE_ID, COCKPIT_PROFILE_ID, GRANTABLE_PROFILE_IDS,
    OBSERVER_PROFILE_ID, OBSERVER_SCOPE_IDS, TRADER_PROFILE_ID, permissions, profiles,
};

/// The registry the application builds its authority from: the published
/// profiles and permissions, finalized exactly as the host finalizes them.
fn authority() -> ControlRegistry {
    let mut registry = ControlRegistry::new();
    for descriptor in profiles() {
        registry
            .register_profile(descriptor)
            .expect("the published profiles register");
    }
    for descriptor in permissions() {
        registry
            .register_permission(descriptor)
            .expect("the published permissions register");
    }
    registry
        .finalize_authority()
        .expect("the published authority finalizes");
    registry
}

fn ceiling(registry: &ControlRegistry, id: &str) -> BTreeSet<PermissionId> {
    registry
        .permission_ceiling(&ProfileId::new(id).expect("static profile ID is valid"))
        .unwrap_or_else(|| panic!("`{id}` is a registered profile"))
}

/// A1: the five tiers are a chain, `analyst` sitting above the read-only
/// floor and below everything that writes. The handshake refuses a pair it
/// cannot compare, so a tier bolted on sideways would not be reachable at
/// all — it would take every connection that asked for it down with it.
#[test]
fn the_profiles_are_a_chain_with_the_analyst_above_the_observer() {
    let registry = authority();
    let chain = [
        OBSERVER_PROFILE_ID,
        ANALYST_PROFILE_ID,
        ANNOTATOR_PROFILE_ID,
        COCKPIT_PROFILE_ID,
        TRADER_PROFILE_ID,
    ];
    for pair in chain.windows(2) {
        let lower = ceiling(&registry, pair[0]);
        let upper = ceiling(&registry, pair[1]);
        assert!(
            lower.is_subset(&upper),
            "`{}` must be contained in `{}` for the chain to stay comparable",
            pair[0],
            pair[1]
        );
        assert!(
            lower.len() < upper.len(),
            "`{}` must be strictly above `{}`, or it is not a tier of its own",
            pair[1],
            pair[0]
        );
    }
}

/// A2: the sensitive reads are what the analyst tier carries. Every one of
/// them is ceilinged there and prompts; every ordinary read stays at the
/// observer floor, so enabling read-only access still grants no private data.
#[test]
fn the_sensitive_reads_are_the_analyst_tier_and_the_rest_stay_at_the_floor() {
    let registry = authority();
    let observer = ceiling(&registry, OBSERVER_PROFILE_ID);
    let analyst = ceiling(&registry, ANALYST_PROFILE_ID);
    for (id, _, sensitive) in OBSERVER_SCOPE_IDS {
        let permission = PermissionId::new(*id).expect("static permission ID is valid");
        let descriptor = registry
            .permission(&permission)
            .unwrap_or_else(|| panic!("`{id}` is a registered permission"));
        if *sensitive {
            assert!(
                !observer.contains(&permission),
                "`{id}` is a private read and must not sit at the observer floor"
            );
            assert!(
                analyst.contains(&permission),
                "`{id}` is a private read and belongs to the analyst tier"
            );
            assert_eq!(
                descriptor.default_grant,
                DefaultGrant::Prompt,
                "`{id}` is granted by a tick, never by omission"
            );
        } else {
            assert!(
                observer.contains(&permission),
                "`{id}` is an ordinary read and stays at the observer floor"
            );
        }
    }
}

/// A3: the tier reads and does not write. Stated against the ceilings rather
/// than against a list of prefixes, so a write permission invented tomorrow
/// under a name nobody predicted still fails here if it drifts down.
#[test]
fn nothing_the_analyst_tier_reaches_can_write() {
    let registry = authority();
    let analyst = ceiling(&registry, ANALYST_PROFILE_ID);
    let annotator = ceiling(&registry, ANNOTATOR_PROFILE_ID);
    let cockpit = ceiling(&registry, COCKPIT_PROFILE_ID);
    let trader = ceiling(&registry, TRADER_PROFILE_ID);
    let writes: BTreeSet<PermissionId> = trader
        .difference(&analyst)
        .chain(annotator.difference(&analyst))
        .chain(cockpit.difference(&analyst))
        .cloned()
        .collect();
    for permission in &analyst {
        assert!(
            !writes.contains(permission),
            "`{permission}` writes, so the analyst tier must not reach it"
        );
        assert!(
            permission.as_str() == "observe" || permission.as_str().starts_with("observe."),
            "`{permission}` is not a read, so it does not belong to the analyst tier"
        );
    }
}

/// A4's half that lives in the published tables: the tier is listed as
/// grantable, ordered by ceiling, and the trade tier still is not.
#[test]
fn the_analyst_tier_is_grantable_and_the_trade_tier_is_not() {
    assert_eq!(
        GRANTABLE_PROFILE_IDS,
        [
            OBSERVER_PROFILE_ID,
            ANALYST_PROFILE_ID,
            ANNOTATOR_PROFILE_ID,
            COCKPIT_PROFILE_ID,
        ],
        "the grantable ceilings are listed lowest first"
    );
    assert!(
        !GRANTABLE_PROFILE_IDS.contains(&TRADER_PROFILE_ID),
        "nothing hands out the tier that can trade"
    );
}
