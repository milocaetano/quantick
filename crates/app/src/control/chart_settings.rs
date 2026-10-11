//! The chart settings window and its candle presets, bound to the same
//! owner the double click, the toolbar and the menus reach.

use crate::app::LayersPort;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::chart::MODULE_ID;
use quantick_control_schema::chart_settings::{
    CHART_SETTINGS_EVENT_KIND, ChartSettingsInput, descriptor,
};
use serde_json::Value;

use super::{actions::ActionRegistry, gateway::ControlAccess, journal::NewEvent};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: LayersPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let invalid = |error: String| ControlError::invalid_request(error);
    let input: ChartSettingsInput =
        serde_json::from_value(input.clone()).map_err(|error| invalid(error.to_string()))?;
    let result = app
        .layer_wiring()
        .chart_settings(input)
        .map_err(|refusal| invalid(refusal.to_owned()))?;
    access.append_event(NewEvent::by(
        MODULE_ID,
        CHART_SETTINGS_EVENT_KIND,
        actor,
        serde_json::json!({ "chart_settings": result }),
    ));
    serde_json::to_value(result).map_err(|error| invalid(error.to_string()))
}
