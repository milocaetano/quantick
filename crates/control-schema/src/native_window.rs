//! Explicit native window requests; no market or worker state is changed.
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const RESULT_MAX_BYTES: usize = 512;

pub const WINDOW_STATE_CAPABILITY_ID: &str = "layout.window.state.set";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WindowState {
    Normal,
    Maximized,
    Minimized,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WindowInput {
    pub state: WindowState,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
pub struct WindowResult {
    /// Accepted for the next native frame; this is not an OS acknowledgement.
    pub requested: WindowState,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<WindowInput, WindowResult>(
        WINDOW_STATE_CAPABILITY_ID,
        crate::layout::LAYOUT_MODULE_ID,
        "Set native window state",
        "Requests normal, maximized or minimized presentation state for the single native window. Application exit is excluded from agent authority.",
        "Explicit states never toggle. A required idempotency key replays the accepted request without minimizing twice.",
    );
    descriptor.idempotency = IdempotencyPolicy::Required;
    descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
    descriptor
}

/// One bounded native request waits for the UI thread to deliver it.
#[derive(Default)]
pub struct WindowRequests {
    pending: Option<WindowState>,
}
impl WindowRequests {
    pub fn request(
        &mut self,
        state: WindowState,
    ) -> Result<(), quantick_control::error::ControlError> {
        if self.pending.is_some_and(|pending| pending != state) {
            return Err(quantick_control::error::ControlError::invalid_request(
                "a native window request is already pending",
            ));
        }
        self.pending = Some(state);
        Ok(())
    }
    pub fn take(&mut self) -> Option<WindowState> {
        self.pending.take()
    }
}

/// Scene selection reports OS-observed presentation, independently of acceptance.
pub const READBACKS: &[crate::readback::Readback] = &[crate::readback::snapshot(
    WINDOW_STATE_CAPABILITY_ID,
    IdempotencyPolicy::Required,
    crate::scene::CONTROLS_SCOPE_ID,
    "controls[].selected",
    "tab_strip.maximize or tab_strip.minimize is selected once the native viewport reports that state; both clear in normal state",
    &["native_window_requests_validate_authority_and_retry_without_repeating_the_effect"],
)];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_requests_are_bounded_and_explicit_retries_are_safe() {
        let mut requests = WindowRequests::default();
        requests.request(WindowState::Maximized).unwrap();
        requests.request(WindowState::Maximized).unwrap();
        assert!(requests.request(WindowState::Normal).is_err());
        assert_eq!(requests.take(), Some(WindowState::Maximized));
        assert_eq!(requests.take(), None);
        requests.request(WindowState::Normal).unwrap();
        assert_eq!(requests.take(), Some(WindowState::Normal));
    }
    #[test]
    fn malformed_or_additional_window_fields_are_rejected() {
        for input in [
            serde_json::json!({"state":"toggle"}),
            serde_json::json!({"state":"maximized", "extra":true}),
        ] {
            assert!(serde_json::from_value::<WindowInput>(input).is_err());
        }
    }
}
