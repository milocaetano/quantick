//! Named price framing over the same pane state as its gutter gesture.
use super::{actions::ActionRegistry, gateway::ControlAccess, layout};
use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::price_axis::{PriceAxisInput, PriceAxisResult, descriptor};
use serde_json::Value;

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: PriceAxisInput = serde_json::from_value(value.clone())
        .map_err(|error| ControlError::invalid_request(error.to_string()))?;
    let range = input.mode.range()?;
    let index = layout::tab_index(
        app,
        layout::TabTarget {
            tab_id: Some(input.tab_id),
        },
    )?;
    let tab = app
        .tabs_mut()
        .tab_at_mut(index)
        .ok_or_else(|| ControlError::invalid_request("the tab closed while the call ran"))?;
    let pane = tab
        .panes_mut()
        .find(|pane| pane.id == input.pane_id.get())
        .ok_or_else(|| {
            ControlError::invalid_request("price axis names an unknown pane on the requested tab")
        })?;
    pane.sync_price_axis_mode();
    match range {
        Some((low, high)) => {
            pane.price_view.set_manual_range(low, high);
        }
        None => pane.price_view.reset(),
    }
    serde_json::to_value(PriceAxisResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        viewport: super::chart::viewport_snapshot(pane),
    })
    .map_err(|error| ControlError::invalid_request(error.to_string()))
}
