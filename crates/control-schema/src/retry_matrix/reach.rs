//! Which profile holds a capability, and whether a grant reaches it.
//!
//! Derived from the registry, never written: both the drift checker and the
//! renderer ask the same question of the same contract.

use quantick_control::handshake::ProfileAuthority;
use quantick_control::id::{PermissionId, ProfileId};
use quantick_control::registry::CapabilityDescriptor;
use quantick_control_host::authority::{GRANTABLE_PROFILE_IDS, TRADER_PROFILE_ID};
use quantick_control_host::contract::CapabilityContract;

use std::collections::BTreeSet;

/// Every profile, lowest ceiling first: the ones a grant can hand out, then
/// the one nothing hands out. The first whose ceiling holds a capability's
/// permissions is the one that reaches it.
pub fn profiles_in_ceiling_order() -> impl Iterator<Item = &'static str> {
    GRANTABLE_PROFILE_IDS
        .into_iter()
        .chain(std::iter::once(TRADER_PROFILE_ID))
}

/// The lowest profile whose ceiling admits `capability`, and whether a grant
/// can hand that profile out.
pub fn holder<P>(
    contract: &CapabilityContract<P>,
    capability: &CapabilityDescriptor,
) -> Option<Holder> {
    profiles_in_ceiling_order().find_map(|id| {
        let ceiling = ceiling(contract, id)?;
        capability
            .required_permissions
            .is_subset(&ceiling)
            .then(|| Holder {
                profile: id,
                grantable: GRANTABLE_PROFILE_IDS.contains(&id),
                ceiling,
            })
    })
}

/// The lowest profile whose ceiling admits a capability, and whether a
/// grant can hand that profile out.
pub struct Holder {
    pub profile: &'static str,
    pub grantable: bool,
    pub ceiling: BTreeSet<PermissionId>,
}

pub fn ceiling<P>(
    contract: &CapabilityContract<P>,
    profile: &str,
) -> Option<BTreeSet<PermissionId>> {
    let id = ProfileId::new(profile).ok()?;
    contract.registry().permission_ceiling(&id)
}
