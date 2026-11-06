//! Validated whole-look application and bounded picker discovery.
use super::BubbleLook;
use quantick_orderflow::HeatmapConfig;

pub const PRESET_PAGE_MAX: usize = 64;

impl BubbleLook {
    /// Picker order is preserved; a reload may change it between pages.
    pub fn preset_page(&self, offset: usize, limit: usize) -> Result<Vec<String>, String> {
        if !(1..=PRESET_PAGE_MAX).contains(&limit) {
            return Err(format!("Preset page limit must be 1..={PRESET_PAGE_MAX}"));
        }
        if offset > self.presets.presets.len() {
            return Err("Preset page offset exceeds the available count".to_owned());
        }
        Ok(self
            .presets
            .presets
            .iter()
            .skip(offset)
            .take(limit)
            .map(|preset| preset.name.clone())
            .collect())
    }

    /// Uses the UI picker operation, including same-name reapplication, and
    /// the same `spheres` verdict ([`BubbleLook::apply_preset`]). An
    /// identical configuration and selection have no asset or lane effects.
    pub fn apply_named_preset(
        &mut self,
        name: &str,
        config: &mut HeatmapConfig,
        spheres: bool,
    ) -> Result<bool, String> {
        self.apply_named_preset_with(
            name,
            config,
            spheres,
            &quantick_appearance::ResolvedTheme::bundled_dark().orderflow,
        )
    }
    pub fn apply_named_preset_with(
        &mut self,
        name: &str,
        config: &mut HeatmapConfig,
        spheres: bool,
        appearance: &quantick_appearance::orderflow::OrderflowAppearance,
    ) -> Result<bool, String> {
        let preset = match self.presets.get(name) {
            Some(preset) if !name.trim().is_empty() => preset.clone(),
            _ => return Err("Unknown or empty bubble preset name".to_owned()),
        };
        // Dressed as the pick dresses the screen, so a look worn without its
        // spheres reads back as itself.
        let mut desired = config.clone();
        super::dress(&preset, &mut desired, spheres, appearance);
        desired.sanitize();
        if desired == *config && self.look_name == name {
            return Ok(false);
        }
        self.apply_preset_with(name, config, spheres, appearance);
        self.note_edit();
        Ok(true)
    }

    /// The panel's reset: put the bound asset's factory look
    /// ([`AssetBinding::factory_preset`]) on `config` as a fresh open wears
    /// it — an inherited palette stays inherited and follows the theme, so
    /// an asset back on its unedited declared look keeps no stored entry —
    /// with its tick spheres only where `spheres`, the verdict a whole-preset
    /// pick gets; where not, the spheres worn stay. Then name it. With no
    /// asset bound, or none the shipped presets declare a look for, the
    /// code defaults are restored instead ([`BubbleLook::defaults_restored`]).
    /// Either way the asset on screen files what it shows at the next sync.
    /// Returns the factory preset's name, `None` for the code defaults.
    ///
    /// [`AssetBinding::factory_preset`]: crate::bubble_asset_store::AssetBinding::factory_preset
    pub fn restore_factory(&mut self, config: &mut HeatmapConfig, spheres: bool) -> Option<String> {
        let factory = self.asset.as_ref().and_then(|binding| {
            let preset = binding.factory_preset()?;
            Some((preset, binding.key().to_owned()))
        });
        let Some((preset, key)) = factory else {
            config.reset_bubble_visuals();
            self.defaults_restored(config);
            return None;
        };
        let worn = config.volume_dots.tick_bubbles;
        preset.apply_to(config);
        if !spheres {
            config.volume_dots.tick_bubbles = worn;
        }
        self.look_name.clone_from(&preset.name);
        self.name_draft.clone_from(&preset.name);
        self.status = Some(format!("factory look '{}' restored for {key}", preset.name));
        self.note_lane_set(config);
        Some(preset.name)
    }
}
