//! Native title-control availability and transitions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub maximized: bool,
    pub minimized: bool,
    pub agent_on: bool,
    pub menu_open: bool,
    pub menu_hover: String,
}
#[derive(Default)]
pub struct State {
    pub snapshot: Option<Snapshot>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Minimize,
    Maximize,
    Restore,
    Close,
    StartDrag,
    ToggleMaximize,
    Menu,
    Agent,
}
pub enum Intent {
    Refresh(Snapshot),
    Actions(Vec<Action>),
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects(pub Vec<Action>);
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
            Intent::Actions(actions) => Effects(
                actions
                    .into_iter()
                    .filter_map(|action| {
                        let snapshot = state.snapshot.as_ref()?;
                        match action {
                            Action::Agent if !snapshot.agent_on => None,
                            Action::ToggleMaximize => Some(if snapshot.maximized {
                                Action::Restore
                            } else {
                                Action::Maximize
                            }),
                            action => Some(action),
                        }
                    })
                    .collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;
    #[test]
    fn title_buttons_resolve_state_and_hide_disabled_agent_access() {
        let mut state = State::default();
        Model::update(&mut state, Intent::Refresh(Snapshot::default()));
        assert_eq!(
            Model::update(
                &mut state,
                Intent::Actions(vec![Action::ToggleMaximize, Action::Agent, Action::Menu])
            ),
            Effects(vec![Action::Maximize, Action::Menu])
        );
        Model::update(
            &mut state,
            Intent::Refresh(Snapshot {
                maximized: true,
                agent_on: true,
                ..Snapshot::default()
            }),
        );
        assert_eq!(
            Model::update(
                &mut state,
                Intent::Actions(vec![Action::ToggleMaximize, Action::Agent])
            ),
            Effects(vec![Action::Restore, Action::Agent])
        );
    }
}
