//! Native presentation requests through a headless host port.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::native_window::{WindowInput, WindowResult, WindowState, descriptor};
use serde_json::Value;

pub trait NativeWindowHost {
    fn request_targeted_native_window(
        &mut self,
        input: WindowInput,
    ) -> Result<quantick_control::wire::WireU64, ControlError> {
        if input.window_id.is_some_and(|id| id.get() != 0) {
            return Err(ControlError::invalid_request(
                "market_window.missing_window",
            ));
        }
        self.request_native_window(input.state)?;
        Ok(quantick_control::wire::WireU64::new(0))
    }
    fn market_windows(&self) -> quantick_control_schema::market_windows::WindowsSnapshot {
        quantick_control_schema::market_windows::WindowsSnapshot::default()
    }
    fn operate_market_tabs(
        &mut self,
        _: quantick_control_schema::market_windows::TabOperation,
    ) -> Result<quantick_control_schema::market_windows::TabOperationResult, ControlError> {
        Err(ControlError::invalid_request(
            "market windows are unavailable",
        ))
    }
    fn place_native_window(
        &mut self,
        _: quantick_control_schema::market_windows::PlacementInput,
    ) -> Result<quantick_control_schema::market_windows::PlacementResult, ControlError> {
        Err(ControlError::invalid_request(
            "native window placement is unavailable",
        ))
    }
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
    let window_id = app.request_targeted_native_window(input)?;
    serde_json::to_value(WindowResult {
        requested: input.state,
        window_id: Some(window_id),
    })
    .map_err(invalid)
}
