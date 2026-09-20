//! What keeps the written rows honest against the registry they describe.
//!
//! [`crate::retry_matrix`] states which half of the matrix is derived and
//! which is written. This module is the comparison between them: the
//! generator refuses to render while any [`Drift`] stands.

use super::reach::{Holder, holder};
use crate::readback::Readback;

use quantick_control::id::PermissionId;
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use quantick_control_host::authority::SNAPSHOT_CAPABILITY_ID;
use quantick_control_host::contract::CapabilityContract;
use quantick_control_host::events::READ_CAPABILITY_ID as EVENTS_READ_CAPABILITY_ID;

use serde_json::Value;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// One way the written rows and the registry disagree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Drift {
    /// A mutable capability the matrix has no row for.
    Unmapped { capability: String },
    /// A row for a capability that is not registered, or that cannot change
    /// state and so needs no reconciling.
    Orphan { capability: String },
    /// Two rows for one capability.
    Duplicate { capability: String },
    /// The descriptor publishes a different policy than the row expects.
    PolicyMoved {
        capability: String,
        expected: IdempotencyPolicy,
        published: IdempotencyPolicy,
    },
    /// The row names a read capability the registry does not register.
    UnknownRead { capability: String, read: String },
    /// The row's read capability can itself change state.
    ReadNotReadOnly { capability: String, read: String },
    /// The row names a snapshot scope the registry does not register, or
    /// reads `snapshot.read` without naming one.
    UnknownScope { capability: String, scope: String },
    /// The row's read and its scope or event kind do not fit together: a
    /// snapshot needs a scope, the journal needs an event kind, neither takes
    /// the other's, and no other read has a readback grammar a client (or the
    /// transport tests) could follow.
    ReadShape { capability: String, read: String },
    /// The scope's published schema has no such field.
    FieldMissing {
        capability: String,
        scope: String,
        field: String,
    },
    /// No profile the contract defines holds the capability's permissions.
    NoCeiling { capability: String },
    /// A caller granted the capability could not read its readback: the
    /// read needs a permission beyond the capability's own and the default
    /// read grant (or, for a capability no grant reaches, beyond the ceiling
    /// that holds it). A readback that needs a sensitive scope the trader
    /// never ticked reconciles nothing for the client that needs it.
    ReadbackOutOfReach { capability: String, profile: String },
}

impl fmt::Display for Drift {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unmapped { capability } => write!(
                f,
                "`{capability}` is mutable and has no row: add its readback to READBACKS"
            ),
            Self::Orphan { capability } => write!(
                f,
                "a row covers `{capability}`, which is not a registered mutable capability"
            ),
            Self::Duplicate { capability } => write!(f, "two rows cover `{capability}`"),
            Self::PolicyMoved {
                capability,
                expected,
                published,
            } => write!(
                f,
                "`{capability}` now publishes {published:?}, the row expects {expected:?}: \
                 revisit its readback before updating the row"
            ),
            Self::UnknownRead { capability, read } => write!(
                f,
                "the row for `{capability}` reads back through `{read}`, which is not registered"
            ),
            Self::ReadNotReadOnly { capability, read } => write!(
                f,
                "the row for `{capability}` reads back through `{read}`, which is not read-only"
            ),
            Self::UnknownScope { capability, scope } => write!(
                f,
                "the row for `{capability}` names snapshot scope `{scope}`, which is not registered"
            ),
            Self::ReadShape { capability, read } => write!(
                f,
                "the row for `{capability}` reads back through `{read}` with a scope or event \
                 kind that read does not take: `snapshot.read` names a scope, `events.read` an \
                 event kind, and nothing else is a readback"
            ),
            Self::FieldMissing {
                capability,
                scope,
                field,
            } => write!(
                f,
                "the row for `{capability}` names `{field}`, which the `{scope}` schema does not carry"
            ),
            Self::NoCeiling { capability } => write!(
                f,
                "no profile the contract defines holds every permission `{capability}` requires"
            ),
            Self::ReadbackOutOfReach {
                capability,
                profile,
            } => write!(
                f,
                "a `{profile}` caller granted `{capability}` and the default reads cannot read \
                 its readback"
            ),
        }
    }
}

