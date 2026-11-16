//! The window's bubble job: resolve the addressed pane, which serves its own
//! bubble calls, and lend the theme a whole look is dressed in.
use super::price_axis::pane;
use crate::app::control_host::ControlPort;
use quantick_appearance::orderflow::OrderflowAppearance;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::bubble_preset::{BubblePane, PaneBubblesPort};
impl PaneBubblesPort for dyn ControlPort {
    type Theme = OrderflowAppearance;
    fn bubble_theme(&self) -> OrderflowAppearance {
        self.appearance().orderflow
    }
    fn pane_bubbles(
        &mut self,
        tab_id: u64,
        pane_id: u64,
    ) -> Result<&mut dyn BubblePane<OrderflowAppearance>, ControlError> {
        Ok(pane(self, WireU64::new(tab_id), WireU64::new(pane_id))?)
    }
}
