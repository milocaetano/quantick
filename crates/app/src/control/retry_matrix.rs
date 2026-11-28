//! What a client does when a call that changes something did not answer: the
//! app's door onto the retry matrix. Why the rows exist is
//! `quantick_control_handlers::wire::retry_matrix`; their join, the check and
//! the renderer are `quantick_control_handlers::retry_matrix`. This module renders
//! `docs/control-plane/retry-matrix.md` from `main`'s dump path.

pub(crate) use quantick_control_handlers::retry_matrix::*;
#[cfg(test)]
pub(crate) use quantick_control_handlers::wire::retry_matrix::*;
// What the sidecar tests reach through `super::*`.
#[cfg(test)]
use super::{SNAPSHOT_CAPABILITY_ID, contract::ObserverContract, workspace};
#[cfg(test)]
use quantick_control::registry::IdempotencyPolicy;

use super::inventory::standard_contract;

/// Render the committed contents of `docs/control-plane/retry-matrix.md`, or
/// say why the rows and the registry disagree.
pub(crate) fn retry_matrix_markdown() -> Result<String, String> {
    let contract = standard_contract()?;
    let findings = drift(READBACKS, contract.capabilities());
    if !findings.is_empty() {
        let lines: Vec<String> = findings.iter().map(ToString::to_string).collect();
        return Err(format!(
            "the retry matrix disagrees with the registry:\n  {}",
            lines.join("\n  ")
        ));
    }
    Ok(render(contract.capabilities()))
}

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
