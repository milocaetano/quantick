//! What a client does when a call that changes something did not answer.
//!
//! A+ gate 5 asks one question of every capability an agent can reach: does it
//! support safe retry and deduplication, or does it declare that it cannot be
//! retried and give enough readback to reconcile an uncertain outcome? The
//! descriptors answer half of that — each publishes an
//! [`IdempotencyPolicy`] — and `gateway/idempotency.rs` enforces it. The other
//! half had no home: *which read* tells a client whether its lost
//! `annotate.label.create` landed was written in prose beside the capability,
//! if anywhere, and nothing failed when a new capability arrived without one.
//!
//! This module is that home. [`READBACKS`] holds one row per mutable
//! capability: the policy it is expected to declare, the read capability (and
//! the snapshot scope or journal event kind) that shows its effect, the field
//! in that read, what that field says when the call applied, and the transport
//! test that proves the row. `docs/control-plane/retry-matrix.md` is the
//! registry rendered through those rows by `quantick-app --dump-retry-matrix`,
//! the way the capability inventory beside it is rendered.
//!
//! # What is derived and what is written
//!
//! Derived from the registry, never written here: which capabilities are
//! mutable, the policy each declares, what the gateway therefore does with a
//! key, and whether any grant reaches the capability at all
//! ([`GRANTABLE_PROFILE_IDS`]). Written here, because no registry knows it:
//! which read reconciles a call. [`drift`] is what keeps the written half
//! honest against the derived half — a mutable capability with no row, a row
//! for nothing, a declared policy the descriptor no longer publishes, a
//! readback capability or scope the registry does not carry or that can
//! change state, a read with a scope or event kind it does not take, a
//! snapshot field the scope's own published schema does not have, or a
//! readback a caller granted the capability could not read. The generator
//! refuses to render while any of those stands. A journal row's field has no
//! published schema to check against — event payloads are not registered —
//! so it is proven where it is used instead: the transport tests read every
//! journal row's field back from a real event.
//!
//! # Cost
//!
//! None at run time. The table is `const` data; [`drift`] and the renderer run
//! only from `main`'s dump path, before a window exists, and from tests.

pub(crate) use quantick_control_schema::retry_matrix::*;
// What the sidecar tests reach through `super::*`.
#[cfg(test)]
use super::{
    contract::{ObserverContract, SNAPSHOT_CAPABILITY_ID},
    workspace,
};
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
