//! Strategy arming presentation state; execution and preset IO stay with the host.
use quantick_chart_interaction::pane::PaneSide;
use quantick_drawings::DrawingId;
use quantick_strategy::{
    presets::StoredPreset,
    sound::{AlertSound, Cue},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ArmRequest {
    pub side: PaneSide,
    pub drawing: DrawingId,
    pub form: Box<StoredPreset>,
    pub label: String,
}

#[derive(Debug)]
pub struct Popup {
    pub tab: u64,
    pub side: PaneSide,
    pub drawing: DrawingId,
    pub form: StoredPreset,
    pub preset_choice: Option<String>,
    pub save_name: String,
    pub error: Option<String>,
}
#[derive(Default, Debug)]
pub struct State {
    popup: Option<Popup>,
    rows: Vec<(String, StoredPreset)>,
    pending_sound_picker: bool,
    pub shares_available: bool,
    pub alert_failure: Option<String>,
}
impl State {
    pub fn popup(&self) -> Option<&Popup> {
        self.popup.as_ref()
    }
    pub fn is_open(&self) -> bool {
        self.popup.is_some()
    }
    pub fn rows(&self) -> &[(String, StoredPreset)] {
        &self.rows
    }
    pub fn sound_picker_pending(&self) -> bool {
        self.pending_sound_picker
    }
}

/// Text fields retain the domain preset's representation, including invalid drafts.
#[derive(Debug, Clone, Copy)]
pub enum TextField {
    Quantity,
    MinFactor,
    MaxFactor,
    MinRange,
    TpMult,
    SlMult,
}
impl TextField {
    pub fn read(self, form: &StoredPreset) -> &str {
        match self {
            Self::Quantity => &form.quantity,
            Self::MinFactor => &form.min_factor,
            Self::MaxFactor => &form.max_factor,
            Self::MinRange => &form.min_range,
            Self::TpMult => &form.tp_mult,
            Self::SlMult => &form.sl_mult,
        }
    }
    fn set(self, form: &mut StoredPreset, value: String) {
        match self {
            Self::Quantity => form.quantity = value,
            Self::MinFactor => form.min_factor = value,
            Self::MaxFactor => form.max_factor = value,
            Self::MinRange => form.min_range = value,
            Self::TpMult => form.tp_mult = value,
            Self::SlMult => form.sl_mult = value,
        }
    }
}

/// Each editor changes only its named field, never replaces an authoritative form.
#[derive(Debug)]
pub enum Field {
    Side(bool),
    Text(TextField, String),
    Window(u32),
    Rearm(bool),
    OnBreak(bool),
    Alarm(bool),
    AlarmWhen(bool),
    AlarmShare(u32),
    AlarmRepeat(bool),
    AlarmCooldown(u32),
    AlarmSound(AlertSound),
    AlarmCut(bool),
    AlarmSeconds(u32),
    AlarmOnly(bool),
}
#[derive(Debug)]
pub enum Intent {
    Open {
        tab: u64,
        side: PaneSide,
        drawing: DrawingId,
        form: Box<StoredPreset>,
    },
    Observe {
        active_tab: u64,
        counted_bar_sides: Vec<PaneSide>,
        alert_failure: Option<String>,
    },
    BankSnapshot(Vec<(String, StoredPreset)>),
    Edit(Field),
    SelectPreset(String),
    SaveName(String),
    SavePreset,
    DeletePreset,
    Submit,
    Cancel,
    SettleArm(Result<(), String>),
    StageSoundPicker,
    AlarmSection,
    TestSound {
        current: AlertSound,
    },
}
#[derive(Debug, PartialEq)]
pub enum BankEffect {
    Save {
        name: String,
        form: Box<StoredPreset>,
    },
    Remove(String),
}
#[derive(Default, Debug, PartialEq)]
pub struct Effects {
    pub arm: Option<ArmRequest>,
    pub test_alert: Option<Cue>,
    pub bank: Option<BankEffect>,
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        let mut effects = Effects::default();
        match intent {
            Intent::Open {
                tab,
                side,
                drawing,
                form,
            } => {
                state.popup = Some(Popup {
                    tab,
                    side,
                    drawing,
                    form: *form,
                    preset_choice: None,
                    save_name: String::new(),
                    error: None,
                });
            }
            Intent::Observe {
                active_tab,
                counted_bar_sides,
                alert_failure,
            } => {
                if state.popup.as_ref().is_some_and(|p| p.tab != active_tab) {
                    state.popup = None;
                }
                state.shares_available = state
                    .popup
                    .as_ref()
                    .is_some_and(|p| counted_bar_sides.contains(&p.side));
                if state.alert_failure != alert_failure {
                    state.alert_failure = alert_failure;
                }
            }
            Intent::BankSnapshot(rows) => state.rows = rows,
            Intent::StageSoundPicker => state.pending_sound_picker = true,
            Intent::AlarmSection => state.pending_sound_picker = false,
            Intent::Cancel | Intent::SettleArm(Ok(())) => state.popup = None,
            Intent::SettleArm(Err(error)) => {
                if let Some(popup) = state.popup.as_mut() {
                    popup.error = Some(error);
                }
            }
            intent => {
                let Some(popup) = state.popup.as_mut() else {
                    return effects;
                };
                match intent {
                    Intent::Edit(field) => edit(&mut popup.form, field, state.shares_available),
                    Intent::SelectPreset(name) => {
                        if let Some((_, form)) = state.rows.iter().find(|(row, _)| *row == name) {
                            popup.form = form.clone();
                            popup.preset_choice = Some(name);
                        }
                    }
                    Intent::SaveName(name) => popup.save_name = name,
                    Intent::SavePreset => {
                        let name = popup.save_name.trim();
                        if !name.is_empty() {
                            effects.bank = Some(BankEffect::Save {
                                name: name.to_owned(),
                                form: Box::new(popup.form.clone()),
                            });
                            popup.preset_choice = Some(name.to_owned());
                        }
                    }
                    Intent::DeletePreset => {
                        effects.bank = popup.preset_choice.take().map(BankEffect::Remove);
                    }
                    Intent::Submit => {
                        let label = popup
                            .preset_choice
                            .clone()
                            .or_else(|| {
                                let name = popup.save_name.trim();
                                (!name.is_empty()).then(|| name.to_owned())
                            })
                            .unwrap_or_else(|| "custom".into());
                        effects.arm = Some(ArmRequest {
                            side: popup.side,
                            drawing: popup.drawing,
                            form: Box::new(popup.form.clone()),
                            label,
                        });
                    }
                    Intent::TestSound { current } => {
                        effects.test_alert = Some(Cue::new(current, popup.form.alarm_play_secs))
                    }
                    _ => unreachable!("lifecycle intents handled above"),
                }
            }
        }
        effects
    }
}
fn edit(form: &mut StoredPreset, field: Field, shares_available: bool) {
    match field {
        Field::Side(v) => form.side = if v { "buy" } else { "sell" }.into(),
        Field::Text(field, value) => field.set(form, value),
        Field::Window(v) => form.window = v,
        Field::Rearm(v) => form.rearm = if v { "auto" } else { "one_shot" }.into(),
        Field::OnBreak(v) => form.on_break = if v { "retest_limit" } else { "ignore" }.into(),
        Field::Alarm(v) => {
            form.alarm = v;
            if !v {
                form.alarm_only = false;
            }
        }
        Field::AlarmWhen(v) => {
            if !v || shares_available {
                form.alarm_when = if v { "share" } else { "on_close" }.into();
            }
        }
        Field::AlarmShare(v) => form.alarm_share_percent = v,
        Field::AlarmRepeat(v) => {
            form.alarm_repeat = if v { "cooldown" } else { "once_per_bar" }.into()
        }
        Field::AlarmCooldown(v) => form.alarm_cooldown_secs = v,
        Field::AlarmSound(v) => form.alarm_sound = v.token().into(),
        Field::AlarmCut(v) => {
            form.alarm_play_secs = v.then_some(quantick_strategy::presets::DEFAULT_ALARM_PLAY_SECS)
        }
        Field::AlarmSeconds(v) => form.alarm_play_secs = Some(v),
        Field::AlarmOnly(v) => form.alarm_only = v,
    }
}
#[cfg(test)]
#[path = "strategy_popup/tests/mod.rs"]
mod tests;
