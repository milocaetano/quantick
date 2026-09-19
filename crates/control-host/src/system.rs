//! Build and runtime identity snapshot.
//!
//! The build facts — the crate version and the commit the build environment
//! stamped — are the application's, so the application hands them in as a
//! [`BuildIdentity`] read at its own compile time; the host never claims its
//! own version is the chart's.

use quantick_control::{
    handshake::CURRENT_PROTOCOL_VERSION, limits::CONTROL_UI_BUDGET_US, wire::WireU64,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCOPE_ID: &str = "system.info";
pub const MODULE_ID: &str = "system";
pub const SCHEMA_VERSION: u32 = 1;

/// What only the application's build knows about itself.
#[derive(Clone, Copy, Debug)]
pub struct BuildIdentity {
    /// The application crate's version, `env!("CARGO_PKG_VERSION")` there.
    pub application_version: &'static str,
    /// The commit the build environment stamped, if it stamped one.
    pub git_commit: Option<&'static str>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SystemSnapshot {
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

/// The snapshot for a build.
#[must_use]
pub fn snapshot(build: BuildIdentity) -> SystemSnapshot {
    let git_commit = build.git_commit.map(str::to_owned);
    SystemSnapshot {
        application: "quantick".to_owned(),
        application_version: build.application_version.to_owned(),
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
