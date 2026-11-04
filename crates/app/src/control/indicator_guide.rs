//! Durable per-indicator price-hover guide, shared by UI and automation.

pub(crate) use quantick_control_schema::indicator_guide::*;

use crate::app::LayoutPort;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};

use serde_json::Value;

use crate::indicator_worker::SlotId;

use super::{
    actions::ActionRegistry, gateway::ControlAccess, journal::NewEvent, script::SCRIPT_MODULE_ID,
};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: LayoutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: IndicatorGuideInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let changed = crate::app::set_indicator_mouse_vertical_line(
        &mut app.layout_adapter(),
        input.tab_id.get(),
        input.pane_id.get(),
        SlotId(input.slot_id.get()),
        input.enabled,
    );
    if !changed {
        return Err(ControlError::invalid_request(
            "the target must name a live non-price indicator reported by analysis.indicators",
        ));
    }
    let result = IndicatorGuideResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        slot_id: input.slot_id,
        enabled: input.enabled,
    };
    let payload = serde_json::json!({ "indicator_guide": result });
    access.append_event(NewEvent::by(
        SCRIPT_MODULE_ID,
        INDICATOR_GUIDE_EVENT_KIND,
        actor,
        payload,
    ));
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}
