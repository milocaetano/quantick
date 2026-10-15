//! The pane facts the tick sphere rule reads, and wearing what it admitted.
use super::price_axis::{invalid, pane};
use crate::app::control_host::ControlPort;
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_handlers::bubble_preset::{TickBubblePort, TickSpherePane};
use quantick_control_schema::tick_bubbles::TickBubblesSnapshot;
impl TickBubblePort for dyn ControlPort {
    fn tick_sphere_pane(&mut self, tab: u64, id: u64) -> Result<TickSpherePane, ControlError> {
        let pane = pane(self, WireU64::new(tab), WireU64::new(id))?;
        let tick_chart = pane.state.tick_membership().is_some();
        let owner = pane.orderflow.as_ref();
        Ok(owner.map_or(
            TickSpherePane {
                tick_chart,
                asset_mode: false,
            },
            |owner| owner.tick_sphere_pane(tick_chart),
        ))
    }
    fn wear_tick_spheres(
        &mut self,
        tab: u64,
        id: u64,
        preset: Option<&str>,
    ) -> Result<(bool, Option<TickBubblesSnapshot>), ControlError> {
        let owner = pane(self, WireU64::new(tab), WireU64::new(id))?
            .orderflow
            .as_mut()
            .ok_or_else(|| invalid("The pane has no order-flow view"))?;
        let changed = owner.apply_tick_preset(preset).map_err(invalid)?;
        Ok((
            changed,
            owner
                .cached_config()
                .volume_dots
                .tick_bubbles
                .map(Into::into),
        ))
    }
}
