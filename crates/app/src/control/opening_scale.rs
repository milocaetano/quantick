//! Bind opening-burst scaling to the pane's existing configuration owner.

use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::opening_scale::{OpeningScaleInput, OpeningScaleResult, descriptor};
use serde_json::Value;

use super::price_axis::{invalid, pane};
use super::{actions::ActionRegistry, gateway::ControlAccess};

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<P: TabsPort + TabsMutPort + ?Sized>(
    app: &mut P,
    _access: &mut ControlAccess,
    _actor: &ActorContext,
    value: &Value,
) -> Result<Value, ControlError> {
    let input: OpeningScaleInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let view = pane(app, input.tab_id, input.pane_id)?.orderflow.as_mut();
    let view = view.ok_or_else(|| invalid("the requested pane has no order-flow view"))?;
    let changed = view.set_ignore_opening_burst_in_scale(input.ignore_opening_burst_in_scale);
    let ignore_opening_burst_in_scale = view
        .cached_config()
        .volume_dots
        .ignore_opening_burst_in_scale;
    let (tab_id, pane_id) = (input.tab_id, input.pane_id);
    let result = OpeningScaleResult {
        tab_id,
        pane_id,
        ignore_opening_burst_in_scale,
        changed,
    };
    serde_json::to_value(result).map_err(invalid)
}
