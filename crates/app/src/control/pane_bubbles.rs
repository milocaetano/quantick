//! The window's pane jobs: resolve the addressed pane, which serves its own
//! axis, marks, bubble and tape calls, and lend the theme a whole look is dressed in.
use super::addressed_pane::pane;
use crate::app::control_host::ControlPort;
use quantick_appearance::orderflow::OrderflowAppearance;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::addressed_pane::AddressedPanePort;
use quantick_control_handlers::bubble_pane::{BubblePane, PaneBubblesPort};
impl PaneBubblesPort for dyn ControlPort {
    type Theme = OrderflowAppearance;
    fn bubble_theme(&self) -> OrderflowAppearance {
        self.appearance().orderflow
    }
    fn pane_bubbles(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut dyn BubblePane<OrderflowAppearance>, ControlError> {
        Ok(pane(self, tab_id, pane_id)?)
    }
}
impl AddressedPanePort for dyn ControlPort {
    type Pane = crate::pane::ChartPane;
    fn addressed_pane(
        &mut self,
        tab: WireU64,
        id: WireU64,
    ) -> Result<&mut Self::Pane, ControlError> {
        pane(self, tab, id)
    }
}
