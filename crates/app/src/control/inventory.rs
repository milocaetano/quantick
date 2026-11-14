//! The capability inventory, rendered from the registry rather than written
//! beside it.
//!
//! `docs/control-plane/capability-inventory.md` used to be prose. It was
//! measured once, at `fcd2ac4`, and by the time anyone read it again the
//! registry had moved 476 commits and the document had not: it listed
//! capabilities that do not exist and omitted most of the ones that do. A
//! reader could not tell which half they were holding.
//!
//! So the file is now output. [`capability_inventory_markdown`] walks the same
//! [`ObserverContract`](super::contract::ObserverContract) the gateway serves
//! and renders every registered capability with the four facts a caller needs
//! before it can invoke one — the identifier, the version it must ask for, the
//! module that owns it, and the permissions the connection must hold. Three
//! things keep the committed copy honest:
//!
//! - `quantick-app --dump-capability-inventory` writes it, so regenerating is
//!   a named command rather than a manual edit.
//! - A test in `crates/app` compares the committed file against this function,
//!   which is the authoritative check: it holds the real registry.
//! - `crates/guards/src/generated.rs` compares the committed file against the
//!   `*_CAPABILITY_ID` constants textually, which is the *fast* check: no
//!   dependencies, no build of this crate, and it runs in the same second as
//!   the rest of the guards.
//!
//! The fast check exists because the authoritative one lives behind the
//! slowest crate in the repository. An agent that adds a capability and
//! forgets to regenerate learns it from `cargo test -p quantick-guards`
//! seconds later, not from CI twenty minutes later.

use quantick_control::registry::CapabilityDescriptor;

use super::contract::ObserverContract;

use quantick_control_host::inventory::render;

/// Render the committed contents of
/// `docs/control-plane/capability-inventory.md`.
///
/// Deterministic by construction: the registry stores capabilities in a
/// `BTreeMap` keyed by `(id, version)`, so the row order is the identifier
/// order and two runs of this function on the same tree cannot disagree. That
/// is what lets the committed copy be compared byte for byte instead of
/// parsed.
pub(crate) fn capability_inventory_markdown() -> Result<String, String> {
    let contract = standard_contract()?;
    let capabilities: Vec<&CapabilityDescriptor> = contract.registry().capabilities().collect();
    Ok(render(&capabilities))
}

/// The contract the gateway serves, built offline for a generator.
///
/// Shared with [`super::retry_matrix`], which renders the same registry from a
/// different angle; two generators each assembling their own contract would be
/// two answers to "which registry is this document about?".
pub(crate) fn standard_contract() -> Result<ObserverContract, String> {
    let projections =
        super::standard_registry().map_err(|error| format!("projection registry: {error}"))?;
    let actions =
        super::actions::standard_actions().map_err(|error| format!("action registry: {error}"))?;
    ObserverContract::new(
        &projections,
        std::sync::Arc::new(actions),
        super::evidence::EvidenceStore::new(),
    )
    .map_err(|error| format!("observer contract: {error}"))
}

#[cfg(test)]
mod tests;
