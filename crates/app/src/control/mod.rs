//! On-demand semantic observation of the running application.
//!
//! This module is the UI-hosted implementation of the transport-neutral
//! contracts in `quantick-control`. Semantic projection stays on a bounded
//! application-thread port; authentication, framing, and socket I/O stay in
//! the local gateway workers.

mod actions;
pub(crate) mod chart;
mod contract;
mod events;
mod feed;
mod gateway;
mod health;
mod interaction;
mod journal;
pub(crate) use interaction::{cursor_snapshot, drawing_band_name};
mod registry;
#[cfg(test)]
pub(crate) mod schema_catalog;
mod system;
mod trace;
mod types;
mod workspace;

pub(crate) use actions::{MARK_CAPABILITY_ID, MARK_CAPABILITY_VERSION};
#[cfg(test)]
pub(crate) use contract::{DESCRIBE_CAPABILITY_ID, SNAPSHOT_CAPABILITY_ID};
pub(crate) use gateway::{ActionOrigin, ControlAccess, MARK_SHORTCUT};

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
