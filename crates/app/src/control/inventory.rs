//! Compatibility entry point for the app's inventory dump flag.
//! The headless generator owns rendering; tests assemble the live registry
//! independently to verify that both composition paths publish identical bytes.

pub(crate) use quantick_control_handlers::generation::capability_inventory_markdown;

#[cfg(test)]
use super::contract::ObserverContract;

/// The contract the gateway serves, built offline for a generator.
///
/// Shared with [`super::retry_matrix`], which renders the same registry from a
/// different angle; two generators each assembling their own contract would be
/// two answers to "which registry is this document about?".
#[cfg(test)]
pub(crate) fn standard_contract() -> Result<ObserverContract, String> {
    let projections =
        super::standard_registry().map_err(|error| format!("projection registry: {error}"))?;
    let actions =
        super::actions::standard_actions().map_err(|error| format!("action registry: {error}"))?;
    ObserverContract::new(
        &projections,
        std::sync::Arc::new(actions),
        super::evidence::EvidenceStore::new(),
        crate::control::system::BUILD,
    )
    .map_err(|error| format!("observer contract: {error}"))
}

#[cfg(test)]
mod tests;
