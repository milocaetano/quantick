//! Settings: whether it is up, the section on screen, the facts its rows show and
//! what a row asked for. The host owns every value; a row only asks.
use quantick_appearance::Builtin;
use quantick_civil::TzOffset;
use quantick_control_handlers::settings::{SettingsAct, SettingsReadings};
use quantick_control_schema::settings_sections::{SettingRow, SettingsSection};

/// The values the rows show, read from their owners while Settings is up.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub readings: SettingsReadings,
    /// The local agent gateway, as its panel reads it; the default is no gateway.
    pub agent: crate::access_panel::Facts,
    pub agent_note: Option<AgentNote>,
}

/// The line under the agent switch.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentNote {
    Error(String),
    Status(String),
}

impl AgentNote {
    /// The gateway's start error, else its status; a failure outranks news.
    pub fn of(start_error: Option<&str>, status: Option<&str>) -> Option<Self> {
        match start_error {
            Some(error) => Some(Self::Error(error.to_owned())),
            None => status.map(|note| Self::Status(note.to_owned())),
        }
    }
}

/// What a row draws on its right.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Control {
    Button,
    /// The appearance button and the theme menu beside it.
    Appearance(Option<Builtin>),
    Timezone(TzOffset),
    Switch {
        on: bool,
        enabled: bool,
    },
}

#[derive(Debug, Default, PartialEq)]
pub struct State {
    open: bool,
    section: SettingsSection,
    facts: Facts,
    theme_error: Option<String>,
    /// The theme last picked, until the menu closes on it.
    theme_picked: Option<Builtin>,
}

impl State {
    /// The section on screen, while Settings is up.
    pub fn shown(&self) -> Option<SettingsSection> {
        self.open.then_some(self.section)
    }
    pub fn facts(&self) -> &Facts {
        &self.facts
    }
    pub fn theme_error(&self) -> Option<&str> {
        self.theme_error.as_deref()
    }
    /// The picked theme is the one in use: the menu closes. A failed pick
    /// keeps it open, with the reason inside.
    pub fn theme_landed(&self) -> bool {
        self.theme_picked.is_some() && self.theme_picked == self.facts.readings.theme
    }

    /// The control `row` draws, from the observed facts.
    pub fn control(&self, row: SettingRow) -> Control {
        let facts = &self.facts.readings;
        let switch = |on| Control::Switch { on, enabled: true };
        match row {
            SettingRow::Appearance => Control::Appearance(facts.theme),
            SettingRow::Timezone => Control::Timezone(facts.timezone),
            SettingRow::PerfReadings => switch(facts.show_perf),
            SettingRow::ProgressiveHistory => switch(facts.progressive_history),
            SettingRow::VenueLeadIn => switch(facts.venue_lead_in),
            SettingRow::RecordDeals => switch(facts.record_deals),
            SettingRow::AgentSwitch => {
                let (on, enabled) = self.facts.agent.switch_state();
                Control::Switch { on, enabled }
            }
            SettingRow::ManageClients => Control::Button,
        }
    }
}

#[derive(Debug)]
pub enum Intent {
    Open(SettingsSection),
    Close,
    Show(SettingsSection),
    Observe(Box<Facts>),
    Press(SettingRow),
    Flip(SettingRow, bool),
    PickTimezone(TzOffset),
    PickTheme(Builtin),
    /// The theme could not be applied; the words stay in its menu until the
    /// next pick or the next opening.
    ThemeFailed(String),
    /// The menu closed on the theme that landed.
    ThemeMenuClosed,
}

/// What a row asked its owner to do.
#[derive(Debug, Default, PartialEq)]
pub struct Effects {
    pub action: Option<SettingsAct>,
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;

    fn update(state: &mut State, intent: Intent) -> Effects {
        let action = match intent {
            Intent::Open(section) => {
                if !state.open {
                    state.theme_error = None;
                }
                state.open = true;
                state.section = section;
                None
            }
            Intent::Close => {
                state.open = false;
                None
            }
            Intent::Show(section) => {
                state.section = section;
                None
            }
            Intent::Observe(facts) => {
                state.facts = *facts;
                None
            }
            Intent::Press(row) => press(row),
            Intent::Flip(row, on) => flip(state, row, on),
            Intent::PickTimezone(tz) => {
                (tz != state.facts.readings.timezone).then_some(SettingsAct::Timezone(tz))
            }
            Intent::PickTheme(choice) => {
                state.theme_error = None;
                state.theme_picked = Some(choice);
                Some(SettingsAct::Theme(choice))
            }
            Intent::ThemeFailed(error) => {
                state.theme_error = Some(error);
                state.theme_picked = None;
                None
            }
            Intent::ThemeMenuClosed => {
                state.theme_picked = None;
                None
            }
        };
        Effects { action }
    }
}

fn press(row: SettingRow) -> Option<SettingsAct> {
    match row {
        SettingRow::Appearance => Some(SettingsAct::OpenAppearance),
        SettingRow::ManageClients => Some(SettingsAct::OpenAgentAccess),
        _ => None,
    }
}

/// A switch asks only when it can move and its value changes.
fn flip(state: &State, row: SettingRow, on: bool) -> Option<SettingsAct> {
    match state.control(row) {
        Control::Switch { on: was, enabled } if enabled && was != on => {}
        _ => return None,
    }
    match row {
        SettingRow::PerfReadings => Some(SettingsAct::PerfReadings(on)),
        SettingRow::ProgressiveHistory => Some(SettingsAct::ProgressiveHistory(on)),
        SettingRow::VenueLeadIn => Some(SettingsAct::VenueLeadIn(on)),
        SettingRow::RecordDeals => Some(SettingsAct::RecordDeals(on)),
        SettingRow::AgentSwitch => Some(SettingsAct::AgentSwitch(on)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "settings_window/tests.rs"]
mod tests;
