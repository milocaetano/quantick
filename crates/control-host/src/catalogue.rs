//! The snapshot-scope catalogue: every scope a projection registry declares,
//! as the contract publishes it in `describe` and checks a grant against.
//!
//! It sits between the two halves of this crate — built from a
//! [`ProjectionRegistry`] once, at contract construction, and read on every
//! request that names a scope — and belongs to neither admission nor
//! projection.

use std::collections::{BTreeMap, BTreeSet};

use quantick_control::{
    id::{ModuleId, PermissionId, SnapshotScopeId},
    limits::CONTROL_MAX_SNAPSHOT_SCOPES,
    registry::{PermissionDescriptor, RegistryError},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::projection::ProjectionRegistry;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotScopeDescriptor {
    pub id: SnapshotScopeId,
    pub module_id: ModuleId,
    pub schema_version: u32,
    pub title: String,
    pub description: String,
    pub required_permissions: BTreeSet<PermissionId>,
    pub schema: Value,
}

/// Every snapshot scope a projection registry declares, as the contract
/// publishes it (sorted by ID), and the permissions each one needs. A scope
/// naming a permission the authority table does not know is refused.
pub type SnapshotScopeCatalogue = (
    BTreeMap<SnapshotScopeId, BTreeSet<PermissionId>>,
    Vec<SnapshotScopeDescriptor>,
);

/// Build the [`SnapshotScopeCatalogue`] of `projections` against the
/// registered `permissions`.
pub fn snapshot_scope_catalogue<H: ?Sized + 'static>(
    projections: &ProjectionRegistry<H>,
    permissions: &[PermissionDescriptor],
) -> Result<SnapshotScopeCatalogue, RegistryError> {
    let mut scope_permissions = BTreeMap::new();
    let mut snapshot_scopes = Vec::new();
    for descriptor in projections.descriptors() {
        let required_permissions = descriptor.required_permissions.clone();
        for permission in &required_permissions {
            if !permissions.iter().any(|known| known.id == *permission) {
                return Err(RegistryError::Unknown {
                    kind: "permission",
                    id: permission.to_string(),
                });
            }
        }
        scope_permissions.insert(descriptor.scope_id.clone(), required_permissions.clone());
        snapshot_scopes.push(SnapshotScopeDescriptor {
            id: descriptor.scope_id.clone(),
            module_id: descriptor.module_id.clone(),
            schema_version: descriptor.schema_version,
            title: descriptor.title.clone(),
            description: descriptor.description.clone(),
            required_permissions,
            schema: descriptor.schema.clone(),
        });
    }
    snapshot_scopes.sort_by(|left, right| left.id.cmp(&right.id));
    Ok((scope_permissions, snapshot_scopes))
}

/// Every scope in `snapshot_scopes` this grant already reaches, in the order
/// given (the catalogue's, sorted by scope ID) and capped at what one capture
/// may carry.
pub fn readable_scopes(
    snapshot_scopes: &[SnapshotScopeDescriptor],
    grant: &BTreeSet<PermissionId>,
) -> Vec<SnapshotScopeId> {
    snapshot_scopes
        .iter()
        .filter(|descriptor| descriptor.required_permissions.is_subset(grant))
        .map(|descriptor| descriptor.id.clone())
        .take(CONTROL_MAX_SNAPSHOT_SCOPES)
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use quantick_control::registry::{DefaultGrant, ModuleDescriptor};
    use schemars::JsonSchema;
    use serde::Serialize;

    use super::*;
    use crate::clock::HostClock;

    struct NoClock;

    impl HostClock for NoClock {
        fn unix_ms(&self) -> i64 {
            0
        }

        fn monotonic(&self) -> std::time::Duration {
            std::time::Duration::ZERO
        }
    }

    #[derive(Serialize, JsonSchema)]
    struct Empty {}

    /// A registry with one module and a scope per `(id, permission)` pair.
    fn projections(scopes: &[(&str, &str)]) -> ProjectionRegistry<()> {
        let mut registry = ProjectionRegistry::new(Arc::new(NoClock));
        registry
            .register_module(
                ModuleDescriptor {
                    id: ModuleId::new("quote").unwrap(),
                    title: "Quote".to_owned(),
                    description: "Test scopes.".to_owned(),
                },
                |_: &()| 0_u8,
            )
            .unwrap();
        for (id, permission) in scopes {
            registry
                .register_scope(
                    SnapshotScopeId::new(*id).unwrap(),
                    ModuleId::new("quote").unwrap(),
                    1,
                    "Scope",
                    "A test scope.",
                    &[permission],
                    |_: &(), _| Empty {},
                )
                .unwrap();
        }
        registry
    }

    fn permission(id: &str) -> PermissionDescriptor {
        PermissionDescriptor {
            id: PermissionId::new(id).unwrap(),
            label: id.to_owned(),
            description: "A test permission.".to_owned(),
            sensitive: false,
            default_grant: DefaultGrant::Granted,
            profile_ceilings: BTreeSet::new(),
        }
    }

    #[test]
    fn the_catalogue_is_sorted_by_scope_id_and_maps_each_scope_to_its_permissions() {
        let known = [permission("observe"), permission("observe.paper")];
        let (scope_permissions, scopes) = snapshot_scope_catalogue(
            &projections(&[("quote.last", "observe.paper"), ("quote.bid", "observe")]),
            &known,
        )
        .unwrap();
        let ids: Vec<_> = scopes.iter().map(|scope| scope.id.as_str()).collect();
        assert_eq!(ids, ["quote.bid", "quote.last"]);
        assert_eq!(
            scope_permissions[&SnapshotScopeId::new("quote.last").unwrap()],
            BTreeSet::from([PermissionId::new("observe.paper").unwrap()])
        );
    }

    #[test]
    fn a_scope_naming_an_unknown_permission_is_refused() {
        let error = snapshot_scope_catalogue(
            &projections(&[("quote.bid", "observe.nothing")]),
            &[permission("observe")],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            RegistryError::Unknown {
                kind: "permission",
                ..
            }
        ));
    }

    #[test]
    fn a_grant_reads_only_the_scopes_it_covers() {
        let known = [permission("observe"), permission("observe.paper")];
        let (_, scopes) = snapshot_scope_catalogue(
            &projections(&[("quote.last", "observe.paper"), ("quote.bid", "observe")]),
            &known,
        )
        .unwrap();
        let grant = BTreeSet::from([PermissionId::new("observe").unwrap()]);
        assert_eq!(
            readable_scopes(&scopes, &grant),
            vec![SnapshotScopeId::new("quote.bid").unwrap()]
        );
    }
}
