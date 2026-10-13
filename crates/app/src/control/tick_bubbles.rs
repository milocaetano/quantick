//! Bind the tick-only preset operation to its existing configuration owner.
use super::{
    actions::ActionRegistry,
    gateway::ControlAccess,
    price_axis::{invalid, pane},
};
use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tick_bubbles::{TickBubblesInput, TickBubblesResult, descriptor};
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
    let input: TickBubblesInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let pane = pane(app, input.tab_id, input.pane_id)?;
    if pane.state.tick_membership().is_none() {
        return Err(invalid("Tick sphere presets require a tick chart"));
    }
    let owner = pane
        .orderflow
        .as_mut()
        .ok_or_else(|| invalid("The pane has no order-flow view"))?;
    let changed = owner
        .apply_tick_preset(input.preset.as_deref())
        .map_err(invalid)?;
    serde_json::to_value(TickBubblesResult {
        tab_id: input.tab_id,
        pane_id: input.pane_id,
        changed,
        tick_bubbles: owner
            .cached_config()
            .volume_dots
            .tick_bubbles
            .map(Into::into),
    })
    .map_err(invalid)
}
