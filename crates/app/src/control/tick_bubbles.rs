//! Bind chart-only selection to the existing pane owner.
use super::price_axis::{invalid, pane};
use crate::app::control_host::ControlPort;
use quantick_control::error::ControlError;
use quantick_control_schema::tick_bubbles::{TickBubblesInput, TickBubblesResult};
impl quantick_control_handlers::bubble_preset::TickBubblePort for dyn ControlPort {
    fn set_tick_bubbles(
        &mut self,
        input: &TickBubblesInput,
    ) -> Result<TickBubblesResult, ControlError> {
        let pane = pane(self, input.tab_id, input.pane_id)?;
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
        Ok(quantick_control_schema::tick_bubbles::result(
            input,
            changed,
            owner.cached_config(),
        ))
    }
}
