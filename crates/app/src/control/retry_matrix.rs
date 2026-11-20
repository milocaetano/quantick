//! Compatibility entry point for the app's retry-matrix dump flag.
//! Policy, readbacks and rendering belong to the headless crates.

#[cfg(test)]
pub(crate) use quantick_control_handlers::retry_matrix::*;
#[cfg(test)]
pub(crate) use quantick_control_schema::retry_matrix::*;
// What the sidecar tests reach through `super::*`.
#[cfg(test)]
use super::{SNAPSHOT_CAPABILITY_ID, contract::ObserverContract, workspace};
#[cfg(test)]
use quantick_control::registry::IdempotencyPolicy;

#[cfg(test)]
use super::inventory::standard_contract;

pub(crate) use quantick_control_handlers::generation::retry_matrix_markdown;

/// Whether a grant the gateway can hand out reaches `capability` — the rows
/// the transport tests iterate over, decided the way the document decides
/// them rather than by a prefix a test would have to keep in step.
#[cfg(test)]
pub(crate) fn reachable_by_a_grant(capability: &str) -> bool {
    let contract = standard_contract().expect("the registry builds");
    contract
        .registry()
        .capabilities()
        .find(|descriptor| descriptor.id.as_str() == capability)
        .and_then(|descriptor| holder(contract.capabilities(), descriptor))
        .is_some_and(|holder| holder.grantable)
}

#[cfg(test)]
#[path = "retry_matrix_tests.rs"]
mod tests;
