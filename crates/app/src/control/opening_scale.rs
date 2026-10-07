//! Bind opening-burst scaling to the pane's existing configuration owner.

use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::opening_scale::{OpeningScaleInput, OpeningScaleTarget, descriptor};
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
    let pane = pane(app, input.tab_id, input.pane_id)?;
    let owner = pane
        .orderflow
        .as_mut()
        .ok_or_else(|| invalid("the requested pane has no order-flow view"))?;
    let changed = match input.target {
        OpeningScaleTarget::Candle => {
            if pane.state.tick_membership().is_none() || !owner.flow_execution_active() {
                return Err(invalid(
                    "regional opening scale requires active tick FLOW bubbles",
                ));
            }
            owner.set_ignore_flow_opening(input.ignore_opening_burst_in_scale)
        }
        OpeningScaleTarget::Tape => owner.edit_config(|config| {
            config.set_ignore_opening_burst_in_scale(input.ignore_opening_burst_in_scale)
        }),
    };
    serde_json::to_value(input.result(changed)).map_err(invalid)
}
