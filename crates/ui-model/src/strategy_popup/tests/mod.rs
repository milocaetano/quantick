use super::*;
use crate::Model as _;
fn update(state: &mut State, intent: Intent) -> Effects {
    Model::update(state, intent)
}
fn opened() -> State {
    let mut state = State::default();
    update(
        &mut state,
        Intent::Open {
            tab: 7,
            side: PaneSide::Flow,
            drawing: DrawingId(1),
            form: Box::new(StoredPreset::starting_point(quantick_engine::Side::Buy)),
        },
    );
    state
}
fn quantity(state: &State) -> &str {
    &state.popup().unwrap().form.quantity
}
#[test]
fn switching_tabs_closes_the_dialog() {
    let mut state = opened();
    update(
        &mut state,
        Intent::Observe {
            active_tab: 8,
            counted_bar_sides: vec![],
            alert_failure: None,
        },
    );
    assert!(!state.is_open(), "stable owner identity changed");
}
#[test]
fn a_refused_arming_keeps_the_form_and_shows_why() {
    let mut state = opened();
    update(
        &mut state,
        Intent::Edit(Field::Text(TextField::Quantity, "not a number".into())),
    );
    let effect = update(&mut state, Intent::Submit);
    assert_eq!(effect.arm.unwrap().form.quantity, "not a number");
    assert!(state.is_open(), "submit waits for the real host answer");
    update(
        &mut state,
        Intent::SettleArm(Err("a field does not parse".into())),
    );
    assert_eq!(
        state.popup().unwrap().error.as_deref(),
        Some("a field does not parse")
    );
    assert_eq!(quantity(&state), "not a number");
}
#[test]
fn an_accepted_arming_closes_the_dialog() {
    let mut state = opened();
    update(&mut state, Intent::Submit);
    update(&mut state, Intent::SettleArm(Ok(())));
    assert!(!state.is_open());
}
#[test]
fn an_answer_for_a_dialog_that_left_is_dropped() {
    let mut state = opened();
    update(&mut state, Intent::Cancel);
    update(&mut state, Intent::SettleArm(Err("too late".into())));
    assert!(!state.is_open());
}
#[test]
fn migration_preserves_state_and_effects() {
    let mut state = opened();
    update(
        &mut state,
        Intent::Edit(Field::Text(TextField::Quantity, "3".into())),
    );
    update(
        &mut state,
        Intent::Observe {
            active_tab: 7,
            counted_bar_sides: vec![PaneSide::Flow],
            alert_failure: None,
        },
    );
    assert_eq!(quantity(&state), "3");
    let submitted = update(&mut state, Intent::Submit).arm.unwrap();
    assert_eq!(submitted.label, "custom");
    assert_eq!(submitted.side, PaneSide::Flow);
    assert_eq!(submitted.drawing, DrawingId(1));
    update(
        &mut state,
        Intent::Edit(Field::Text(TextField::Quantity, "4".into())),
    );
    assert_eq!(
        submitted.form.quantity, "3",
        "submitted snapshot is independent"
    );
    assert_eq!(quantity(&state), "4");
}
#[test]
fn preset_choice_trimmed_name_and_custom_have_existing_precedence() {
    let mut state = opened();
    let mut preset = state.popup().unwrap().form.clone();
    preset.quantity = "12".into();
    update(
        &mut state,
        Intent::BankSnapshot(vec![("saved".into(), preset.clone())]),
    );
    update(&mut state, Intent::SaveName("  draft  ".into()));
    assert_eq!(
        update(&mut state, Intent::Submit).arm.unwrap().label,
        "draft"
    );
    update(&mut state, Intent::SelectPreset("saved".into()));
    assert_eq!(quantity(&state), "12");
    assert_eq!(
        update(&mut state, Intent::Submit).arm.unwrap().label,
        "saved"
    );
    assert_eq!(
        update(&mut state, Intent::SavePreset).bank,
        Some(BankEffect::Save {
            name: "draft".into(),
            form: Box::new(preset)
        })
    );
    assert_eq!(
        update(&mut state, Intent::DeletePreset).bank,
        Some(BankEffect::Remove("draft".into()))
    );
    assert_eq!(quantity(&state), "12");
    update(&mut state, Intent::SaveName("  ".into()));
    assert_eq!(update(&mut state, Intent::SavePreset), Effects::default());
    assert_eq!(
        update(&mut state, Intent::Submit).arm.unwrap().label,
        "custom"
    );
}
#[test]
fn picker_waits_for_alarm_section_across_open_and_owner_mismatch() {
    let mut state = State::default();
    update(&mut state, Intent::StageSoundPicker);
    update(
        &mut state,
        Intent::Open {
            tab: 7,
            side: PaneSide::Flow,
            drawing: DrawingId(1),
            form: Box::new(StoredPreset::starting_point(quantick_engine::Side::Buy)),
        },
    );
    assert!(state.sound_picker_pending());
    update(
        &mut state,
        Intent::Observe {
            active_tab: 8,
            counted_bar_sides: vec![],
            alert_failure: None,
        },
    );
    assert!(state.sound_picker_pending());
    assert!(!state.is_open());
    update(
        &mut state,
        Intent::Open {
            tab: 8,
            side: PaneSide::Flow,
            drawing: DrawingId(2),
            form: Box::new(StoredPreset::starting_point(quantick_engine::Side::Buy)),
        },
    );
    assert!(
        state.sound_picker_pending(),
        "opening does not erase the staged picker"
    );
    update(&mut state, Intent::AlarmSection);
    assert!(!state.sound_picker_pending());
}
#[test]
fn availability_uses_popup_side_and_preserves_preexisting_share() {
    let mut state = opened();
    update(
        &mut state,
        Intent::Observe {
            active_tab: 7,
            counted_bar_sides: vec![PaneSide::Flow],
            alert_failure: None,
        },
    );
    update(&mut state, Intent::Edit(Field::AlarmWhen(true)));
    update(
        &mut state,
        Intent::Observe {
            active_tab: 7,
            counted_bar_sides: vec![],
            alert_failure: Some("speaker refused".into()),
        },
    );
    assert!(!state.shares_available);
    assert_eq!(state.popup().unwrap().form.alarm_when, "share");
    assert_eq!(state.alert_failure.as_deref(), Some("speaker refused"));
    update(&mut state, Intent::Edit(Field::AlarmWhen(false)));
    update(&mut state, Intent::Edit(Field::AlarmWhen(true)));
    assert_eq!(state.popup().unwrap().form.alarm_when, "on_close");
}
#[test]
fn alarm_off_normalizes_only_and_test_preserves_current_sound_and_cut_order() {
    let mut state = opened();
    update(&mut state, Intent::Edit(Field::Alarm(true)));
    update(&mut state, Intent::Edit(Field::AlarmOnly(true)));
    update(&mut state, Intent::Edit(Field::AlarmCut(true)));
    let current = AlertSound::default();
    let test = update(&mut state, Intent::TestSound { current });
    assert_eq!(
        test.test_alert,
        Some(Cue::new(
            current,
            Some(quantick_strategy::presets::DEFAULT_ALARM_PLAY_SECS)
        ))
    );
    update(&mut state, Intent::Edit(Field::AlarmSeconds(9)));
    assert!(state.is_open());
    update(&mut state, Intent::Edit(Field::Alarm(false)));
    assert!(!state.popup().unwrap().form.alarm_only);
}

#[test]
fn bank_receipt_is_terminal_and_keeps_edits_and_refusal() {
    let mut state = opened();
    update(&mut state, Intent::SettleArm(Err("keep editing".into())));
    update(&mut state, Intent::SaveName("saved".into()));
    let effect = update(&mut state, Intent::SavePreset);
    let Some(BankEffect::Save { name, form }) = effect.bank else {
        panic!("save request");
    };
    let receipt = update(&mut state, Intent::BankSnapshot(vec![(name, *form)]));
    assert_eq!(
        receipt,
        Effects::default(),
        "a bank receipt never requests IO again"
    );
    assert_eq!(
        state.popup().unwrap().preset_choice.as_deref(),
        Some("saved")
    );
    assert_eq!(
        state.popup().unwrap().error.as_deref(),
        Some("keep editing")
    );
    update(
        &mut state,
        Intent::Edit(Field::Text(TextField::Quantity, "25".into())),
    );
    update(&mut state, Intent::SelectPreset("saved".into()));
    assert_ne!(quantity(&state), "25");
    assert_eq!(
        state.popup().unwrap().error.as_deref(),
        Some("keep editing")
    );
}
