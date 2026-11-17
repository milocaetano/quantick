//! `layers.visibility.set`: one pane's layer switched the way its menu
//! switches it. The window resolves the pane and reads the layer
//! ([`crate::chart::ChartPort::chart_layer`]); what is refused, what changed
//! and what the journal keeps are decided here.
use quantick_control::{
    error::{ControlError, codes},
    id::ErrorCode,
    registry::RegistryError,
    wire::ActorContext,
};
use quantick_control_host::journal::NewEvent;
use quantick_control_schema::layers::{
    EVENT_KIND, MODULE_ID, SetVisibilityInput, VisibilityResult, set_visibility_descriptor,
};
use serde_json::{Value, json};

use crate::JournalAccess;
use crate::chart::ChartPort;
use crate::dock::ActionDock;
use crate::invalid;

/// Dock `layers.visibility.set`.
///
/// # Errors
/// A capability already docked.
pub fn register<D: ActionDock<H, A>, H: ChartPort + ?Sized, A: JournalAccess>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(set_visibility_descriptor(), set_visibility)
}

fn set_visibility<H: ChartPort + ?Sized, A: JournalAccess>(
    host: &mut H,
    access: &mut A,
    actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: SetVisibilityInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let (tab_id, pane_id) = (input.tab_id, input.pane_id);
    let before = host.chart_layer(tab_id, pane_id, &input.layer_id)?;
    if let Some(blocked) = before.blocked {
        let code = ErrorCode::new(codes::CAPABILITY_UNAVAILABLE).expect("static error code");
        let mut error = ControlError::new(code, blocked.explanation, false);
        error.context.details = Some(json!({ "reason": blocked.code }));
        return Err(error);
    }
    host.switch_chart_layer(tab_id, pane_id, &input.layer_id, input.visible)?;
    let after = host.chart_layer(tab_id, pane_id, &input.layer_id)?;
    let result = serde_json::to_value(VisibilityResult {
        tab_id,
        pane_id,
        changed: before.layer.requested != after.layer.requested,
        layer: after.layer,
    })
    .map_err(invalid)?;
    // Every admitted application, including a no-op, has a bounded readback.
    // The idempotency host replays keyed answers without reaching this point.
    let payload = json!({
        "connection_id": actor.connection_id,
        "request_id": actor.request_id,
        "result": result,
    });
    access.record_event(NewEvent::by(MODULE_ID, EVENT_KIND, actor, payload));
    Ok(result)
}

#[cfg(test)]
#[path = "layers_tests.rs"]
mod tests;
