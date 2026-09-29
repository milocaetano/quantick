//! Bind opening-burst scaling to the pane's existing configuration owner.

use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::opening_scale::{OpeningScaleInput, OpeningScaleResult, descriptor};
use serde_json::Value;

use super::{actions::ActionRegistry, gateway::ControlAccess, layout};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: OpeningScaleInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let index = layout::tab_index(
        app,
        layout::TabTarget {
            tab_id: Some(input.tab_id),
        },
    )?;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed"))?;
    let side = tab
        .panes()
        .find(|(pane, _)| pane.id == input.pane_id.get())
        .map(|(_, side)| side)
        .ok_or_else(|| ControlError::invalid_request("opening scale names an unknown pane"))?;
    let view = tab.pane_mut(side).orderflow.as_mut().ok_or_else(|| {
        ControlError::invalid_request("the requested pane has no order-flow view")
    })?;
    let changed = view.set_ignore_opening_burst_in_scale(input.ignore_opening_burst_in_scale);
    serde_json::to_value(OpeningScaleResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        ignore_opening_burst_in_scale: view
            .cached_config()
            .volume_dots
            .ignore_opening_burst_in_scale,
        changed,
    })
    .map_err(|error| ControlError::invalid_request(error.to_string()))
}
