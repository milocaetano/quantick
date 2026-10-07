//! Bind "Save changes for this asset" to the pane's asset binding.

use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::bubble_save::{SaveChangesInput, descriptor};
use serde_json::Value;

use super::actions::{ActionDock, ActionRegistry};
use super::gateway::ControlAccess;
use super::price_axis::{invalid, pane};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: SaveChangesInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let view = pane(app, input.tab_id, input.pane_id)?
        .orderflow
        .as_mut()
        .ok_or_else(|| invalid("the requested pane has no order-flow view"))?;
    let unbound =
        || invalid("the requested pane's bubbles belong to no asset; address the flow pane");
    let asset = view.asset().ok_or_else(unbound)?.key().to_owned();
    let switch = view
        .set_save_asset_changes(input.save_changes)
        .ok_or_else(unbound)?;
    serde_json::to_value(input.result(asset, switch)).map_err(invalid)
}
