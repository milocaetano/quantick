//! The drawing rail's OHLC magnet, shared by the rail button and automation,
//! and each drawing's own, shared by its context bar and automation.

pub(crate) use quantick_control_schema::drawing_magnet::*;

use crate::app::{GatewayPort, TabsMutPort, TabsPort, ToolRailPort};

use super::price_axis::invalid;
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
    let input: ObjectMagnetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let pane = super::price_axis::pane(app, input.tab_id, input.pane_id)?;
    let id = DrawingId(input.drawing_id.get());
    let index = pane
        .drawings
        .index_of(id)
        .ok_or_else(|| input.unknown_drawing())?;
    let drawing = &pane.drawings.items()[index];
    if !drawing.takes_magnet() {
        return Err(no_magnet(drawing.tool.name(), drawing.tool.id()));
    }
    let result = input.result(pane.drawings.set_magnet_at(index, input.enabled));
    let payload = serde_json::json!({ "drawing_magnet": result });
    access.append_event(NewEvent::by(
        ANNOTATE_MODULE_ID,
        OBJECT_MAGNET_EVENT_KIND,
        actor,
        payload,
    ));
    serde_json::to_value(result).map_err(|error| ControlError::invalid_request(error.to_string()))
}

/// A hand's flip of the rail's magnet is `annotate.magnet.set`, so it
/// journals alike. Without control access it lands directly.
pub(crate) fn flip_rail_magnet<P: GatewayPort + ToolRailPort + ?Sized>(app: &mut P, on: bool) {
    let input = serde_json::json!({ "enabled": on });
    let human = super::ActionOrigin::Human;
    if app
        .control_action(DRAWING_MAGNET_CAPABILITY_ID, 1, human, input)
        .is_err()
    {
        app.set_drawing_magnet(on);
    }
}

/// A hand's flip of the selected drawing's own magnet (the context bar's
/// slot, when it asked for one) is `annotate.drawing_magnet.set`, so it
/// journals alike. Without control access it lands directly; one undo step
/// either way.
pub(crate) fn flip_selected_drawing_magnet<P>(app: &mut P, on: Option<bool>)
where
    P: GatewayPort + TabsPort + TabsMutPort + ?Sized,
{
    let Some(on) = on else {
        return;
    };
    let reads = app.tab_reads();
    let tab_index = reads.active_tab_index();
    let tabs = reads.tabs();
    let side = tabs[tab_index].drawing_side();
    let pane = tabs[tab_index].pane(side);
    let Some(index) = pane.drawings.selected() else {
        return;
    };
    let input = ObjectMagnetInput {
        tab_id: WireU64::new(tabs.id_at(tab_index)),
        pane_id: WireU64::new(pane.id),
        drawing_id: WireU64::new(pane.drawings.items()[index].id.0),
        enabled: on,
    };
    let human = super::ActionOrigin::Human;
    let called = serde_json::to_value(input).is_ok_and(|input| {
        app.control_action(OBJECT_MAGNET_CAPABILITY_ID, 1, human, input)
            .is_ok()
    });
    if !called {
        let pane = app.tabs_mut().pane_mut(tab_index, side);
        pane.drawings.set_magnet_at(index, on);
    }
}
