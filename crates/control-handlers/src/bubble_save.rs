//! "Save changes for this asset" over the addressed pane's bubble look: the
//! window resolves the pane, the look's asset binding answers the switch.
use crate::bubble_preset::{PaneBubblesPort, invalid};
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::bubble_save::{SaveChangesInput, descriptor};
use serde_json::Value;

pub fn register<D: ActionDock<H, A>, H: PaneBubblesPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: PaneBubblesPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: SaveChangesInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let (_, pane) = host.pane_bubbles(input.tab_id.get(), input.pane_id.get())?;
    let view = pane
        .bubble_look()
        .ok_or_else(|| invalid("the requested pane has no order-flow view"))?;
    let unbound =
        || invalid("the requested pane's bubbles belong to no asset; address the flow pane");
    let asset = view.asset_key().ok_or_else(unbound)?.to_owned();
    let switch = view
        .set_save_asset_changes(input.save_changes)
        .ok_or_else(unbound)?;
    serde_json::to_value(input.result(asset, switch)).map_err(invalid)
}

#[cfg(test)]
#[path = "bubble_save_tests.rs"]
mod tests;
