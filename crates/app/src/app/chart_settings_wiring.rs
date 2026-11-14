//! Adapters between stable pane appearance owners and the existing settings surface.
use super::chart_layers_wiring::LayerWiring;
use crate::candle_presets::PresetError;
use crate::pane::ChartPane;
use crate::style::ChartStyle;
use crate::surfaces::ChartSettingsTab;
use quantick_chart::pane_style::StyleTarget;
use quantick_control_handlers::chart_settings as settings;
use quantick_control_schema::chart_settings::{ChartSettingsInput, ChartSettingsResult};
use quantick_ui_model::Model as _;

impl LayerWiring<'_> {
    fn focused_target(&self) -> StyleTarget {
        let tab = &self.tabs[self.tabs.active_index()];
        StyleTarget {
            tab_id: self.tabs.active_id(),
            pane_id: tab.focused_pane().id,
        }
    }
    fn target_pane(&self, target: StyleTarget) -> Result<&ChartPane, PresetError> {
        self.tabs
            .by_id(target.tab_id)
            .and_then(|tab| {
                tab.panes()
                    .find(|(pane, _)| pane.id == target.pane_id)
                    .map(|(pane, _)| pane)
            })
            .ok_or(PresetError::Refused(
                "the addressed chart pane no longer exists; open settings on a live pane",
            ))
    }
    fn style_at(&self, target: StyleTarget) -> Result<ChartStyle, PresetError> {
        let pane = self.target_pane(target)?;
        let theme = self.appearance.resolved();
        Ok(pane
            .presentation
            .candles
            .resolve_surface(self.style, theme, pane.surface()))
    }
    fn set_style_at(&mut self, target: StyleTarget, style: ChartStyle) -> Result<(), PresetError> {
        self.target_pane(target)?;
        let tab = self.tabs.by_id_mut(target.tab_id).expect("validated tab");
        let side = tab
            .panes()
            .find(|(pane, _)| pane.id == target.pane_id)
            .map(|(_, side)| side)
            .expect("validated pane");
        tab.pane_mut(side).presentation.candles.set(style);
        Ok(())
    }
    /// Apply a UI result only to the editor's still-live pinned target.
    pub(crate) fn apply_editor_style(&mut self, style: ChartStyle) -> Result<(), PresetError> {
        let target = self
            .chart_settings
            .editor
            .target
            .ok_or(PresetError::Refused("no chart editor target"))?;
        let context = self
            .target_pane(target)
            .ok()
            .map(|pane| pane.presentation.candles.context());
        settings::require_editor(self.chart_settings.editor.observe(target, context))?;
        self.set_style_at(target, style)?;
        *self.style_revision = self.style_revision.saturating_add(1);
        Ok(())
    }
    pub(crate) fn chart_settings(
        &mut self,
        input: ChartSettingsInput,
    ) -> Result<ChartSettingsResult, PresetError> {
        initialize(self.chart_settings);
        let target = self
            .chart_settings
            .editor
            .resolve(settings::requested(&input), self.focused_target());
        let context = self
            .target_pane(target)
            .ok()
            .map(|pane| pane.presentation.candles.context());
        let available = self.chart_settings.editor.observe(target, context);
        let current = settings::addressed_style(&input, self.style_at(target).ok(), available);
        let surface = quantick_chart::pane_style::StyleSurface::new(
            self.appearance.resolved(),
            self.target_pane(target)
                .map_or(quantick_appearance::ChartSurface::Paper, |pane| {
                    pane.surface()
                }),
        );
        let applied = settings::apply(
            &mut self.chart_settings.session,
            self.chart_settings
                .candles
                .as_mut()
                .expect("initialized settings"),
            input,
            current.map(|_| target),
            current,
            surface,
        )?;
        self.chart_settings.editor.observe(target, context);
        quantick_ui_model::chart_settings::Model::update(
            self.chart_settings,
            quantick_ui_model::chart_settings::Intent::SessionChanged(applied.style.or(current)),
        );
        if let Some(style) = applied.style {
            self.set_style_at(target, style)?;
            *self.style_revision = self.style_revision.saturating_add(1);
            super::health::emit_style_changed(&style, *self.style_revision, applied.preset);
        }
        Ok(applied.result)
    }
    pub(crate) fn log_editor_style(&self, preset: Option<crate::style::CandlePreset>) {
        if let Some(target) = self
            .chart_settings
            .editor
            .target
            .or(self.chart_settings.editor.last_target)
            && let Ok(style) = self.style_at(target)
        {
            super::health::emit_style_changed(&style, *self.style_revision, preset);
        }
    }
    pub(crate) fn open_target_settings(
        &mut self,
        tab: ChartSettingsTab,
        requested: Option<StyleTarget>,
    ) {
        initialize(self.chart_settings);
        let target = requested.unwrap_or_else(|| self.focused_target());
        if let Ok(style) = self.style_at(target) {
            let context = self
                .target_pane(target)
                .ok()
                .map(|pane| pane.presentation.candles.context());
            self.chart_settings.editor.observe(target, context);
            quantick_ui_model::chart_settings::Model::update(
                self.chart_settings,
                quantick_ui_model::chart_settings::Intent::Open {
                    tab,
                    target: Some(target),
                    style,
                },
            );
            self.chart_settings.editor.observe(target, context);
        }
    }
    pub(crate) fn open_settings(&mut self, tab: ChartSettingsTab) {
        self.open_target_settings(tab, None);
    }
}

