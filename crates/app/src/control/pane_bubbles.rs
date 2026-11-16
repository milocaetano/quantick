//! The window's one bubble job: resolve the addressed pane, which serves its
//! own bubble calls, and hand over the theme a whole look is dressed in.
use super::price_axis::pane;
use crate::app::control_host::ControlPort;
use quantick_appearance::orderflow::OrderflowAppearance;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::bubble_preset::{PaneBubblesPort, ResolvedPane};
impl PaneBubblesPort for dyn ControlPort {
    type Theme = OrderflowAppearance;
    fn pane_bubbles(
        &mut self,
        tab_id: u64,
        pane_id: u64,
    ) -> Result<ResolvedPane<'_, Self::Theme>, ControlError> {
        let theme = self.appearance().orderflow;
        let pane = pane(self, WireU64::new(tab_id), WireU64::new(pane_id))?;
        Ok((theme, pane))
    }
}
