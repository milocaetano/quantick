//! One candle editor sitting and its named looks, shared by UI and control.
use crate::candle_presets::{CandlePresetStore, PresetError, Resolved};
use quantick_chart::style::{CandlePreset, ChartStyle, StyleEdit};
use std::path::PathBuf;

pub struct CandleSettings {
    edit: StyleEdit,
    presets: CandlePresetStore,
    path: PathBuf,
}
impl CandleSettings {
    pub fn open(path: PathBuf) -> Self {
        Self {
            edit: StyleEdit::default(),
            presets: CandlePresetStore::load(&path),
            path,
        }
    }
    pub fn begin(&mut self, current: ChartStyle) {
        self.edit.begin(current);
    }
    pub fn end(&mut self) {
        self.edit.end();
    }
    pub fn in_sitting(&self) -> bool {
        self.edit.opened_on().is_some()
    }
    pub fn reset(&self, current: &ChartStyle) -> Option<ChartStyle> {
        self.edit.reset(current)
    }
    pub fn apply(
        &self,
        name: &str,
        current: &ChartStyle,
    ) -> Option<(ChartStyle, Option<CandlePreset>)> {
        let resolved = self.presets.resolve(name)?;
        let preset = match resolved {
            Resolved::BuiltIn(preset) => Some(preset),
            Resolved::Custom(_) => None,
        };
        Some((resolved.apply_to(current), preset))
    }
    pub fn save(&mut self, name: &str, style: ChartStyle) -> Result<String, PresetError> {
        self.presets.save_as(&self.path, name, style)
    }
    pub fn saved(&self, name: &str) -> Option<ChartStyle> {
        self.presets.get(name).copied()
    }
    pub fn delete(&mut self, name: &str) -> Result<bool, PresetError> {
        self.presets.delete(&self.path, name)
    }
    pub fn matching(&self, style: &ChartStyle) -> Option<String> {
        self.presets.matching(style).map(str::to_owned)
    }
    pub fn custom_names(&self) -> Vec<String> {
        self.presets.names().map(str::to_owned).collect()
    }
    pub fn reload_presets(&mut self) {
        self.presets = CandlePresetStore::load(&self.path);
    }
}
