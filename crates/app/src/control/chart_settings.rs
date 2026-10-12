//! The chart settings window and its candle presets, bound to the same
//! owner the double click, the toolbar and the menus reach.

use crate::app::LayersPort;
use crate::candle_presets::PresetError;
use quantick_control::error::{ControlError, codes};
use quantick_control::{id::ErrorCode, registry::RegistryError, wire::ActorContext};
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
        .map_err(preset_error)?;
    access.append_event(NewEvent::by(
        MODULE_ID,
        CHART_SETTINGS_EVENT_KIND,
        actor,
        serde_json::json!({ "chart_settings": result }),
    ));
    serde_json::to_value(result).map_err(|error| invalid(error.to_string()))
}

/// A refusal is the caller's to fix; a failed write is the disk's, nothing
/// changed, and the same call may succeed later.
fn preset_error(error: PresetError) -> ControlError {
    match error {
        PresetError::Refused(reason) => ControlError::invalid_request(reason),
        PresetError::NotWritten(_) => {
            let mut refused = ControlError::new(
                ErrorCode::new(codes::CAPABILITY_UNAVAILABLE).expect("static error code"),
                error.message(),
                true,
            );
            refused.context.details = Some(serde_json::json!({ "reason": "presets_not_written" }));
            refused
        }
    }
}
