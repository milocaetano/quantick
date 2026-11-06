//! The forming bar's progress on the price tag: the pane a call names says
//! which market, and the market settings keep the switch.
use super::price_axis::{invalid, pane};
use crate::app::control_host::ControlPort;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::price_tag_progress::PriceTagProgressPort;

impl PriceTagProgressPort for dyn ControlPort {
    fn set_price_tag_progress(
        &mut self,
        tab: u64,
        id: u64,
        on: bool,
    ) -> Result<bool, ControlError> {
        pane(self, WireU64::new(tab), WireU64::new(id))?;
        let changed = self.layout_adapter().set_bar_progress(tab, on);
        changed.ok_or_else(|| invalid("the tab closed while the call ran"))
    }
}
