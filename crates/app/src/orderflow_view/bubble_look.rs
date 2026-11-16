//! The order-flow view serves its pane's bubble calls from its own look.
use super::OrderflowView;
use quantick_appearance::orderflow::OrderflowAppearance as Theme;
use quantick_control_handlers::bubble_preset::BubbleLook;
use quantick_control_schema::bubble_preset::{ListInput, ListResult};
use quantick_control_schema::orderflow::BubblesStateSnapshot;
use quantick_orderflow::{HeatmapConfig, TickTuning};
use quantick_stores::bubble_asset_store::SaveSwitch;
impl BubbleLook<Theme> for OrderflowView {
    fn apply_named_preset(
        &mut self,
        name: &str,
        spheres: bool,
        theme: &Theme,
    ) -> Result<bool, String> {
        self.apply_named_preset_with(name, spheres, theme)
    }
    fn reset_bubble_look(&mut self) -> bool {
        OrderflowView::reset_bubble_look(self)
    }
    fn bubble_presets_page(&self, input: &ListInput) -> Result<ListResult, String> {
        quantick_control_schema::bubble_preset::page(input, &self.look)
    }
    fn wear_tick_spheres(
        &mut self,
        preset: Option<&str>,
        tuning: TickTuning,
    ) -> Result<bool, String> {
        self.apply_tick_preset(preset, tuning)
    }
    fn bubbles_snapshot(&self) -> BubblesStateSnapshot {
        OrderflowView::bubbles_snapshot(self)
    }
    fn cached_config(&self) -> &HeatmapConfig {
        OrderflowView::cached_config(self)
    }
    fn asset_key(&self) -> Option<&str> {
        self.asset().map(|asset| asset.key())
    }
    fn set_save_asset_changes(&mut self, on: bool) -> Option<SaveSwitch> {
        OrderflowView::set_save_asset_changes(self, on)
    }
}
