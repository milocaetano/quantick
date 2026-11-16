//! One fake pane for the bubble families' handler tests: the window resolves
//! it, its facts answer the tick rule, and its view records what it wore.
use crate::bubble_preset::{BubbleLook, BubblePane, PaneBubblesPort, ResolvedPane, TickSpherePane};
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
    fn pane_bubbles(
        &mut self,
        _tab_id: u64,
        _pane_id: u64,
    ) -> Result<ResolvedPane<'_, ()>, ControlError> {
        Ok(((), self))
    }
}

impl BubblePane<()> for FakePane {
    fn tick_sphere_pane(&self) -> TickSpherePane {
        self.facts
    }
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<()>> {
        self.view
            .as_mut()
            .map(|view| view as &mut dyn BubbleLook<()>)
    }
}

impl BubbleLook<()> for FakeLook {
    fn apply_named_preset(&mut self, _: &str, _: bool, _: &()) -> Result<bool, String> {
        unreachable!("no fake test wears a whole look")
    }
    fn reset_bubble_look(&mut self) -> bool {
        unreachable!("no fake test resets the look")
    }
    fn bubble_presets_page(&self, _: &ListInput) -> Result<ListResult, String> {
        unreachable!("no fake test lists presets")
    }
    fn wear_tick_spheres(
        &mut self,
        preset: Option<&str>,
        tuning: TickTuning,
    ) -> Result<bool, String> {
        self.worn = Some(preset.map(str::to_owned));
        self.tuning = tuning;
        Ok(true)
    }
    fn bubbles_snapshot(&self) -> BubblesStateSnapshot {
        unreachable!("no fake test reads the bubbles snapshot")
    }
    fn cached_config(&self) -> &HeatmapConfig {
        &self.config
    }
    fn asset_key(&self) -> Option<&str> {
        self.asset.as_deref()
    }
    fn set_save_asset_changes(&mut self, on: bool) -> Option<SaveSwitch> {
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
