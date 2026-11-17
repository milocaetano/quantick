//! The order-flow view serves its pane's bubble calls from its own look.
use super::OrderflowView;
use quantick_appearance::orderflow::OrderflowAppearance as Theme;
use quantick_control_handlers::bubble_pane::{AssetSave, BubbleLook};
use quantick_control_schema::bubble_preset::{ListInput, ListResult, page};
use quantick_control_schema::orderflow::BubblesStateSnapshot;
use quantick_orderflow::{HeatmapConfig, TickTuning};
use quantick_stores::bubble_asset_store::SaveSwitch;
impl BubbleLook<Theme> for OrderflowView {
    fn wear_preset(&mut self, name: &str, spheres: bool, theme: &Theme) -> Result<bool, String> {
        self.apply_named_preset_with(name, spheres, theme)
    }
    fn restore_factory_look(&mut self) -> bool {
        self.reset_bubble_look()
    }
    fn presets_page(&self, input: &ListInput) -> Result<ListResult, String> {
        page(input, &self.look)
    }
    fn wear_tick(&mut self, preset: Option<&str>, tuning: TickTuning) -> Result<bool, String> {
        self.apply_tick_preset(preset, tuning)
    }
    fn look_snapshot(&self) -> BubblesStateSnapshot {
        self.bubbles_snapshot()
    }
    fn look_config(&self) -> &HeatmapConfig {
        self.cached_config()
    }
}
impl AssetSave for OrderflowView {
    fn asset_key(&self) -> Option<&str> {
        self.asset().map(|asset| asset.key())
    }
    fn switch_asset_save(&mut self, on: bool) -> Option<SaveSwitch> {
        self.set_save_asset_changes(on)
    }
}
