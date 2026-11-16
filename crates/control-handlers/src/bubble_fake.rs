//! One fake pane for the bubble families' handler tests: the window resolves
//! it, its facts answer the tick rule, and its view records what it wore.
use crate::bubble_preset::{AssetSave, BubbleLook, BubblePane, PaneBubblesPort, TickSpherePane};
use quantick_control::error::ControlError;
use quantick_control_schema::bubble_preset::{ListInput, ListResult};
use quantick_control_schema::orderflow::BubblesStateSnapshot;
use quantick_orderflow::{HeatmapConfig, TickTuning};
use quantick_stores::bubble_asset_store::SaveSwitch;

pub(crate) struct FakePane {
    pub facts: TickSpherePane,
    pub view: Option<FakeLook>,
}

#[derive(Default)]
pub(crate) struct FakeLook {
    /// The whole look last worn, and whether it brought its spheres.
    pub preset: Option<(String, bool)>,
    pub worn: Option<Option<String>>,
    pub tuning: TickTuning,
    /// The bound asset's key; `None` is a view bound to no asset.
    pub asset: Option<String>,
    pub save_changes: bool,
    pub config: HeatmapConfig,
}

impl FakePane {
    /// A pane with these facts and an order-flow view bound to no asset.
    pub fn new(facts: TickSpherePane) -> Self {
        Self {
            facts,
            view: Some(FakeLook::default()),
        }
    }

    pub fn look(&self) -> &FakeLook {
        self.view.as_ref().expect("the fake pane has a view")
    }

    pub fn look_mut(&mut self) -> &mut FakeLook {
        self.view.as_mut().expect("the fake pane has a view")
    }
}

impl PaneBubblesPort for FakePane {
    type Theme = ();
    fn bubble_theme(&self) {}
    fn pane_bubbles(&mut self, _: u64, _: u64) -> Result<&mut dyn BubblePane<()>, ControlError> {
        Ok(self)
    }
}

impl BubblePane<()> for FakePane {
    fn tick_sphere_pane(&self) -> TickSpherePane {
        self.facts
    }
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<()>> {
        Some(self.view.as_mut()?)
    }
    fn asset_save(&mut self) -> Option<&mut dyn AssetSave> {
        Some(self.view.as_mut()?)
    }
}

impl BubbleLook<()> for FakeLook {
    fn wear_preset(&mut self, name: &str, spheres: bool, _: &()) -> Result<bool, String> {
        self.preset = Some((name.to_owned(), spheres));
        Ok(true)
    }
    fn restore_factory_look(&mut self) -> bool {
        self.preset.take().is_some()
    }
    fn presets_page(&self, _: &ListInput) -> Result<ListResult, String> {
        unreachable!("no fake test lists presets")
    }
    fn wear_tick(&mut self, preset: Option<&str>, tuning: TickTuning) -> Result<bool, String> {
        self.worn = Some(preset.map(str::to_owned));
        self.tuning = tuning;
        Ok(true)
    }
    fn look_snapshot(&self) -> BubblesStateSnapshot {
        let zero = rust_decimal::Decimal::ZERO;
        BubblesStateSnapshot::from_config(&self.config, zero, None, &[], None, Vec::new())
    }
    fn look_config(&self) -> &HeatmapConfig {
        &self.config
    }
}

impl AssetSave for FakeLook {
    fn asset_key(&self) -> Option<&str> {
        self.asset.as_deref()
    }
    fn switch_asset_save(&mut self, on: bool) -> Option<SaveSwitch> {
        self.asset.as_ref()?;
        let moved = self.save_changes != on;
        self.save_changes = on;
        Some(match (moved, on) {
            (false, _) => SaveSwitch::Unmoved,
            (true, true) => SaveSwitch::ScreenStored,
            (true, false) => SaveSwitch::Off,
        })
    }
}
