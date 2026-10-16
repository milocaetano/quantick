//! The pane facts the bubble size rule reads, and setting the gain it admitted.
use super::price_axis::{invalid, pane};
use crate::app::control_host::ControlPort;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::bubble_scale::{BubbleScalePane, BubbleScalePort};
use quantick_control_schema::bubble_scale::BubbleScaleTarget;
impl BubbleScalePort for dyn ControlPort {
    fn bubble_scale_pane(&mut self, tab: u64, id: u64) -> Result<BubbleScalePane, ControlError> {
        let pane = pane(self, WireU64::new(tab), WireU64::new(id))?;
        let missing = || invalid("the requested pane has no order-flow view");
        pane.bubble_scale_facts().ok_or_else(missing)
    }
    fn set_bubble_scale(
        &mut self,
        tab: u64,
        id: u64,
        target: BubbleScaleTarget,
        scale: f32,
    ) -> Result<(bool, f32), ControlError> {
        let pane = pane(self, WireU64::new(tab), WireU64::new(id))?;
        let missing = || invalid("the requested pane has no order-flow view");
        let owner = pane.orderflow.as_mut().ok_or_else(missing)?;
        let changed = owner.set_bubble_display_scale(target.into(), scale);
        Ok((changed, owner.bubble_display_scale(target.into())))
    }
}