/// Every disagreement between `rows` and the registry `contract` serves.
///
/// Takes the rows as a parameter rather than reading [`READBACKS`] so a test
/// can hand it a mutated table and watch each check bite.
pub fn drift<P>(rows: &[Readback], contract: &CapabilityContract<P>) -> Vec<Drift> {
    let registry = contract.registry();
    // Every version of a capability shares its row: a new version changes
    // what a call carries, not how a lost one is reconciled, and each version
    // is checked against the row on its own.
    let mut mutable: BTreeMap<&str, Vec<&CapabilityDescriptor>> = BTreeMap::new();
    for descriptor in registry
        .capabilities()
        .filter(|descriptor| !descriptor.read_only)
    {
        mutable
            .entry(descriptor.id.as_str())
            .or_default()
            .push(descriptor);
    }
    let mut findings = Vec::new();
    let mut seen = BTreeMap::<&str, usize>::new();
    for row in rows {
        *seen.entry(row.capability).or_default() += 1;
    }
    for (capability, count) in &seen {
        if *count > 1 {
            findings.push(Drift::Duplicate {
                capability: (*capability).to_owned(),
            });
        }
    }
    for capability in mutable.keys() {
        if !seen.contains_key(capability) {
            findings.push(Drift::Unmapped {
                capability: (*capability).to_owned(),
            });
        }
    }
    for row in rows {
        let Some(versions) = mutable.get(row.capability) else {
            findings.push(Drift::Orphan {
                capability: row.capability.to_owned(),
            });
            continue;
        };
        for descriptor in versions {
            for finding in row_drift(row, descriptor, contract) {
                if !findings.contains(&finding) {
                    findings.push(finding);
                }
            }
        }
    }
    findings
}

pub fn row_drift<P>(
    row: &Readback,
    descriptor: &CapabilityDescriptor,
    contract: &CapabilityContract<P>,
) -> Vec<Drift> {
    let capability = row.capability.to_owned();
    let mut findings = Vec::new();
    if descriptor.idempotency != row.policy {
        findings.push(Drift::PolicyMoved {
            capability: capability.clone(),
            expected: row.policy,
            published: descriptor.idempotency,
        });
    }
    let Some(read) = contract
        .registry()
        .capabilities()
        .find(|candidate| candidate.id.as_str() == row.read)
    else {
        findings.push(Drift::UnknownRead {
            capability,
            read: row.read.to_owned(),
        });
        return findings;
    };
    if !read.read_only {
        findings.push(Drift::ReadNotReadOnly {
            capability: capability.clone(),
            read: row.read.to_owned(),
        });
    }
    let shape_fits = match row.read {
        SNAPSHOT_CAPABILITY_ID => row.event.is_none(),
        EVENTS_READ_CAPABILITY_ID => row.scope.is_none() && row.event.is_some(),
        _ => false,
    };
    if !shape_fits {
        findings.push(Drift::ReadShape {
            capability: capability.clone(),
            read: row.read.to_owned(),
        });
    }
    let mut needed = read.required_permissions.clone();
    if row.read == SNAPSHOT_CAPABILITY_ID {
        let scope_id = row.scope.unwrap_or("");
        match snapshot_scope(contract, scope_id) {
            None => findings.push(Drift::UnknownScope {
                capability: capability.clone(),
                scope: scope_id.to_owned(),
            }),
            Some(scope) => {
                if !schema_has_path(&scope.schema, row.field) {
                    findings.push(Drift::FieldMissing {
                        capability: capability.clone(),
                        scope: scope_id.to_owned(),
                        field: row.field.to_owned(),
                    });
                }
                needed.extend(scope.required_permissions.iter().cloned());
            }
        }
    }
    match holder(contract, descriptor) {
        None => findings.push(Drift::NoCeiling { capability }),
        Some(holder) if !needed.is_subset(&readable(contract, descriptor, &holder)) => {
            findings.push(Drift::ReadbackOutOfReach {
                capability,
                profile: holder.profile.to_owned(),
            });
        }
        Some(_) => {}
    }
    findings
}

