//! Bind whole-look contracts to the existing pane owner.
use super::price_axis::{invalid, pane};
use crate::app::control_host::ControlPort;
use quantick_control::error::ControlError;
use quantick_control_schema::bubble_preset::{ApplyInput, ApplyResult, ListInput, ListResult};
impl quantick_control_handlers::bubble_preset::BubblePresetPort for dyn ControlPort {
    fn apply_bubble_preset(&mut self, input: &ApplyInput) -> Result<ApplyResult, ControlError> {
        let owner = pane(self, input.tab_id, input.pane_id)?
            .orderflow
            .as_mut()
            .ok_or_else(|| invalid("The pane has no order-flow view"))?;
        let changed = owner.apply_named_preset(&input.preset).map_err(invalid)?;
        Ok(quantick_control_schema::bubble_preset::applied(
            input,
            changed,
            owner.bubbles_snapshot(),
            owner.cached_config(),
        ))
    }
    fn list_bubble_presets(&mut self, input: &ListInput) -> Result<ListResult, ControlError> {
        pane(self, input.tab_id, input.pane_id)?
            .orderflow
            .as_ref()
            .ok_or_else(|| invalid("The pane has no order-flow view"))?
            .bubble_presets_page(input)
            .map_err(invalid)
    }
}
