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

    /// Uses the UI picker operation, including same-name reapplication.
    /// An identical configuration and selection have no asset or lane effects.
    pub fn apply_named_preset(
        &mut self,
        name: &str,
        config: &mut HeatmapConfig,
    ) -> Result<bool, String> {
        let preset = match self.presets.get(name) {
            Some(preset) if !name.trim().is_empty() => preset.clone(),
            _ => return Err("Unknown or empty bubble preset name".to_owned()),
        };
        // Dressed as the pick dresses the screen, so a look worn without its
        // spheres reads back as itself.
        let mut desired = config.clone();
        self.dress(&preset, &mut desired);
        desired.sanitize();
        if desired == *config && self.look_name == name {
            return Ok(false);
        }
        self.apply_preset(name, config);
        self.note_edit();
        Ok(true)
    }
}
