//! Bind the headless picker contract to its registered surface.
use super::{actions::ActionRegistry, gateway::ControlAccess, journal::NewEvent};
use crate::app::IndicatorPickerPort;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::indicator_picker::{EVENT_KIND, PickerInput, descriptor};
use serde_json::Value;

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}
fn set<P: IndicatorPickerPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: PickerInput = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = app
        .indicator_picker(input)
        .map_err(ControlError::invalid_request)?;
    access.append_event(NewEvent::by(
        "indicator",
        EVENT_KIND,
        actor,
        serde_json::json!({"indicator_picker": result}),
    ));
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}
