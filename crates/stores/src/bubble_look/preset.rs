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
        let available: Vec<_> = self.available_presets().collect();
        if offset > available.len() {
            return Err("Preset page offset exceeds the available count".to_owned());
        }
        Ok(available
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|preset| preset.name.clone())
            .collect())
    }

    /// Shared whole-look picker policy: chart-only entries require calibration
    /// support; whole looks retain their existing cross-asset application.
    #[must_use]
    pub fn preset_available(&self, preset: &crate::bubble_presets::BubblePreset) -> bool {
        !preset.tick_only || self.tick_calibration_available(preset)
    }

    /// Exact picker entries, also used for control pagination identity and totals.
    pub fn available_presets(&self) -> impl Iterator<Item = &crate::bubble_presets::BubblePreset> {
        self.presets
            .presets
            .iter()
            .filter(|preset| self.preset_available(preset))
    }

    /// Uses the UI picker operation, including same-name reapplication.
    /// An identical configuration and selection have no asset or lane effects.
    pub fn apply_named_preset(
        &mut self,
        name: &str,
        config: &mut HeatmapConfig,
    ) -> Result<bool, String> {
        if name.trim().is_empty() || self.presets.get(name).is_none() {
            return Err("Unknown or empty bubble preset name".to_owned());
        }
        let mut desired = config.clone();
        let preset = self.presets.get(name).expect("validated preset");
        if preset.tick_only {
            self.tick_style(Some(name))?;
        }
        preset.apply_to(&mut desired);
        desired.sanitize();
        if desired == *config && (preset.tick_only || self.look_name == name) {
            return Ok(false);
        }
        self.apply_preset(name, config);
        self.note_edit();
        Ok(true)
    }
}
