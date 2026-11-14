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
    if let Some(item) = picked { Model::update(state, Intent::Pick(0, item)); }
    Model::update(
        state,
        Intent::MenuStep {
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
    let observe = |state: &State, name| {
        state.observe(
            false,
            &facts.live,
            facts.saved.as_ref(),
            (name, false),
        )
    };
    assert!(
        observe(&state, "Desk").is_none(),
        "closed idle has no snapshot intent"
    );
    Model::update(&mut state, Intent::Pick(0, PaneMenuItem::Remove));
    let opening = observe(&state, "Desk").unwrap();
    Model::update(&mut state, opening);
    assert!(
        observe(&state, "Desk").is_none(),
        "unchanged pending reuses its projection"
    );
    let renamed = observe(&state, "Renamed").unwrap();
    Model::update(&mut state, renamed);
    assert!(state.prompt().unwrap().title.contains("Renamed"));
    answer(&mut state, Some(RemoveAnswer::Cancel), false);
    assert!(observe(&state, "Another desk").is_none());
}


#[test]
fn pane_menu_pick_is_consumed_once_in_its_tab() {
    let mut state = State::default();
    Model::update(&mut state, Intent::ToggleMenu(0));
    assert_eq!(state.menu_open(), Some(0));
    Model::update(&mut state, Intent::ToggleMenu(0));
    assert_eq!(state.menu_open(), None);
    Model::update(&mut state, Intent::ToggleMenu(0));
    Model::update(&mut state, Intent::ToggleMenu(1));
    assert_eq!(state.menu_open(), Some(1), "only one slot owns the menu");
    Model::update(&mut state, Intent::DismissMenu);
    assert_eq!(state.menu_open(), None);
    Model::update(&mut state, Intent::Pick(0, PaneMenuItem::Minimize));
    assert!(state.has_pick());
    assert_eq!(step(&mut state, snapshot(), None, false), Effects::Act(Some(CanvasAct::Minimize { slot: 0 })));
    assert!(!state.has_pick());
    assert_eq!(step(&mut state, snapshot(), None, false), Effects::Act(None));
    Model::update(&mut state, Intent::Pick(0, PaneMenuItem::Remove));
    assert_eq!(step(&mut state, snapshot(), None, false), Effects::Act(None));
    assert_eq!(state.pending(), Some(0));
    assert!(!state.has_pick());
    assert_eq!(answer(&mut state, Some(RemoveAnswer::Cancel), false), Effects::Act(None));
}
#[test]
fn pane_menu_scopes_preserve_live_tabs_and_prune_closed_tabs() {
    let mut adapter = crate::Adapter::<Model, (), Effects>::new((), |effects| effects);
    for (tab, slot) in [(7, 0), (8, 1)] {
        adapter.update_scoped(tab, Intent::SetMenuOpen(Some(slot)));
    }
    adapter.update_scoped(7, Intent::Pick(0, PaneMenuItem::Minimize));
    assert_eq!(adapter.scoped(8).0.menu_open(), Some(1));
    adapter.retain_scopes(|tab| tab == 8);
    assert_eq!(adapter.scoped(8).0.menu_open(), Some(1));
    assert!(!adapter.scoped(8).0.has_pick());
    assert_eq!(adapter.scoped(7).0.menu_open(), None);
    assert!(!adapter.scoped(7).0.has_pick());
}
