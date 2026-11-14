use super::*;
use crate::Model as _;
use quantick_stores::script_library::ScriptOrigin;
fn fixture() -> State {
    let mut state = State::default();
    Model::update(
        &mut state,
        Intent::Refresh {
            target: (9, 3),
            scripts: vec![ScriptEntry {
                name: "ema.pine".into(),
                origin: ScriptOrigin::Embedded(""),
            }],
            active: vec![ActiveEntry {
                slot: 17,
                label: "EMA".into(),
                hidden: false,
                errored: true,
                stale: false,
            }],
        },
    );
    Model::update(&mut state, Intent::Open((9, 3)));
    state
}
#[test]
fn migration_preserves_state_and_effects() {
    for (action, remains_open) in [
        (ActiveAction::ToggleHidden, true),
        (ActiveAction::Settings, false),
        (ActiveAction::Remove, false),
    ] {
        let mut state = fixture();
        Model::update(&mut state, Intent::Query("ema".into()));
        Model::update(&mut state, Intent::Category(Category::Active));
        assert_eq!(
            Model::update(&mut state, Intent::ActiveAction(17, action)),
            vec![Effect::Active(17, action)]
        );
        assert_eq!(state.picker.target.is_some(), remains_open);
    }
    for choice in [Choice::Native("native.ema"), Choice::Script(0)] {
        let mut state = fixture();
        assert_eq!(
            Model::update(&mut state, Intent::SelectEntry(choice.clone())),
            vec![Effect::Add(choice)]
        );
        assert!(state.picker.target.is_none());
        assert!(!state.focus_requested);
    }
    let mut state = fixture();
    assert_eq!(
        Model::update(&mut state, Intent::ViewSource(0)),
        vec![Effect::ViewSource("ema.pine".into())]
    );
    assert!(state.picker.target.is_none());
}
#[test]
fn control_search_uses_the_same_picker_and_filename_identity() {
    use quantick_control_schema::indicator_picker::PickerAction;
    let mut state = fixture();
    let scripts = state.scripts.clone();
    Model::update(
        &mut state,
        Intent::Control {
            target: (9, 3),
            scripts,
            input: PickerInput {
                operation: PickerAction::Search {
                    query: "eMa".into(),
                    category: "all".into(),
                },
                offset: 0,
                limit: 64,
            },
        },
    );
    let result = state.control_result.take().unwrap().unwrap();
    assert_eq!(state.picker.query, "eMa");
    assert!(
        result
            .entries
            .iter()
            .all(|entry| entry.name.to_lowercase().contains("ema") || entry.id == "script.0")
    );
    assert!(
        result
            .entries
            .iter()
            .any(|entry| entry.id == "script.0" && entry.name == "Exponential Moving Average")
    );
    assert_eq!(
        state
            .entries
            .iter()
            .map(|entry| entry.choice.clone())
            .collect::<Vec<_>>(),
        vec![Choice::Native("native.ema"), Choice::Script(0)]
    );
}
#[test]
fn unchanged_refresh_keeps_projection_and_focus_is_consumed_once() {
    let mut state = fixture();
    assert!(state.focus_requested);
    Model::update(&mut state, Intent::FocusHandled);
    let entries = state.entries.as_ptr();
    let active = state.active.as_ptr();
    let scripts = state.scripts.clone();
    let indicators = state.indicators.clone();
    Model::update(
        &mut state,
        Intent::Refresh {
            target: (9, 3),
            scripts,
            active: indicators,
        },
    );
    Model::update(&mut state, Intent::Open((9, 3)));
    assert_eq!(state.entries.as_ptr(), entries);
    assert_eq!(state.active.as_ptr(), active);
    assert!(!state.focus_requested);
    Model::update(
        &mut state,
        Intent::Refresh {
            target: (9, 4),
            scripts: vec![],
            active: vec![],
        },
    );
    assert!(state.picker.target.is_none());
    assert!(
        Model::update(
            &mut state,
            Intent::SelectEntry(Choice::Native("native.ema"))
        )
        .is_empty()
    );
}
#[test]
fn stale_or_filtered_actions_have_no_effect() {
    let mut state = fixture();
    Model::update(&mut state, Intent::Query("absent".into()));
    assert!(
        Model::update(
            &mut state,
            Intent::SelectEntry(Choice::Native("native.ema"))
        )
        .is_empty()
    );
    assert!(Model::update(&mut state, Intent::ViewSource(0)).is_empty());
    assert!(Model::update(&mut state, Intent::ActiveAction(17, ActiveAction::Remove)).is_empty());
    assert!(state.picker.target.is_some());
    assert!(Model::update(&mut state, Intent::Close).is_empty());
    assert!(state.picker.target.is_none());
}

#[test]
fn active_status_preserves_error_stale_hidden_precedence() {
    let mut entry = fixture().active.remove(0);
    entry.hidden = true;
    entry.stale = true;
    assert_eq!(entry.status(), Status::Errored);
    entry.errored = false;
    assert_eq!(entry.status(), Status::Stale);
    entry.stale = false;
    assert_eq!(entry.status(), Status::Hidden);
    entry.hidden = false;
    assert_eq!(entry.status(), Status::Running);
}
