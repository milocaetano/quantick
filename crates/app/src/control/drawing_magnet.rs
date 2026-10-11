//! The drawing rail's OHLC magnet, shared by the rail button and automation,
//! and each drawing's own, shared by its context bar and automation.

pub(crate) use quantick_control_schema::drawing_magnet::*;

use crate::app::{TabsMutPort, TabsPort, ToolRailPort};
use crate::drawings::DrawingId;

use quantick_control::{
    error::ControlError,
    registry::RegistryError,
    wire::{ActorContext, WireU64},
};

use quantick_control::annotation::ANNOTATE_MODULE_ID;
use serde_json::Value;

use super::{actions::ActionRegistry, gateway::ControlAccess, journal::NewEvent};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(rail_descriptor(), set)
}

fn set<P: ToolRailPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: DrawingMagnetInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let result = DrawingMagnetResult {
        enabled: input.enabled,
        changed: app.set_drawing_magnet(input.enabled),
    };
    let payload = serde_json::json!({ "drawing_magnet": result });
    access.append_event(NewEvent::by(
        ANNOTATE_MODULE_ID,
        DRAWING_MAGNET_EVENT_KIND,
        actor,
        payload,
    ));
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// Dock `annotate.drawing_magnet.set`: one drawing's own magnet, the switch
/// its context bar flips.
pub(crate) fn register_object(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(object_descriptor(), set_object)
}

fn set_object<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    access: &mut ControlAccess,
    actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: ObjectMagnetInput = serde_json::from_value(input.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let id = DrawingId(input.drawing_id.get());
    let mut found = None;
    for (tab_index, tab) in app.tab_reads().tabs().iter().enumerate() {
        for side in tab.sides() {
            let pane = tab.pane(side);
            if let Some(index) = pane.drawings.index_of(id) {
                found = Some((tab_index, side, index));
            }
        }
    }
    let Some((tab_index, side, index)) = found else {
        return Err(ControlError::invalid_request(format!(
            "no open chart holds a drawing with id {}",
            id.0
        )));
    };
    let tab_id = app.tab_reads().tabs().id_at(tab_index);
    let pane = app.tabs_mut().pane_mut(tab_index, side);
    let result = ObjectMagnetResult {
        drawing_id: input.drawing_id,
        tab_id: WireU64::new(tab_id),
        pane_id: WireU64::new(pane.id),
        enabled: input.enabled,
        changed: pane.drawings.set_magnet_at(index, input.enabled),
    };
    let payload = serde_json::json!({ "drawing_magnet": result });
    access.append_event(NewEvent::by(
        ANNOTATE_MODULE_ID,
        OBJECT_MAGNET_EVENT_KIND,
        actor,
        payload,
    ));
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}
