//! Durable per-indicator price-hover guide, shared by UI and automation.
use crate::indicator_host::IndicatorHost;
use crate::{dock::ActionDock, tabs::JournalAccess};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_host::{authority::SCRIPT_MODULE_ID, journal::NewEvent};
use quantick_control_schema::indicator_guide::{
    INDICATOR_GUIDE_EVENT_KIND, IndicatorGuideInput, IndicatorGuideResult, descriptor,
};
use serde_json::Value;

/// Dock the guide call.
pub fn register<H: IndicatorHost + ?Sized, A: JournalAccess>(
    registry: &mut impl ActionDock<H, A>,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: IndicatorHost + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: IndicatorGuideInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    host.indicators().set_indicator_guide(
        input.tab_id.get(),
        input.pane_id.get(),
        input.slot_id.get(),
        input.enabled,
    )?;
    let result = IndicatorGuideResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        slot_id: input.slot_id,
        enabled: input.enabled,
    };
    let payload = serde_json::json!({ "indicator_guide": result });
    access.record_event(NewEvent::by(
        SCRIPT_MODULE_ID,
        INDICATOR_GUIDE_EVENT_KIND,
        actor,
        payload,
    ));
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

#[cfg(test)]
#[path = "indicator_guide_tests.rs"]
mod tests;
