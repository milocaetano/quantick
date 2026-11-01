//! Native presentation requests through a headless host port.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::native_window::{WindowInput, WindowResult, WindowState, descriptor};
use serde_json::Value;

pub trait NativeWindowHost {
    fn request_native_window(&mut self, _: WindowState) -> Result<(), ControlError> {
        Err(ControlError::invalid_request(
            "native window is unavailable",
        ))
    }
}

pub fn register<H: NativeWindowHost + ?Sized, A>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register_outside_replay(descriptor(), set)
}

fn set<H: NativeWindowHost + ?Sized, A>(
    app: &mut H,
    _: &mut A,
    _: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let invalid = |error: serde_json::Error| ControlError::invalid_request(error.to_string());
    let input: WindowInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    app.request_native_window(input.state)?;
    serde_json::to_value(WindowResult {
        requested: input.state,
    })
    .map_err(invalid)
}
