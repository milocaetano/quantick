//! The pane facts the tick sphere rule reads, and wearing what it admitted.
use super::price_axis::{invalid, pane};
use crate::app::control_host::ControlPort;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::bubble_preset::{TickBubblePort, TickSpherePane, WornSpheres};
impl TickBubblePort for dyn ControlPort {
    fn tick_sphere_pane(&mut self, tab: u64, id: u64) -> Result<TickSpherePane, ControlError> {
        let pane = pane(self, WireU64::new(tab), WireU64::new(id))?;
        let asset = pane.orderflow.as_ref().and_then(|owner| owner.asset());
        let asset_mode = asset.is_some_and(|asset| asset.mode().is_some());
        let tick_chart = pane.state.tick_membership().is_some();
        Ok(TickSpherePane::new(tick_chart, asset_mode))
    }
    fn wear_tick_spheres(&mut self, tab: u64, id: u64, name: Option<&str>) -> WornSpheres {
        let pane = pane(self, WireU64::new(tab), WireU64::new(id))?;
        let missing = || invalid("The pane has no order-flow view");
        let owner = pane.orderflow.as_mut().ok_or_else(missing)?;
        let changed = owner.apply_tick_preset(name).map_err(invalid)?;
        let worn = owner.cached_config().volume_dots.tick_bubbles;
        Ok((changed, worn.map(Into::into)))
    }
}
