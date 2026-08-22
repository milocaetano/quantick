//! Stable, redacted errors intended for automated clients.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    id::{ErrorCode, PreconditionId},
    wire::ModuleRevision,
};

pub mod codes {
    pub const AUTH_FAILED: &str = "control.auth_failed";
    pub const BACKPRESSURE: &str = "control.backpressure";
    pub const CAPABILITY_UNKNOWN: &str = "control.capability_unknown";
    pub const CAPABILITY_UNAVAILABLE: &str = "control.capability_unavailable";
    pub const CURSOR_INVALID: &str = "control.cursor_invalid";
    pub const IDEMPOTENCY_CONFLICT: &str = "control.idempotency_conflict";
    pub const INSTANCE_AMBIGUOUS: &str = "control.instance_ambiguous";
    pub const INSTANCE_GONE: &str = "control.instance_gone";
    pub const INVALID_REQUEST: &str = "control.invalid_request";
    pub const PAGE_STALE: &str = "control.page_stale";
    pub const PAYLOAD_TOO_LARGE: &str = "control.payload_too_large";
    pub const PERMISSION_DENIED: &str = "control.permission_denied";
    pub const REVISION_CONFLICT: &str = "control.revision_conflict";
    pub const REQUEST_IN_PROGRESS: &str = "control.request_in_progress";
    pub const RESOURCE_GONE: &str = "control.resource_gone";
    pub const SCOPE_DENIED: &str = "control.scope_denied";
    pub const TIMEOUT: &str = "control.timeout";
    pub const VERSION_UNSUPPORTED: &str = "control.version_unsupported";

    pub const ALL: &[&str] = &[
        AUTH_FAILED,
        BACKPRESSURE,
        CAPABILITY_UNKNOWN,
        CAPABILITY_UNAVAILABLE,
        CURSOR_INVALID,
        IDEMPOTENCY_CONFLICT,
        INSTANCE_AMBIGUOUS,
        INSTANCE_GONE,
        INVALID_REQUEST,
        PAGE_STALE,
        PAYLOAD_TOO_LARGE,
        PERMISSION_DENIED,
        REVISION_CONFLICT,
        REQUEST_IN_PROGRESS,
        RESOURCE_GONE,
        SCOPE_DENIED,
        TIMEOUT,
        VERSION_UNSUPPORTED,
    ];
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ControlError {
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    #[serde(flatten)]
    pub context: Box<ControlErrorContext>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ControlErrorContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_revisions: Vec<ModuleRevision>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub violated_precondition: Option<PreconditionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_id: Option<String>,
}

impl ControlError {
    pub fn new(code: ErrorCode, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code,
            message: message.into(),
            retryable,
            context: Box::new(ControlErrorContext {
                current_revisions: Vec::new(),
                violated_precondition: None,
                details: None,
                next_steps: Vec::new(),
                diagnostic_id: None,
            }),
        }
    }

    pub(crate) fn known(code: &'static str, message: impl Into<String>, retryable: bool) -> Self {
        Self::new(
            ErrorCode::new(code).expect("library error codes are valid registry IDs"),
            message,
            retryable,
        )
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::known(codes::INVALID_REQUEST, message, false)
    }

    pub fn revision_conflict(current_revisions: Vec<ModuleRevision>) -> Self {
        let mut error = Self::known(
            codes::REVISION_CONFLICT,
            "state changed after the request was prepared",
            true,
        );
        error.context.current_revisions = current_revisions;
        error.context.next_steps = vec!["Read fresh state and retry with its revision.".to_owned()];
        error
    }

    pub fn idempotency_conflict() -> Self {
        Self::known(
            codes::IDEMPOTENCY_CONFLICT,
            "the idempotency key was already used with different input",
            false,
        )
    }

    /// An append-only or revision-locked page can no longer be continued
    /// against the source it was opened on.
    pub fn page_stale(message: impl Into<String>) -> Self {
        Self::known(codes::PAGE_STALE, message, true)
    }
}
