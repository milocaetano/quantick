//! Chart settings decisions shared by the desktop and other adapters.
use quantick_chart::pane_style::{StyleEditor, StyleSurface, StyleTarget};
use quantick_chart::style::{CandlePreset, ChartStyle};
use quantick_control::wire::WireU64;
use quantick_control_schema::chart_settings::{
    ChartSettingsInput, ChartSettingsResult, ChartSettingsTab, ChartSettingsTarget, style_wire,
};
use quantick_stores::candle_presets::PresetError;
use quantick_stores::candle_settings::CandleSettings;
use quantick_stores::chart_style::StyleFile;

#[derive(Default)]
pub struct ChartSettingsSession {
    pub open: bool,
    pub tab: ChartSettingsTab,
    pub editor: StyleEditor,
}
impl ChartSettingsSession {
    pub fn open(
        &mut self,
        tab: ChartSettingsTab,
        target: Option<StyleTarget>,
        style: ChartStyle,
        candles: &mut CandleSettings,
    ) {
        if target.is_some_and(|target| self.editor.pin(target)) {
            candles.end();
        }
        self.open = true;
        self.tab = tab;
        candles.begin(style);
    }
    pub fn close(&mut self, candles: &mut CandleSettings) {
        self.open = false;
        self.editor.last_target = self.editor.target.or(self.editor.last_target);
        self.editor.target = None;
        self.editor.stale = false;
        candles.end();
    }
}

pub fn requested(input: &ChartSettingsInput) -> Option<StyleTarget> {
    match input {
        ChartSettingsInput::Open { target, .. }
        | ChartSettingsInput::ApplyCandlePreset { target, .. }
        | ChartSettingsInput::SaveCandlePreset { target, .. }
        | ChartSettingsInput::ResetCandles { target }
        | ChartSettingsInput::Inspect { target }
        | ChartSettingsInput::UpdateCandles { target, .. } => target.map(|target| StyleTarget {
            tab_id: target.tab_id.0,
            pane_id: target.pane_id.0,
        }),
        ChartSettingsInput::Close | ChartSettingsInput::DeleteCandlePreset { .. } => None,
    }
}
pub struct Applied {
    pub result: ChartSettingsResult,
    pub style: Option<ChartStyle>,
    pub preset: Option<CandlePreset>,
}

/// Only an explicit open may rebind a retired editor sitting to a new source.
pub fn addressed_style(
    input: &ChartSettingsInput,
    current: Option<ChartStyle>,
    available: bool,
) -> Option<ChartStyle> {
    current.filter(|_| available || matches!(input, ChartSettingsInput::Open { .. }))
}

pub fn require_editor(available: bool) -> Result<(), PresetError> {
    available.then_some(()).ok_or(PresetError::Refused(
        "the editor's chart source changed; reopen settings on a live pane",
    ))
}

pub fn apply(
    session: &mut ChartSettingsSession,
    candles: &mut CandleSettings,
    input: ChartSettingsInput,
    target: Option<StyleTarget>,
    current: Option<ChartStyle>,
    surface: StyleSurface<'_>,
) -> Result<Applied, PresetError> {
    let window_before = (session.open, session.tab, session.editor.target);
    let mut look = current;
    let mut list_changed = false;
    let mut preset = None;
    let required = || {
        current.ok_or(PresetError::Refused(
            "the addressed chart pane no longer exists; open settings on a live pane",
        ))
    };
    match input {
        ChartSettingsInput::Open { tab, .. } => session.open(tab, target, required()?, candles),
        ChartSettingsInput::Close => session.close(candles),
        ChartSettingsInput::ApplyCandlePreset { name, .. } => {
            let (style, selected) =
                candles
                    .apply(&name, &required()?)
                    .ok_or(PresetError::Refused(
                        "no built-in or saved candle preset has that name",
                    ))?;
            look = Some(style);
            preset = selected;
        }
        ChartSettingsInput::SaveCandlePreset { name, .. } => {
            let current = required()?;
            let before = candles.saved(&name);
            candles.save(&name, current)?;
            list_changed = before != Some(current.snapshot());
        }
        ChartSettingsInput::DeleteCandlePreset { name } => {
            if CandlePreset::from_name(&name).is_some() {
                return Err(PresetError::Refused("built-in presets cannot be deleted"));
            }
            list_changed = candles.delete(&name)?;
        }
        ChartSettingsInput::ResetCandles { .. } => {
            if !session.open || session.editor.target != target || !candles.in_sitting() {
                return Err(PresetError::Refused(
                    "reset requires the editor to be open on that same chart pane",
                ));
            }
            look = Some(candles.reset(&required()?).unwrap_or(required()?));
        }
        ChartSettingsInput::Inspect { .. } => {
            required()?;
        }
        ChartSettingsInput::UpdateCandles { style, .. } => {
            let stored = required()?;
            let file = style_wire::decode(style).map_err(PresetError::Refused)?;
            if !file.valid() {
                return Err(PresetError::Refused(
                    "chart style values must be finite and within the documented bounds",
                ));
            }
            let edited: ChartStyle = file.into();
            if let Some(refusal) = edited.theme_owned_edit(&stored, &surface.resolve(stored)) {
                return Err(PresetError::Refused(refusal));
            }
            look = Some(edited);
        }
    }
    let look = look.map(|style| surface.resolve(style));
    let changed = look != current;
    let result = ChartSettingsResult {
        open: session.open,
        tab: session.tab,
        target: target.map(|target| ChartSettingsTarget {
            tab_id: WireU64(target.tab_id),
            pane_id: WireU64(target.pane_id),
        }),
        effective_style: look.map(|style| style_wire::encode(&StyleFile::from(&style))),
        candle_preset: look.and_then(|style| candles.matching(&style)),
        built_in_presets: CandlePreset::ALL
            .iter()
            .map(|preset| preset.label().to_owned())
            .collect(),
        custom_presets: candles.custom_names(),
        changed: changed
            || list_changed
            || window_before != (session.open, session.tab, session.editor.target),
    };
    Ok(Applied {
        result,
        style: if changed { look } else { None },
        preset,
    })
}

#[cfg(test)]
mod tests;
