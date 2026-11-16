use super::*;
use crate::Model as _;

fn opened(facts: Facts) -> State {
    let mut state = State::default();
    Model::update(&mut state, Intent::Open(SettingsSection::Display));
    Model::update(&mut state, Intent::Observe(Box::new(facts)));
    state
}

fn action(state: &mut State, intent: Intent) -> Option<Action> {
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
        show_perf: true,
        ..Facts::default()
    });
    assert_eq!(
        action(&mut state, Intent::Flip(SettingRow::PerfReadings, true)),
        None,
        "already on"
    );
    for (row, asked) in [
        (SettingRow::PerfReadings, Action::PerfReadings(false)),
        (SettingRow::ProgressiveHistory, Action::ProgressiveHistory(true)),
        (SettingRow::VenueLeadIn, Action::VenueLeadIn(true)),
        (SettingRow::RecordDeals, Action::RecordDeals(true)),
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
        agent: AgentFacts {
            can_flip,
            ..AgentFacts::default()
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
        Some(Action::AgentSwitch(true))
    );
}

#[test]
fn buttons_timezone_and_theme_ask_their_owners() {
    let mut state = opened(Facts::default());
    assert_eq!(
        action(&mut state, Intent::Press(SettingRow::Appearance)),
        Some(Action::OpenAppearance)
    );
    assert_eq!(
        action(&mut state, Intent::Press(SettingRow::ManageClients)),
        Some(Action::OpenAgentAccess)
    );
    assert_eq!(
        action(&mut state, Intent::Press(SettingRow::Timezone)),
        None
    );
    let same = state.facts().timezone;
    assert_eq!(action(&mut state, Intent::PickTimezone(same)), None);
    let other = TzOffset::ALL
        .into_iter()
        .find(|offset| *offset != same)
        .expect("a second offset");
    assert_eq!(
        action(&mut state, Intent::PickTimezone(other)),
        Some(Action::Timezone(other))
    );
    let choice = Builtin::ALL[0];
    action(&mut state, Intent::ThemeFailed("disk full".to_owned()));
    assert_eq!(state.theme_error(), Some("disk full"));
    assert_eq!(
        action(&mut state, Intent::PickTheme(choice)),
        Some(Action::Theme(choice))
    );
    assert_eq!(state.theme_error(), None, "a new pick clears the old failure");
}

#[test]
fn each_row_draws_the_control_its_facts_describe() {
    let theme = Some(Builtin::ALL[0]);
    let state = opened(Facts {
        theme,
        record_deals: true,
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
