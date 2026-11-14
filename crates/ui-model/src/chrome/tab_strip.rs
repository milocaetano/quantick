//! Cached tab identities and intent validation. The arrangement remains authoritative.
use quantick_workspace::arrangement::strip::{StripSurface, TabFeed};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAction {
    Activate(usize),
    Close(usize),
    New,
    Home,
}
pub use TabAction as Action;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    pub id: u64,
    pub label: String,
    pub feed: TabFeed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub chips: Vec<Chip>,
    pub active: usize,
    pub surface: StripSurface,
    pub allow_close_last: bool,
}
#[derive(Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
}
pub enum Intent {
    Refresh(Snapshot),
    Action(Action),
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects(pub Option<Action>);
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Refresh(snapshot) => {
                if state.snapshot.as_ref() != Some(&snapshot) {
                    state.snapshot = Some(snapshot);
                }
                Effects::default()
            }
            Intent::Action(action) => Effects(
                state
                    .snapshot
                    .as_ref()
                    .filter(|s| match action {
                        Action::Activate(index) => index < s.chips.len(),
                        Action::Close(index) => {
                            index < s.chips.len() && (s.chips.len() > 1 || s.allow_close_last)
                        }
                        Action::Home | Action::New => true,
                    })
                    .map(|_| action),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;
    #[test]
    fn tab_actions_reject_stale_indices_and_last_tab_close() {
        let mut state = State::default();
        let snapshot = Snapshot {
            chips: vec![Chip {
                id: 7,
                label: "BTC".into(),
                feed: TabFeed::Live,
            }],
            active: 0,
            surface: StripSurface::default(),
            allow_close_last: false,
        };
        Model::update(&mut state, Intent::Refresh(snapshot.clone()));
        for action in [Action::Activate(1), Action::Close(1), Action::Close(0)] {
            assert_eq!(
                Model::update(&mut state, Intent::Action(action)),
                Effects(None)
            );
        }
        for action in [Action::Activate(0), Action::Home, Action::New] {
            assert_eq!(
                Model::update(&mut state, Intent::Action(action)),
                Effects(Some(action))
            );
        }
        Model::update(
            &mut state,
            Intent::Refresh(Snapshot {
                allow_close_last: true,
                ..snapshot
            }),
        );
        assert_eq!(
            Model::update(&mut state, Intent::Action(Action::Close(0))),
            Effects(Some(Action::Close(0)))
        );
    }
}
