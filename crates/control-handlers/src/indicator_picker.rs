//! The searchable indicator dialog's call: the picker contract over the
//! indicator owner.
use crate::indicator_host::IndicatorControlHost;
use crate::{dock::ActionDock, tabs::JournalAccess};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::indicator_picker::{EVENT_KIND, PickerInput, descriptor};
use serde_json::Value;

/// Dock the picker call.
pub fn register<H: IndicatorControlHost + ?Sized, A: JournalAccess>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: IndicatorControlHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: PickerInput = serde_json::from_value(input.clone())
        .map_err(|e| ControlError::invalid_request(e.to_string()))?;
    let result = host.indicators().indicator_picker(input)?;
    access.record_event(NewEvent::by(
        "indicator",
        EVENT_KIND,
        actor,
        serde_json::json!({"indicator_picker": result}),
    ));
    serde_json::to_value(result).map_err(|e| ControlError::invalid_request(e.to_string()))
}

#[cfg(test)]
#[path = "indicator_picker_tests.rs"]
mod tests;
