//! A chart pane serves its own bubble calls: the tick rule's facts from its
//! bars and its view's asset, the look from its order-flow view.
use super::ChartPane;
use quantick_appearance::orderflow::OrderflowAppearance as Theme;
use quantick_control_handlers::bubble_preset::{BubbleLook, BubblePane, TickSpherePane};
impl BubblePane<Theme> for ChartPane {
    fn tick_sphere_pane(&self) -> TickSpherePane {
        let asset = self.orderflow.as_ref().and_then(|owner| owner.asset());
        let asset_mode = asset.is_some_and(|asset| asset.mode().is_some());
        TickSpherePane::new(self.series.state.tick_membership().is_some(), asset_mode)
    }
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<Theme>> {
        let view = self.orderflow.as_mut()?;
        Some(view)
    }
}
