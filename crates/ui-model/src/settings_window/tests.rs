use super::*;
use crate::Model as _;

fn opened(facts: Facts) -> State {
    let mut state = State::default();
    Model::update(&mut state, Intent::Open(SettingsSection::Display));
    Model::update(&mut state, Intent::Observe(Box::new(facts)));
    state
}

fn action(state: &mut State, intent: Intent) -> Option<SettingsAct> {
    Model::update(state, intent).action
}

#[test]
fn settings_opens_on_the_asked_section_moves_between_sections_and_closes() {
    let mut state = State::default();
    assert_eq!(state.shown(), None);
    for section in SettingsSection::ALL {
        assert_eq!(action(&mut state, Intent::Open(section)), None);
        assert_eq!(state.shown(), Some(section));
    }
    action(&mut state, Intent::Show(SettingsSection::Data));
    assert_eq!(state.shown(), Some(SettingsSection::Data));
    action(&mut state, Intent::Close);
    assert_eq!(state.shown(), None, "a closed window shows no section");
    action(&mut state, Intent::Open(SettingsSection::AgentAccess));
    assert_eq!(state.shown(), Some(SettingsSection::AgentAccess));
}

#[test]
fn each_switch_asks_its_owner_only_when_its_value_changes() {
    let mut state = opened(Facts {
        readings: SettingsReadings {
            show_perf: true,
            ..SettingsReadings::default()
        },
        ..Facts::default()
    });
    assert_eq!(
        action(&mut state, Intent::Flip(SettingRow::PerfReadings, true)),
        None,
        "already on"
    );
    for (row, asked) in [
        (SettingRow::PerfReadings, SettingsAct::PerfReadings(false)),
        (
            SettingRow::ProgressiveHistory,
            SettingsAct::ProgressiveHistory(true),
        ),
        (SettingRow::VenueLeadIn, SettingsAct::VenueLeadIn(true)),
        (SettingRow::RecordDeals, SettingsAct::RecordDeals(true)),
    ] {
        let on = !matches!(row, SettingRow::PerfReadings);
        assert_eq!(action(&mut state, Intent::Flip(row, on)), Some(asked));
    }
    assert_eq!(
        action(&mut state, Intent::Flip(SettingRow::ManageClients, true)),
        None,
        "a button is not a switch"
    );
}

#[test]
fn the_agent_switch_moves_only_when_its_gateway_lets_it() {
    let agent = |can_flip| Facts {
        agent: crate::access_panel::Facts {
            identity_available: can_flip,
            ..Default::default()
        },
        ..Facts::default()
    };
    let mut locked = opened(agent(false));
    assert_eq!(
        locked.control(SettingRow::AgentSwitch),
        Control::Switch {
            on: false,
            enabled: false
        }
    );
    assert_eq!(
        action(&mut locked, Intent::Flip(SettingRow::AgentSwitch, true)),
        None
    );
    let mut free = opened(agent(true));
    assert_eq!(
        action(&mut free, Intent::Flip(SettingRow::AgentSwitch, true)),
        Some(SettingsAct::AgentSwitch(true))
    );
}

#[test]
fn buttons_timezone_and_theme_ask_their_owners() {
    let mut state = opened(Facts::default());
    assert_eq!(
        action(&mut state, Intent::Press(SettingRow::Appearance)),
        Some(SettingsAct::OpenAppearance)
    );
    assert_eq!(
        action(&mut state, Intent::Press(SettingRow::ManageClients)),
        Some(SettingsAct::OpenAgentAccess)
    );
    assert_eq!(
        action(&mut state, Intent::Press(SettingRow::Timezone)),
        None
    );
    let same = state.facts().readings.timezone;
    assert_eq!(action(&mut state, Intent::PickTimezone(same)), None);
    let other = TzOffset::ALL
        .into_iter()
        .find(|offset| *offset != same)
        .expect("a second offset");
    assert_eq!(
        action(&mut state, Intent::PickTimezone(other)),
        Some(SettingsAct::Timezone(other))
    );
    let choice = Builtin::ALL[0];
    action(&mut state, Intent::PickTheme(choice));
    assert!(!state.theme_landed(), "not in use yet");
    action(&mut state, Intent::ThemeFailed("disk full".to_owned()));
    assert!(!state.theme_landed(), "a failed pick keeps the menu open");
    assert_eq!(state.theme_error(), Some("disk full"));
    assert_eq!(
        action(&mut state, Intent::PickTheme(choice)),
        Some(SettingsAct::Theme(choice))
    );
    assert_eq!(
        state.theme_error(),
        None,
        "a new pick clears the old failure"
    );
    action(&mut state, Intent::ThemeFailed("disk full".to_owned()));
    action(&mut state, Intent::Open(SettingsSection::Data));
    assert_eq!(
        state.theme_error(),
        Some("disk full"),
        "another door onto the open window keeps the failure on screen"
    );
    action(&mut state, Intent::Close);
    action(&mut state, Intent::Open(SettingsSection::Display));
    assert_eq!(
        state.theme_error(),
        None,
        "reopening forgets an old failure"
    );
}

#[test]
fn each_row_draws_the_control_its_facts_describe() {
    let theme = Some(Builtin::ALL[0]);
    let state = opened(Facts {
        readings: SettingsReadings {
            theme,
            record_deals: true,
            ..SettingsReadings::default()
        },
        ..Facts::default()
    });
    assert_eq!(
        state.control(SettingRow::Appearance),
        Control::Appearance(theme)
    );
    assert_eq!(
        state.control(SettingRow::Timezone),
        Control::Timezone(TzOffset::default())
    );
    assert_eq!(
        state.control(SettingRow::RecordDeals),
        Control::Switch {
            on: true,
            enabled: true
        }
    );
    assert_eq!(state.control(SettingRow::ManageClients), Control::Button);
}

#[test]
fn the_agent_note_shows_a_start_error_over_the_status() {
    assert_eq!(
        AgentNote::of(Some("port taken"), Some("Enabled")),
        Some(AgentNote::Error("port taken".to_owned()))
    );
    assert_eq!(
        AgentNote::of(None, Some("Enabled")),
        Some(AgentNote::Status("Enabled".to_owned()))
    );
    assert_eq!(AgentNote::of(None, None), None);
}
