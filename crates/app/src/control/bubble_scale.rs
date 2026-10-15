//! Bind bubble magnification to the same pane owner as its sliders.
use crate::app::{TabsMutPort, TabsPort};
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::bubble_scale::{BubbleScaleInput, BubbleScaleTarget, descriptor};
use serde_json::Value;

use super::actions::ActionRegistry;
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
    let input: BubbleScaleInput = serde_json::from_value(value.clone()).map_err(invalid)?;
    let scale = input.validated_scale()?;
    let pane = pane(app, input.tab_id, input.pane_id)?;
    if input.target == BubbleScaleTarget::Candle && pane.state.tick_membership().is_none() {
        return Err(invalid("candle bubble size requires a tick chart"));
    }
    let owner = pane
        .orderflow
        .as_mut()
        .ok_or_else(|| invalid("the requested pane has no order-flow view"))?;
    let target = match input.target {
        BubbleScaleTarget::Tape => quantick_orderflow::BubbleScaleTarget::Tape,
        BubbleScaleTarget::Candle => quantick_orderflow::BubbleScaleTarget::Candle,
    };
    owner
        .bubble_display_scale(target)
        .ok_or_else(|| invalid("the requested bubble display is not active"))?;
    let changed = owner.set_bubble_display_scale(target, scale);
    let actual = owner
        .bubble_display_scale(target)
        .ok_or_else(|| invalid("the requested bubble display is no longer active"))?;
    serde_json::to_value(input.result(actual, changed)?).map_err(invalid)
}