/// What a caller granted `capability` can read without asking the trader for
/// anything more: the capability's own permissions and the default read
/// grant. A capability no grant reaches has no such caller yet, so its
/// readback is held to the ceiling that would admit it.
pub fn readable<P>(
    _contract: &CapabilityContract<P>,
    capability: &CapabilityDescriptor,
    holder: &Holder,
) -> BTreeSet<PermissionId> {
    if !holder.grantable {
        return holder.ceiling.clone();
    }
    let mut readable = quantick_control_host::authority::default_grant();
    readable.extend(capability.required_permissions.iter().cloned());
    readable
}

/// The permissions a row's readback needs: its read capability's, and its
/// snapshot scope's when it names one.
pub fn read_needs<P>(contract: &CapabilityContract<P>, row: &Readback) -> BTreeSet<PermissionId> {
    let mut needed = contract
        .registry()
        .capabilities()
        .find(|candidate| candidate.id.as_str() == row.read)
        .map(|read| read.required_permissions.clone())
        .unwrap_or_default();
    if let Some(scope) = row.scope.and_then(|scope| snapshot_scope(contract, scope)) {
        needed.extend(scope.required_permissions.iter().cloned());
    }
    needed
}

/// Whether a JSON schema carries `path` — dotted properties, `[]` stepping
/// into an array's items — following `$ref`s into the schema's own `$defs`
/// and trying every branch of an `anyOf`, `oneOf` or `allOf`.
///
/// The scope's published schema is the one a client validates against, so a
/// field it lacks is a field no client can read, whatever the Rust struct
/// happens to hold today.
pub fn schema_has_path(schema: &Value, path: &str) -> bool {
    let mut nodes = vec![schema];
    for segment in path.split('.') {
        let (name, into_array) = match segment.strip_suffix("[]") {
            Some(name) => (name, true),
            None => (segment, false),
        };
        let mut next = Vec::new();
        for node in nodes {
            for object in branches(schema, node, 0) {
                let Some(property) = object.get("properties").and_then(|map| map.get(name)) else {
                    continue;
                };
                if into_array {
                    next.extend(
                        branches(schema, property, 0)
                            .into_iter()
                            .filter_map(|array| array.get("items")),
                    );
                } else {
                    next.push(property);
                }
            }
        }
        if next.is_empty() {
            return false;
        }
        nodes = next;
    }
    true
}

/// How many `$ref` and combinator hops the schema walk follows before it
/// gives up. The published scope schemas nest a handful deep; the bound exists
/// only so a recursive schema cannot loop, not to fit any real one.
pub const MAX_SCHEMA_HOPS: usize = 16;

/// `node` with its `$ref` resolved and its combinators flattened, bounded by
/// [`MAX_SCHEMA_HOPS`].
pub fn branches<'a>(root: &'a Value, node: &'a Value, depth: usize) -> Vec<&'a Value> {
    if depth > MAX_SCHEMA_HOPS {
        return Vec::new();
    }
    if let Some(reference) = node.get("$ref").and_then(Value::as_str) {
        return root
            .pointer(reference.trim_start_matches('#'))
            .map(|target| branches(root, target, depth + 1))
            .unwrap_or_default();
    }
    let mut out = vec![node];
    for combinator in ["anyOf", "oneOf", "allOf"] {
        if let Some(options) = node.get(combinator).and_then(Value::as_array) {
            for option in options {
                out.extend(branches(root, option, depth + 1));
            }
        }
    }
    out
}

/// One registered snapshot scope, by id — what a named readback is checked
/// against.
fn snapshot_scope<'a, P>(
    contract: &'a CapabilityContract<P>,
    id: &str,
) -> Option<&'a quantick_control_host::catalogue::SnapshotScopeDescriptor> {
    contract
        .snapshot_scopes()
        .iter()
        .find(|descriptor| descriptor.id.as_str() == id)
}
