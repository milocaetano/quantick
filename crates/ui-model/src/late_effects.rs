//! The asks a frame settles after every surface has drawn.
use crate::HostEffect;
use crate::alerts::Request as AlertRequest;
use crate::bar_switch::BarSwitchRequest;
use crate::chart_settings::{FootprintChange, Persistence};
use crate::chrome_toolbar::ToolbarAction;
use crate::indicator_picker::{ActiveAction, Effect as PickerEffect};
use crate::script_editor::Effect as EditorEffect;
use crate::strategy_popup::ArmRequest;
use quantick_alerts::sound::Cue;
use quantick_chart::style::{CandlePreset, ChartStyle};
use quantick_stores::indicator_picker::Choice;
use quantick_stores::script_library::ScriptEditorAction;
use quantick_workspace::market_list::PaletteIntent;

/// Every ask the frame's surfaces made, however many surfaces asked.
///
/// A value two surfaces cannot both be right about goes to the first asker in
/// draw order. Undo is a flag: each Undo pops a drawing, so several asks in
/// one frame still undo once. Action lists and persistence keep draw order.
#[derive(Debug, Default, PartialEq)]
pub struct LateEffects {
    pub market: Option<PaletteIntent>,
    pub undo_drawing: bool,
    pub bar_switch: Option<BarSwitchRequest>,
    pub alerts: Option<Box<AlertRequest>>,
    pub indicator_actions: Vec<ToolbarAction>,
    pub script_editor_actions: Vec<ScriptEditorAction>,
    /// The chart settings window edited the chart style.
    pub style: Option<ChartStyle>,
    /// An appearance change settled and is owed a log line.
    pub log_style_change: Option<StyleLogRequest>,
    pub footprint: Option<FootprintChange>,
    pub persistence: Vec<Persistence>,
    pub arm_strategy: Option<ArmRequest>,
    /// A sound was auditioned, by the alert editor or the arming dialog.
    pub test_alert: Option<Cue>,
}

/// Ask the host to record an appearance change.
///
/// Carries only what the surface knows that the host does not: whether the
/// trader reached the new appearance by clicking a named preset, which the
/// log prefers over guessing the name back from the values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StyleLogRequest {
    /// The preset the trader clicked this frame, if they clicked one.
    pub applied_preset: Option<CandlePreset>,
}

impl LateEffects {
    /// Fold the effects in draw order; effects of other surfaces are ignored.
    pub fn fold(effects: impl IntoIterator<Item = HostEffect>) -> Self {
        let mut late = Self::default();
        for effect in effects {
            late.take(effect);
        }
        late
    }

    fn take(&mut self, effect: HostEffect) {
        match effect {
            HostEffect::MarketPalette(asked) => self.market = self.market.take().or(asked),
            HostEffect::Toast(toast) => self.undo_drawing |= toast.undo_drawing,
            HostEffect::BarSwitch(asked) => self.bar_switch = self.bar_switch.take().or(asked),
            HostEffect::Alerts(alerts) => {
                self.alerts = self.alerts.take().or(alerts.request);
                self.test_alert = self.test_alert.take().or(alerts.preview);
            }
            HostEffect::StrategyPopup(popup) => {
                self.arm_strategy = self.arm_strategy.take().or(popup.arm);
                self.test_alert = self.test_alert.take().or(popup.test_alert);
            }
            HostEffect::ChartSettings(settings) => {
                self.style = self.style.take().or(settings.style);
                let log = (settings.log).map(|applied_preset| StyleLogRequest { applied_preset });
                self.log_style_change = self.log_style_change.take().or(log);
                self.footprint = self.footprint.take().or(settings.footprint);
                self.persistence.extend(settings.persistence);
            }
            HostEffect::IndicatorPicker(picked) => {
                for effect in picked {
                    self.picker(effect);
                }
            }
            HostEffect::ScriptEditor(edited) => {
                for effect in edited {
                    if let EditorEffect::Requests {
                        actions,
                        add_to_chart,
                    } = effect
                    {
                        self.script_editor_actions.extend(actions);
                        self.indicator_actions
                            .extend(add_to_chart.map(ToolbarAction::AddScriptIndicator));
                    }
                }
            }
            _ => {}
        }
    }

    fn picker(&mut self, effect: PickerEffect) {
        match effect {
            PickerEffect::Add(Choice::Native(id)) => {
                self.indicator_actions.push(ToolbarAction::AddNative(id));
            }
            PickerEffect::Add(Choice::Script(index)) => {
                (self.indicator_actions).push(ToolbarAction::AddScriptIndicator(index));
            }
            PickerEffect::ViewSource(id) => {
                (self.script_editor_actions).push(ScriptEditorAction::Open { id });
            }
            PickerEffect::Active(slot, action) => self.indicator_actions.push(match action {
                ActiveAction::ToggleHidden => ToolbarAction::ToggleIndicatorHidden(slot),
                ActiveAction::Settings => ToolbarAction::OpenIndicatorSettings(slot),
                ActiveAction::Remove => ToolbarAction::RemoveIndicator(slot),
            }),
        }
    }
}

#[cfg(test)]
#[path = "late_effects/tests.rs"]
mod tests;
