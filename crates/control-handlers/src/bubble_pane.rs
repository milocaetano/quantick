//! The bubble families' one port: the window resolves the addressed pane,
//! the pane answers the tick rule's facts, and its order-flow view serves
//! the look and the asset save switch.
use quantick_control::{error::ControlError, wire::WireU64};
use quantick_control_schema::bubble_preset::{ListInput, ListResult};
use quantick_control_schema::orderflow::BubblesStateSnapshot;
use quantick_orderflow::{HeatmapConfig, TickTuning};
use quantick_stores::bubble_asset_store::SaveSwitch;

/// The window's bubble job: resolve the addressed pane, and lend the theme a
/// whole look is dressed in. Everything else is the pane's.
pub trait PaneBubblesPort {
    /// The window's order-flow appearance.
    type Theme;
    fn bubble_theme(&self) -> Self::Theme;
    /// The pane `tab_id` and `pane_id` name.
    fn pane_bubbles(
        &mut self,
        tab_id: WireU64,
        pane_id: WireU64,
    ) -> Result<&mut dyn BubblePane<Self::Theme>, ControlError>;
}

/// A chart pane, as its bubble calls read it.
pub trait BubblePane<T> {
    /// The pane's facts for [`TickSpherePane::admit`].
    fn tick_sphere_pane(&self) -> TickSpherePane;
    /// The pane's order-flow view, when it has one.
    fn bubble_look(&mut self) -> Option<&mut dyn BubbleLook<T>>;
    /// The same view's asset save switch, when it has one.
    fn asset_save(&mut self) -> Option<&mut dyn AssetSave>;
}

/// An order-flow view's bubble look: what the look calls change and read.
/// Its names differ from the view's own methods, so an implementation that
/// forwards to one cannot fall back on itself.
pub trait BubbleLook<T> {
    /// Wear the whole preset `name`; its tick spheres only when `spheres`,
    /// the pane's [`TickSpherePane::may_choose`].
    fn wear_preset(&mut self, name: &str, spheres: bool, theme: &T) -> Result<bool, String>;
    /// Restore the factory look, as the panel's reset button does: the look
    /// a fresh open shows, its tick spheres included.
    fn restore_factory_look(&mut self) -> bool;
    fn presets_page(&self, input: &ListInput) -> Result<ListResult, String>;
    /// Wear `preset`'s tick spheres with `tuning` applied, or none.
    fn wear_tick(&mut self, preset: Option<&str>, tuning: TickTuning) -> Result<bool, String>;
    fn look_snapshot(&self) -> BubblesStateSnapshot;
    fn look_config(&self) -> &HeatmapConfig;
}

/// An order-flow view's asset binding, as the save switch reads it.
pub trait AssetSave {
    /// The key of the asset the view's look belongs to, once a tab bound one.
    fn asset_key(&self) -> Option<&str>;
    /// Switch "Save changes for this asset"; `None` while bound to no asset.
    fn switch_asset_save(&mut self, on: bool) -> Option<SaveSwitch>;
}

/// What the tick sphere rule reads about a pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickSpherePane {
    /// The pane draws tick bars.
    pub tick_chart: bool,
    /// The config switches the mode on for the pane's asset
    /// (`symbol_asset_modes` in feeds.toml).
    pub asset_mode: bool,
}

impl TickSpherePane {
    #[must_use]
    pub const fn new(tick_chart: bool, asset_mode: bool) -> Self {
        Self {
            tick_chart,
            asset_mode,
        }
    }

    /// The one rule for tick spheres, which the settings picker and
    /// `orderflow.bubbles.tick.set` both ask: they may be chosen on a tick
    /// chart whose asset the config switches the mode on for.
    #[must_use]
    pub fn may_choose(self) -> bool {
        self.tick_chart && self.asset_mode
    }

    /// What the settings picker offers, by the same rule: the asset's
    /// `presets` where spheres may be chosen; only the switch-off where they
    /// may not but the pane is `wearing` some; no picker otherwise.
    #[must_use]
    pub fn offered(self, presets: Vec<String>, wearing: bool) -> Option<Vec<String>> {
        if self.may_choose() {
            Some(presets)
        } else {
            wearing.then(Vec::new)
        }
    }

    /// Whether the spheres a pane wears may be tuned (peak spheres, opacity),
    /// from the settings panel or `orderflow.bubbles.tick.set`. The call tunes
    /// a named preset, so the panel may tune exactly when the pane wears one
    /// the call would admit: [`Self::admit`] of `worn`, and a pane wearing
    /// custom spheres or none has nothing the call could name.
    ///
    /// # Errors
    ///
    /// Names why tuning is refused, in the words the call refuses with.
    pub fn tune(self, worn: Option<&str>) -> Result<(), SphereRefusal> {
        let worn = worn.ok_or(SphereRefusal::NoNamedPreset)?;
        self.admit(Some(worn))
    }

    /// Whether `preset` may be worn. Dropping spheres always may, whatever
    /// put them on (review #29); a named preset needs [`Self::may_choose`].
    ///
    /// # Errors
    ///
    /// Names which half of the rule a named preset fails.
    pub fn admit(self, preset: Option<&str>) -> Result<(), SphereRefusal> {
        match preset {
            None => Ok(()),
            Some(_) if !self.tick_chart => Err(SphereRefusal::NotTickChart),
            Some(_) if !self.asset_mode => Err(SphereRefusal::AssetModeOff),
            Some(_) => Ok(()),
        }
    }
}

/// Which half of the tick sphere rule a named preset fails, so a caller
/// branches on the reason instead of parsing a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SphereRefusal {
    /// The pane does not draw tick bars.
    NotTickChart,
    /// The feed config switches no asset mode on for the pane's asset.
    AssetModeOff,
    /// The pane wears custom spheres or none: there is no preset to tune.
    NoNamedPreset,
}

impl SphereRefusal {
    /// Why, in the words `orderflow.bubbles.tick.set` refuses with.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        match self {
            Self::NotTickChart => "Tick sphere presets require a tick chart",
            Self::AssetModeOff => {
                "Tick sphere presets require an asset whose config asset mode is on"
            }
            Self::NoNamedPreset => "peak_spheres and opacity tune a named preset",
        }
    }
}

impl From<SphereRefusal> for ControlError {
    fn from(refusal: SphereRefusal) -> Self {
        use quantick_control_schema::{bubble_scale, tick_bubbles};
        match refusal {
            SphereRefusal::NotTickChart => crate::bubble_scale::refused_precondition(
                bubble_scale::TICK_CHART_PRECONDITION,
                refusal.reason(),
                "switch the pane to a tick chart, or send a null preset",
            ),
            SphereRefusal::AssetModeOff => crate::bubble_scale::refused_precondition(
                tick_bubbles::ASSET_MODE_PRECONDITION,
                refusal.reason(),
                "switch an asset mode on for this asset in the feed config (symbol_asset_modes)",
            ),
            SphereRefusal::NoNamedPreset => invalid(refusal.reason()),
        }
    }
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "bubble_pane_tests.rs"]
mod tests;
