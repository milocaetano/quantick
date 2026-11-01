//! The Home tab through a headless host port.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::home_tab::{HomeShowInput, HomeShowResult, descriptor};
use quantick_control_schema::scene::ActiveSurfaceDto;
use serde_json::Value;

pub trait HomeTabHost {
    fn show_home_tab(&mut self) -> Result<(), ControlError> {
        Err(ControlError::invalid_request("the Home tab is unavailable"))
    }
}

/// Outside the replay trace: Home works during a replay because it changes no
/// market, and a recorded call must not bring Home up when a session re-runs.
pub fn register<H: HomeTabHost + ?Sized, A>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), show)
}

fn show<H: HomeTabHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let invalid = |error: serde_json::Error| ControlError::invalid_request(error.to_string());
    let _: HomeShowInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    app.show_home_tab()?;
    serde_json::to_value(HomeShowResult {
        active_surface: ActiveSurfaceDto::Home,
    })
    .map_err(invalid)
}
