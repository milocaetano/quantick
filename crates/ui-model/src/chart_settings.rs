//! Chart settings presentation transitions. Stores and sessions retain domain authority;
//! the host supplies loaded stores and executes persistence requests.
use crate::Model as _;
use quantick_chart::{
    pane_style::StyleTarget,
    style::{CandlePreset, ChartStyle},
};
use quantick_control_handlers::chart_settings::ChartSettingsSession;
pub use quantick_control_schema::chart_settings::ChartSettingsTab;
use quantick_stores::{
    candle_settings::CandleSettings, footprint_config::FootprintConfig,
    footprint_presets::PresetStore,
};

pub const STYLE_LOG_DEBOUNCE_MS: u64 = 350;

#[derive(Default)]
pub struct State {
    pub session: ChartSettingsSession,
    pub candles: Option<CandleSettings>,
    pub footprint_presets: PresetStore,
    pub loaded: bool,
    pub target_label: String,
    pub candle_name: String,
    pub candle_refusal: Option<String>,
    pub footprint_name: String,
    pub footprint_full: bool,
    pub candle_rows: Vec<String>,
    pub footprint_rows: Vec<String>,
    pub matching_candle: Option<String>,
    pub builtin_candle: Option<CandlePreset>,
    pub matching_footprint: Vec<bool>,
    pub reset: Option<ChartStyle>,
    pub style: ChartStyle,
    pub footprint: FootprintConfig,
    pub footprint_target: String,
    pub footprint_customized: bool,
    log_pending: bool,
    last_change: Option<u64>,
    pending_preset: Option<CandlePreset>,
}
impl std::ops::Deref for State {
    type Target = ChartSettingsSession;
    fn deref(&self) -> &Self::Target {
        &self.session
    }
}
impl std::ops::DerefMut for State {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.session
    }
}
impl State {
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn is_open_on(&self, tab: ChartSettingsTab) -> bool {
        self.open && self.tab == tab
    }
    pub fn can_save_candle(&self) -> bool {
        self.loaded && !self.candle_name.trim().is_empty()
    }
    pub fn can_save_footprint(&self) -> bool {
        self.loaded && !self.footprint_name.trim().is_empty()
    }
    fn project(&mut self) {
        self.builtin_candle = CandlePreset::detect(&self.style.candles);
        if let Some(candles) = &self.candles {
            self.candle_rows = candles.custom_names();
            self.matching_candle = candles.matching(&self.style);
            self.reset = candles.reset(&self.style);
        }
        self.footprint_rows = self.footprint_presets.names().map(str::to_owned).collect();
        self.matching_footprint = self
            .footprint_rows
            .iter()
            .map(|name| self.footprint_presets.get(name) == Some(&self.footprint))
            .collect();
    }
    pub fn close(&mut self) {
        Model::update(self, Intent::Close);
    }
}

