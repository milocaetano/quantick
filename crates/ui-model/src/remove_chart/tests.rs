use super::*;
use crate::Model as _;
use quantick_workspace::canvas::preset;

fn snapshot() -> Snapshot {
    let live = CanvasState::from_preset(preset("time+flow").unwrap());
    Snapshot {
        saved: Some(live.clone()),
        live,
        name: "Desk".to_owned(),
        autosave: false,
    }
}
fn step(
    state: &mut State,
    snapshot: Snapshot,
    picked: Option<PaneMenuItem>,
    escape: bool,
) -> Effects {
    Model::update(
        state,
        Intent::MenuStep {
            picked: picked.map(|item| (0, item)),
            escape,
            snapshot: Box::new(snapshot),
        },
    )
}
fn answer(state: &mut State, clicked: Option<RemoveAnswer>, escaped: bool) -> Effects {
    Model::update(state, Intent::Answer { clicked, escaped })
}
fn open(state: &mut State) {
    assert_eq!(
        step(state, snapshot(), Some(PaneMenuItem::Remove), false),
        Effects::Act(None)
    );
    assert_eq!(state.pending(), Some(0));
    assert!(state.prompt().is_some());
}

#[test]
fn migration_preserves_state_and_effects() {
    let mut state = State::default();
    open(&mut state);
    let title = state.prompt().unwrap().title.as_ptr();
    assert_eq!(
        step(&mut state, snapshot(), None, false),
        Effects::Act(None)
    );
    assert_eq!(
        state.prompt().unwrap().title.as_ptr(),
        title,
        "unchanged prompt stays cached"
    );
    assert_eq!(answer(&mut state, None, false), Effects::None);
    assert_eq!(state.pending(), Some(0));
    for choice in RemoveAnswer::ALL {
        for escaped in [false, true] {
            open(&mut state);
            assert_eq!(
                answer(&mut state, Some(choice), escaped),
                Effects::Act(canvas::confirmed(choice, 0))
            );
            assert_eq!(state.pending(), None);
            assert_eq!(state.prompt(), None);
            assert_eq!(answer(&mut state, Some(choice), escaped), Effects::None);
        }
    }
    open(&mut state);
    assert_eq!(answer(&mut state, None, true), Effects::Act(None));
    assert_eq!(state.pending(), None);
    let mut unsaved = snapshot();
    unsaved.saved = None;
    assert_eq!(
        step(&mut state, unsaved, Some(PaneMenuItem::Remove), false),
        Effects::Act(Some(CanvasAct::Remove {
            slot: 0,
            confirm: true
        }))
    );
    assert_eq!(state.pending(), None);
    open(&mut state);
}

#[test]
fn pending_invalidates_without_leaking_escape_to_maximize() {
    let mut state = State::default();
    open(&mut state);
    let mut changed = snapshot();
    changed.live.maximize(1).unwrap();
    assert_eq!(
        step(&mut state, changed.clone(), None, true),
        Effects::Act(None)
    );
    assert_eq!(state.pending(), Some(0));
    changed.live.remove(0).unwrap();
    assert_eq!(
        step(&mut state, changed.clone(), None, true),
        Effects::Act(None)
    );
    assert_eq!(state.pending(), None);
    assert_eq!(state.prompt(), None);
    assert_eq!(
        step(&mut state, changed, None, true),
        Effects::Act(Some(CanvasAct::Restore { slot: 1 }))
    );
    open(&mut state);
    let mut no_save = snapshot();
    no_save.saved = None;
    assert_eq!(step(&mut state, no_save, None, false), Effects::Act(None));
    assert_eq!(state.pending(), None);
}

#[test]
fn cancel_overrides_an_earlier_menu_act() {
    let mut state = State::default();
    open(&mut state);
    assert_eq!(
        step(&mut state, snapshot(), Some(PaneMenuItem::Minimize), false),
        Effects::Act(Some(CanvasAct::Minimize { slot: 0 }))
    );
    assert_eq!(
        answer(&mut state, Some(RemoveAnswer::Cancel), false),
        Effects::Act(None)
    );
    assert_eq!(state.pending(), None);
}

#[test]
fn observation_captures_only_input_or_changed_open_context() {
    let mut state = State::default();
    let facts = snapshot();
    let observe = |state: &State, picked, name| {
        state.observe(
            picked,
            false,
            &facts.live,
            facts.saved.as_ref(),
            (name, false),
        )
    };
    assert!(
        observe(&state, None, "Desk").is_none(),
        "closed idle has no snapshot intent"
    );
    let opening = observe(&state, Some((0, PaneMenuItem::Remove)), "Desk").unwrap();
    Model::update(&mut state, opening);
    assert!(
        observe(&state, None, "Desk").is_none(),
        "unchanged pending reuses its projection"
    );
    let renamed = observe(&state, None, "Renamed").unwrap();
    Model::update(&mut state, renamed);
    assert!(state.prompt().unwrap().title.contains("Renamed"));
    answer(&mut state, Some(RemoveAnswer::Cancel), false);
    assert!(observe(&state, None, "Another desk").is_none());
}
