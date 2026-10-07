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
mod deal_recording;
mod events;
mod evidence;
mod feed;
mod gateway;
mod health;
mod indicator_guide;
mod interaction;
pub(crate) mod inventory;
mod price_axis;
// Moved to `quantick-control-host`; named here so `super::journal` resolves.
use quantick_control_host::journal;
mod bubble_save;
mod layers;
mod layout;
mod notify;
mod opening_scale;
mod orderflow;
mod ports;
pub(crate) use interaction::drawing_band_name;
mod registry;
pub(crate) mod retry_matrix;
mod scene;
#[cfg(test)]
pub(crate) mod schema_catalog;
pub(crate) mod script;
mod session;
mod system;
mod trace;
pub(crate) mod trade;
mod types;
mod workspace;

pub(crate) use actions::{MARK_CAPABILITY_ID, MARK_CAPABILITY_VERSION};
#[cfg(test)]
pub(crate) use annotate::install_all;
pub(crate) use annotate::quick_range_input;
#[cfg(test)]
pub(crate) use annotate::{
    FIB_CAPABILITY_VERSION, FIB_PROJECTION_CAPABILITY_ID, FIB_RETRACEMENT_CAPABILITY_ID,
    HORIZONTAL_LEVELS_CAPABILITY_ID, PARALLEL_CHANNEL_CAPABILITY_ID, PROFILE_CAPABILITY_ID,
    PROFILE_CAPABILITY_VERSION, RECTANGLE_CAPABILITY_ID, TREND_LINE_CAPABILITY_ID,
};
#[cfg(test)]
pub(crate) use contract::{DESCRIBE_CAPABILITY_ID, SNAPSHOT_CAPABILITY_ID, TRADER_PROFILE_ID};
#[cfg(test)]
pub(crate) use evidence::{RawScreenshot, ScreenshotPixels};
pub(crate) use indicator_guide::INDICATOR_GUIDE_CAPABILITY_ID;
#[cfg(test)]
pub(crate) use scene::scene_snapshot;

/// One journal entry a test can record, so a test about *how many* events a
/// read returns does not have to reach into the journal's own vocabulary.
#[cfg(test)]
pub(crate) fn journal_test_event(index: usize) -> journal::NewEvent {
    journal::NewEvent {
        module_id: quantick_control::id::ModuleId::new("test").expect("static module ID is valid"),
        kind: quantick_control::id::EventKind::new("test.recorded")
            .expect("static event kind is valid"),
        actor: None,
        payload: serde_json::json!({ "index": index }),
    }
}
#[cfg(test)]
pub(crate) use gateway::RecordedActor;
#[cfg(test)]
pub(crate) use gateway::ServedRequest;
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

/// Run the registered action `id` on `window`, as the gateway does once a
/// call is admitted.
#[cfg(test)]
pub(crate) fn invoke_action(
    window: &mut crate::app::ControlWindow,
    access: &mut ControlAccess,
    actor: &quantick_control::wire::ActorContext,
    id: &str,
    input: serde_json::Value,
) -> Result<serde_json::Value, quantick_control::error::ControlError> {
    let actions = actions::standard_actions().expect("built-in action registry must be valid");
    let action = actions
        .lookup(id, quantick_control_host::authority::CAPABILITY_VERSION)
        .expect("the action is registered");
    (action.handler)(window, access, actor, &input)
}

/// Every registered action as its id and version.
#[cfg(test)]
pub(crate) fn registered_action_versions() -> Vec<(String, u32)> {
    actions::standard_actions()
        .expect("built-in action registry must be valid")
        .descriptors()
        .map(|descriptor| (descriptor.id.as_str().to_owned(), descriptor.version))
        .collect()
}

/// Build the initial owner-module registry. Adding a later snapshot module is
/// one registration call here; scope IDs remain open strings in the contract.
pub(crate) fn standard_registry() -> Result<ProjectionRegistry, ProjectionRegistryError> {
    let mut registry = ProjectionRegistry::new(std::sync::Arc::new(registry::SystemClock));
    system::register(&mut registry)?;
    workspace::register(&mut registry)?;
    feed::register(&mut registry)?;
    quantick_control_handlers::chart::register(&mut registry)?;
    health::register(&mut registry)?;
    analysis::register(&mut registry)?;
    interaction::register(&mut registry)?;
    orderflow::register(&mut registry)?;
    session::register(&mut registry)?;
    scene::register(&mut registry)?;
    layers::register(&mut registry)?;
    Ok(registry)
}