pub enum Intent {
    Loaded {
        candles: CandleSettings,
        footprint: PresetStore,
    },
    Open {
        tab: ChartSettingsTab,
        target: Option<StyleTarget>,
        style: ChartStyle,
    },
    Close,
    SelectTab(ChartSettingsTab),
    Refresh {
        style: ChartStyle,
        footprint: FootprintConfig,
        target: String,
        customized: bool,
    },
    StoresChanged,
    SessionChanged(Option<ChartStyle>),
    CandleName(String),
    FootprintName(String),
    SelectCandle(String, u64),
    ResetCandles(u64),
    EditStyle(ChartStyle, Option<CandlePreset>, u64),
    SaveCandle,
    DeleteCandle(String),
    CandleSaved {
        name: String,
        result: Result<(), String>,
    },
    CandleDeleted(Result<(), String>),
    SelectFootprint(String),
    EditFootprint(FootprintConfig),
    ResetFootprint,
    SaveFootprint,
    DeleteFootprint(String),
    Tick(u64),
}
#[derive(Debug, PartialEq)]
pub enum Persistence {
    SaveCandle { name: String, style: ChartStyle },
    DeleteCandle(String),
    SaveFootprint,
}
#[derive(Debug, PartialEq)]
pub enum FootprintChange {
    Applied(Box<FootprintConfig>),
    ResetToDefault,
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub style: Option<ChartStyle>,
    pub footprint: Option<FootprintChange>,
    pub log: Option<Option<CandlePreset>>,
    pub persistence: Vec<Persistence>,
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        let mut effects = Effects::default();
        match intent {
            Intent::Loaded { candles, footprint } => {
                state.candles = Some(candles);
                state.footprint_presets = footprint;
                state.loaded = true;
                state.project();
            }
            Intent::Open { tab, target, style } => {
                if let Some(candles) = &mut state.candles {
                    state.session.open(tab, target, style, candles);
                    state.style = style;
                    state.project();
                }
            }
            Intent::Close => {
                if let Some(candles) = &mut state.candles {
                    state.session.close(candles);
                }
                state.candle_refusal = None;
                state.project();
            }
            Intent::SelectTab(tab) => state.session.tab = tab,
            Intent::Refresh {
                style,
                footprint,
                target,
                customized,
            } => {
                if state.style != style
                    || state.footprint != footprint
                    || state.footprint_target != target
                    || state.footprint_customized != customized
                {
                    state.style = style;
                    state.footprint = footprint;
                    state.footprint_target = target;
                    state.footprint_customized = customized;
                    state.project();
                }
            }
            Intent::StoresChanged => state.project(),
            Intent::SessionChanged(style) => {
                if let Some(style) = style {
                    state.style = style;
                }
                if !state.open {
                    state.candle_refusal = None;
                }
                state.project();
            }
            Intent::CandleName(name) => {
                state.candle_name = name;
                state.candle_refusal = None;
            }
            Intent::FootprintName(name) => {
                state.footprint_name = name;
                state.footprint_full = false;
            }
            Intent::SelectCandle(name, now) => {
                if let Some((style, preset)) = state
                    .candles
                    .as_ref()
                    .and_then(|candles| candles.apply(&name, &state.style))
                {
                    return Self::update(state, Intent::EditStyle(style, preset, now));
                }
            }
            Intent::ResetCandles(now) => {
                if let Some(style) = state.reset {
                    return Self::update(state, Intent::EditStyle(style, None, now));
                }
            }
            Intent::EditStyle(style, preset, now) => {
                if style != state.style {
                    state.style = style;
                    state.candle_refusal = None;
                    state.log_pending = true;
                    state.last_change = Some(now);
                    state.pending_preset = preset;
                    state.project();
                    effects.style = Some(style);
                }
            }
            Intent::SaveCandle => {
                if state.can_save_candle() {
                    effects.persistence.push(Persistence::SaveCandle {
                        name: state.candle_name.clone(),
                        style: state.style,
                    });
                }
            }
            Intent::DeleteCandle(name) => effects.persistence.push(Persistence::DeleteCandle(name)),
            Intent::CandleSaved { name, result } => {
                state.candle_refusal = result.err();
                if state.candle_refusal.is_none() && state.candle_name == name {
                    state.candle_name.clear();
                }
                state.project();
            }
            Intent::CandleDeleted(result) => {
                state.candle_refusal = result.err();
                state.project();
            }
            Intent::SelectFootprint(name) => {
                if let Some(config) = state.footprint_presets.get(&name).cloned() {
                    return Self::update(state, Intent::EditFootprint(config));
                }
            }
            Intent::EditFootprint(config) => {
                if state.footprint != config {
                    state.footprint = config.clone();
                    state.project();
                    effects.footprint = Some(FootprintChange::Applied(Box::new(config)));
                }
            }
            Intent::ResetFootprint => effects.footprint = Some(FootprintChange::ResetToDefault),
            Intent::SaveFootprint => {
                if state.loaded
                    && state
                        .footprint_presets
                        .insert(&state.footprint_name, &state.footprint)
                {
                    state.footprint_name.clear();
                    state.footprint_full = false;
                    state.project();
                    effects.persistence.push(Persistence::SaveFootprint);
                } else {
                    state.footprint_full = state.footprint_presets.is_full();
                }
            }
            Intent::DeleteFootprint(name) => {
                if state.footprint_presets.remove(&name) {
                    state.project();
                    effects.persistence.push(Persistence::SaveFootprint);
                }
            }
            Intent::Tick(now) => {
                state.footprint_full = false;
                if state.log_pending
                    && (!state.open
                        || state.last_change.is_some_and(|changed| {
                            now.saturating_sub(changed) >= STYLE_LOG_DEBOUNCE_MS
                        }))
                {
                    state.log_pending = false;
                    effects.log = Some(state.pending_preset.take());
                }
            }
        }
        effects
    }
}

#[cfg(test)]
#[path = "chart_settings/tests.rs"]
mod tests;
