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
    /// ([`AssetBinding::factory_look`]) on `config`, the look a fresh open
    /// shows — its tick spheres and an inherited palette, which follows the
    /// theme, included, and the native tape as the look sets it — and name
    /// it. Only the look: tape only is the pane's view and stays. With no
    /// asset bound the code
    /// defaults are restored ([`BubbleLook::defaults_restored`]). The asset
    /// on screen files the change at the next sync. Returns whether
    /// anything changed: a screen already on that look is left alone, and
    /// nothing is noted for the asset.
    ///
    /// [`AssetBinding::factory_look`]: crate::bubble_asset_store::AssetBinding::factory_look
    pub fn restore_factory(&mut self, config: &mut HeatmapConfig) -> bool {
        let Some(binding) = self.asset.as_ref() else {
            let mut desired = config.clone();
            desired.reset_bubble_visuals();
            desired.sanitize();
            if desired == *config && self.look_name.is_empty() {
                self.status = Some("the bubble defaults are on screen already".to_owned());
                return false;
            }
            *config = desired;
            self.defaults_restored(config);
            return true;
        };
        let look = binding.factory_look(&self.presets);
        let key = binding.key().to_owned();
        let mut desired = config.clone();
        look.apply_to(&mut desired);
        desired.live_lane.tape_only = config.live_lane.tape_only;
        desired.sanitize();
        let named = if look.name.is_empty() {
            "the default look".to_owned()
        } else {
            format!("'{}'", look.name)
        };
        if desired == *config && self.look_name == look.name {
            self.status = Some(format!("{key} is on its factory look {named} already"));
            return false;
        }
        *config = desired;
        self.look_name.clone_from(&look.name);
        self.name_draft.clone_from(&look.name);
        self.status = Some(format!("factory look {named} restored for {key}"));
        self.note_lane_set(config);
        true
    }
}
