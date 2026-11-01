//! Bring the Home tab forward, and put the market back. Presentation only: no
//! market, feed or worker changes, and nothing is saved.
use quantick_control::registry::{CapabilityDescriptor, IdempotencyPolicy};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scene::ActiveSurfaceDto;

const RESULT_MAX_BYTES: usize = 512;

pub const HOME_SHOW_CAPABILITY_ID: &str = "home.show";
pub const HOME_LEAVE_CAPABILITY_ID: &str = "home.leave";

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HomeShowInput {}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HomeShowResult {
    pub active_surface: ActiveSurfaceDto,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HomeLeaveInput {}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HomeLeaveResult {
    pub active_surface: ActiveSurfaceDto,
}

pub fn descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<HomeShowInput, HomeShowResult>(
        HOME_SHOW_CAPABILITY_ID,
        quantick_control_host::authority::HOME_MODULE_ID,
        "Show the Home tab",
        "Shows Home over the central canvas, as the first tab, Ctrl+1 and Ctrl+Shift+H do. Every market stays open and streaming behind it, and it works during a replay. Opening or activating a market leaves Home, as home.leave does. Control-plane writes keep targeting the active market while Home shows.",
        "Showing Home twice is the same state, so a stale or repeated call can only show Home.",
    );
    descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
    descriptor
}

pub fn leave_descriptor() -> CapabilityDescriptor {
    let mut descriptor = crate::layout::transient_descriptor::<HomeLeaveInput, HomeLeaveResult>(
        HOME_LEAVE_CAPABILITY_ID,
        quantick_control_host::authority::HOME_MODULE_ID,
        "Leave the Home tab",
        "Puts the active market back on screen over Home, as its chip and Ctrl+Tab do. No market changes, and on a market already it does nothing.",
        "Leaving Home twice is the same state, so a stale or repeated call can only show the active market.",
    );
    descriptor.expected_cost.max_response_bytes = Some(RESULT_MAX_BYTES);
    descriptor
}

/// The scene names the surface on screen; `tab_strip.home` carries the bounds.
pub const READBACKS: &[crate::readback::Readback] = &[
    crate::readback::snapshot(
        HOME_SHOW_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::scene::CONTROLS_SCOPE_ID,
        "active_surface",
        "active_surface is home, and the tab_strip.home control is selected",
        &[crate::readback::EVERY_OPTIONAL_TEST],
    ),
    crate::readback::snapshot(
        HOME_LEAVE_CAPABILITY_ID,
        IdempotencyPolicy::Optional,
        crate::scene::CONTROLS_SCOPE_ID,
        "active_surface",
        "active_surface is market, and the active market's panes are listed",
        &[crate::readback::EVERY_OPTIONAL_TEST],
    ),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn additional_input_fields_are_rejected() {
        assert!(serde_json::from_value::<HomeShowInput>(serde_json::json!({})).is_ok());
        let extra = serde_json::json!({"tab_id": "1"});
        assert!(serde_json::from_value::<HomeShowInput>(extra.clone()).is_err());
        assert!(serde_json::from_value::<HomeLeaveInput>(serde_json::json!({})).is_ok());
        assert!(serde_json::from_value::<HomeLeaveInput>(extra).is_err());
    }
}