/// Load once at host entry, including control calls before the first paint.
pub(crate) fn initialize(state: &mut quantick_ui_model::chart_settings::State) {
    if !state.loaded {
        reload(state);
    }
}
pub(crate) fn reload(state: &mut quantick_ui_model::chart_settings::State) {
    use quantick_ui_model::chart_settings::{Intent, Model};
    if let Some(candles) = &mut state.candles {
        candles.reload_presets();
        state.footprint_presets = quantick_stores::footprint_presets::PresetStore::load(
            &crate::footprint_presets::default_path(),
        );
        Model::update(state, Intent::StoresChanged);
    } else {
        Model::update(
            state,
            Intent::Loaded {
                candles: quantick_stores::candle_settings::CandleSettings::open(
                    crate::candle_presets::default_path(),
                ),
                footprint: quantick_stores::footprint_presets::PresetStore::load(
                    &crate::footprint_presets::default_path(),
                ),
            },
        );
    }
}
pub(crate) fn persist(
    state: &mut quantick_ui_model::chart_settings::State,
    requests: Vec<quantick_ui_model::chart_settings::Persistence>,
) {
    use quantick_ui_model::chart_settings::{Intent, Model, Persistence};
    for request in requests {
        match request {
            Persistence::SaveCandle { name, style } => {
                let result = state
                    .candles
                    .as_mut()
                    .ok_or_else(|| "Settings presets are not loaded".to_owned())
                    .and_then(|candles| {
                        candles
                            .save(&name, style)
                            .map(|_| ())
                            .map_err(|error| error.message())
                    });
                Model::update(state, Intent::CandleSaved { name, result });
            }
            Persistence::DeleteCandle(name) => {
                let result = state
                    .candles
                    .as_mut()
                    .ok_or_else(|| "Settings presets are not loaded".to_owned())
                    .and_then(|candles| {
                        candles
                            .delete(&name)
                            .map(|_| ())
                            .map_err(|error| error.message())
                    });
                Model::update(state, Intent::CandleDeleted(result));
            }
            Persistence::SaveFootprint => state
                .footprint_presets
                .save(&crate::footprint_presets::default_path()),
        }
    }
}

#[cfg(test)]
#[path = "../surfaces/chart_settings/tests/mod.rs"]
mod view_tests;
