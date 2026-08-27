//! On-demand semantic observation of the running application.
//!
//! This module is the UI-hosted implementation of the transport-neutral
//! contracts in `quantick-control`. Semantic projection stays on a bounded
//! application-thread port; authentication, framing, and socket I/O stay in
//! the local gateway workers.

mod actions;
mod analysis;
mod annotate;
pub(crate) mod chart;
mod contract;
mod events;
mod evidence;
mod feed;
mod gateway;
mod health;
mod interaction;
mod journal;
mod notify;
mod orderflow;
pub(crate) use interaction::drawing_band_name;
mod registry;
mod scene;
#[cfg(test)]
pub(crate) mod schema_catalog;
mod script;
mod session;
mod system;
mod trace;
mod types;
mod workspace;

pub(crate) use actions::{MARK_CAPABILITY_ID, MARK_CAPABILITY_VERSION};
#[cfg(test)]
pub(crate) use contract::{DESCRIBE_CAPABILITY_ID, SNAPSHOT_CAPABILITY_ID};
#[cfg(test)]
pub(crate) use evidence::RawScreenshot;
#[cfg(test)]
pub(crate) use gateway::RecordedActor;
pub(crate) use gateway::{ActionOrigin, ControlAccess, MARK_SHORTCUT};
pub(crate) use notify::AgentPopup;
pub(crate) use types::PaneSideDto;

/// Where the control trace sits beside a recording. Re-exported for the tests
/// that check the file the session scope names is the file the gateway writes.
#[cfg(test)]
pub(crate) fn replay_trace_path_for(session_path: &std::path::Path) -> std::path::PathBuf {
    trace::ReplayTraceFile::path_for(session_path)
}

use registry::{ProjectionRegistry, ProjectionRegistryError};

/// How many actions this build registers — what the catalog test counts
/// against the published surface, so adding one action is one line of code
/// and no arithmetic in a test.
#[cfg(test)]
pub(crate) fn registered_action_count() -> usize {
    actions::standard_actions()
        .expect("built-in action registry must be valid")
        .descriptors()
        .count()
}

/// Build the initial owner-module registry. Adding a later snapshot module is
/// one registration call here; scope IDs remain open strings in the contract.
pub(crate) fn standard_registry() -> Result<ProjectionRegistry, ProjectionRegistryError> {
    let mut registry = ProjectionRegistry::new();
    system::register(&mut registry)?;
    workspace::register(&mut registry)?;
    feed::register(&mut registry)?;
    chart::register(&mut registry)?;
    health::register(&mut registry)?;
    analysis::register(&mut registry)?;
    interaction::register(&mut registry)?;
    orderflow::register(&mut registry)?;
    session::register(&mut registry)?;
    scene::register(&mut registry)?;
    Ok(registry)
}
