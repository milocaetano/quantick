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
    /// An appearance change settled and is owed a log line, naming the preset
    /// the trader clicked if they clicked one.
    pub log_style_change: Option<Option<CandlePreset>>,
    pub footprint: Option<FootprintChange>,
    pub persistence: Vec<Persistence>,
    pub arm_strategy: Option<ArmRequest>,
    /// A sound was auditioned, by the alert editor or the arming dialog.
    pub test_alert: Option<Cue>,
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
                self.log_style_change = self.log_style_change.take().or(settings.log);
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
mod tests {
    use super::*;
    use crate::toast::Effects;
    use quantick_alerts::sound::AlertSound;

    fn remove(symbol: &str) -> PaletteIntent {
        PaletteIntent::Remove {
            feed_id: "binance".to_owned(),
            symbol: symbol.to_owned(),
        }
    }

    #[test]
    fn the_first_palette_ask_wins_and_undo_folds_to_one() {
        let late = LateEffects::fold([
            HostEffect::MarketPalette(None),
            HostEffect::Toast(Effects { undo_drawing: true }),
            HostEffect::MarketPalette(Some(remove("FIRST"))),
            HostEffect::Toast(Effects { undo_drawing: true }),
            HostEffect::MarketPalette(Some(remove("SECOND"))),
        ]);
        assert_eq!(
            late,
            LateEffects {
                market: Some(remove("FIRST")),
                undo_drawing: true,
                ..LateEffects::default()
            }
        );
    }

    #[test]
    fn a_quiet_toast_or_another_surface_asks_nothing() {
        let late = LateEffects::fold([
            HostEffect::Toast(Effects {
                undo_drawing: false,
            }),
            HostEffect::BarSwitch(None),
        ]);
        assert_eq!(late, LateEffects::default());
    }

    fn cue(sound: AlertSound) -> Cue {
        Cue::new(sound, None)
    }

    fn arm(label: &str) -> ArmRequest {
        ArmRequest {
            side: quantick_chart_interaction::pane::PaneSide::Flow,
            drawing: quantick_drawings::DrawingId(1),
            form: Box::new(quantick_strategy::presets::StoredPreset::starting_point(
                quantick_engine::Side::Buy,
            )),
            label: label.to_owned(),
        }
    }

    #[test]
    fn alert_and_strategy_asks_go_to_their_first_asker() {
        let late = LateEffects::fold([
            HostEffect::StrategyPopup(crate::strategy_popup::Effects {
                arm: Some(arm("first")),
                test_alert: Some(cue(AlertSound::Beep)),
                bank: None,
            }),
            HostEffect::Alerts(crate::alerts::Effects {
                request: Some(Box::new(AlertRequest::default())),
                preview: Some(cue(AlertSound::Question)),
            }),
            HostEffect::StrategyPopup(crate::strategy_popup::Effects {
                arm: Some(arm("second")),
                test_alert: None,
                bank: None,
            }),
            HostEffect::Alerts(crate::alerts::Effects {
                request: Some(Box::new(AlertRequest {
                    open: true,
                    ..AlertRequest::default()
                })),
                preview: None,
            }),
        ]);
        assert_eq!(
            late.arm_strategy.map(|arm| arm.label),
            Some("first".to_owned())
        );
        assert_eq!(late.test_alert, Some(cue(AlertSound::Beep)));
        assert_eq!(late.alerts, Some(Box::new(AlertRequest::default())));
    }

    #[test]
    fn chart_settings_asks_keep_the_first_value_and_every_persistence() {
        let mut edited = ChartStyle::default();
        edited.candles.corner_radius += 1.0;
        let late = LateEffects::fold([
            HostEffect::ChartSettings(crate::chart_settings::Effects {
                style: Some(edited),
                footprint: Some(FootprintChange::ResetToDefault),
                log: Some(Some(CandlePreset::Classic)),
                persistence: vec![Persistence::DeleteCandle("a".to_owned())],
            }),
            HostEffect::ChartSettings(crate::chart_settings::Effects {
                style: Some(ChartStyle::default()),
                footprint: None,
                log: Some(None),
                persistence: vec![Persistence::SaveFootprint],
            }),
        ]);
        assert_eq!(late.style, Some(edited));
        assert_eq!(late.footprint, Some(FootprintChange::ResetToDefault));
        assert_eq!(late.log_style_change, Some(Some(CandlePreset::Classic)));
        assert_eq!(
            late.persistence,
            vec![
                Persistence::DeleteCandle("a".to_owned()),
                Persistence::SaveFootprint
            ]
        );
    }

    #[test]
    fn picker_and_editor_actions_keep_draw_order() {
        let late = LateEffects::fold([
            HostEffect::IndicatorPicker(vec![
                PickerEffect::Add(Choice::Native("native.cvd")),
                PickerEffect::Add(Choice::Script(2)),
                PickerEffect::ViewSource("ema".to_owned()),
                PickerEffect::Active(4, ActiveAction::ToggleHidden),
                PickerEffect::Active(5, ActiveAction::Settings),
                PickerEffect::Active(6, ActiveAction::Remove),
            ]),
            HostEffect::ScriptEditor(vec![EditorEffect::Requests {
                actions: vec![ScriptEditorAction::Close],
                add_to_chart: Some(3),
            }]),
        ]);
        assert_eq!(
            late.indicator_actions,
            vec![
                ToolbarAction::AddNative("native.cvd"),
                ToolbarAction::AddScriptIndicator(2),
                ToolbarAction::ToggleIndicatorHidden(4),
                ToolbarAction::OpenIndicatorSettings(5),
                ToolbarAction::RemoveIndicator(6),
                ToolbarAction::AddScriptIndicator(3),
            ]
        );
        assert_eq!(
            late.script_editor_actions,
            vec![
                ScriptEditorAction::Open {
                    id: "ema".to_owned()
                },
                ScriptEditorAction::Close
            ]
        );
    }

    #[test]
    fn the_first_bar_switch_wins() {
        let config = quantick_engine::bar_registry::BUILTIN_BARS.quick_matches("1m")[0];
        let request = |tab| crate::bar_switch::BarSwitchRequest {
            tab,
            side: quantick_chart_interaction::pane::PaneSide::Flow,
            config,
        };
        let first = request(1);
        let late = LateEffects::fold([
            HostEffect::BarSwitch(None),
            HostEffect::BarSwitch(Some(request(1))),
            HostEffect::BarSwitch(Some(request(2))),
        ]);
        assert_eq!(late.bar_switch, Some(first));
    }
}
