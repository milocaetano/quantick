//! A chart pane serves its own bubble calls: the tick rule's facts from its
//! bars and its view's asset, the look and the save switch from its view.
use super::ChartPane;
use quantick_appearance::orderflow::OrderflowAppearance as Theme;
use quantick_control_handlers::bubble_preset::{AssetSave, BubbleLook, BubblePane, TickSpherePane};
impl BubblePane<Theme> for ChartPane {
    fn tick_sphere_pane(&self) -> TickSpherePane {
        let asset = self.orderflow.as_ref().and_then(|owner| owner.asset());
        let asset_mode = asset.is_some_and(|asset| asset.mode().is_some());
        TickSpherePane::new(self.series.state.tick_membership().is_some(), asset_mode)
    }
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<Theme>> {
        Some(self.orderflow.as_mut()?)
    }
    fn asset_save(&mut self) -> Option<&mut dyn AssetSave> {
        Some(self.orderflow.as_mut()?)
    }
}
