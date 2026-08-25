//! On-demand semantic observation of the running application.
//!
//! This module is the UI-hosted implementation of the transport-neutral
//! contracts in `quantick-control`. It contains no socket and does not run in
//! the application frame loop. PR 3 will connect the same projection port to
//! the authenticated local gateway.

pub(crate) mod chart;
mod feed;
mod health;
mod interaction;
pub(crate) use interaction::drawing_band_name;
mod registry;
pub(crate) mod schema_catalog;
mod system;
mod types;
mod workspace;

use registry::{ProjectionRegistry, ProjectionRegistryError};

/// Build the initial owner-module registry. Adding a later snapshot module is
/// one registration call here; scope IDs remain open strings in the contract.
pub(crate) fn standard_registry() -> Result<ProjectionRegistry, ProjectionRegistryError> {
    let mut registry = ProjectionRegistry::new();
    system::register(&mut registry)?;
    workspace::register(&mut registry)?;
    feed::register(&mut registry)?;
    chart::register(&mut registry)?;
    health::register(&mut registry)?;
    interaction::register(&mut registry)?;
    Ok(registry)
}
