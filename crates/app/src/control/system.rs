//! Build and runtime identity snapshot.

use quantick_control::{
    handshake::CURRENT_PROTOCOL_VERSION,
    id::{ModuleId, SnapshotScopeId},
    limits::CONTROL_UI_BUDGET_US,
    registry::ModuleDescriptor,
    wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::app::QuantickApp;

use super::registry::{CaptureContext, ProjectionRegistry, ProjectionRegistryError};

pub(crate) const SCOPE_ID: &str = "system.info";
const MODULE_ID: &str = "system";
const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub(crate) struct SystemSnapshot {
    pub application: String,
    pub application_version: String,
    pub control_protocol_version: u32,
    pub target_os: String,
    pub target_arch: String,
    pub target_family: String,
    pub build_profile: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
    pub git_commit_provenance: String,
    #[schemars(extend("x-unit" = "microseconds"))]
    pub ui_capture_budget_us: WireU64,
}

pub(crate) fn register(registry: &mut ProjectionRegistry) -> Result<(), ProjectionRegistryError> {
    let module_id = ModuleId::new(MODULE_ID).expect("static module ID is valid");
    registry.register_module(
        ModuleDescriptor {
            id: module_id.clone(),
            title: "System".to_owned(),
            description: "Application build and runtime identity.".to_owned(),
        },
        revision,
    )?;
    registry.register_scope(
        SnapshotScopeId::new(SCOPE_ID).expect("static scope ID is valid"),
        module_id,
        SCHEMA_VERSION,
        "System information",
        "Reports the Quantick build, target, protocol, and capture budget.",
        project,
    )
}

fn revision(_app: &QuantickApp) -> SystemSnapshot {
    snapshot()
}

fn project(_app: &QuantickApp, _context: CaptureContext) -> SystemSnapshot {
    snapshot()
}

fn snapshot() -> SystemSnapshot {
    let git_commit = option_env!("QUANTICK_GIT_COMMIT").map(str::to_owned);
    SystemSnapshot {
        application: "quantick".to_owned(),
        application_version: env!("CARGO_PKG_VERSION").to_owned(),
        control_protocol_version: CURRENT_PROTOCOL_VERSION,
        target_os: std::env::consts::OS.to_owned(),
        target_arch: std::env::consts::ARCH.to_owned(),
        target_family: std::env::consts::FAMILY.to_owned(),
        build_profile: if cfg!(debug_assertions) {
            "debug".to_owned()
        } else {
            "release".to_owned()
        },
        git_commit_provenance: if git_commit.is_some() {
            "build_environment".to_owned()
        } else {
            "unavailable_in_this_build".to_owned()
        },
        git_commit,
        ui_capture_budget_us: WireU64::new(CONTROL_UI_BUDGET_US),
    }
}
