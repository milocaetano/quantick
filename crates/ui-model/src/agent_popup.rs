//! Assistant notification presentation state; notification policy stays in control-schema.
use quantick_control_schema::notify::AgentPopup;

#[derive(Default, Debug, PartialEq, Eq)]
pub struct State {
    pending: Option<AgentPopup>,
}
impl State {
    pub fn pending(&self) -> Option<&AgentPopup> {
        self.pending.as_ref()
    }
}

#[derive(Debug)]
pub enum Intent {
    Show(AgentPopup),
    Dismiss,
    WindowClosed,
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = ();
    fn update(state: &mut State, intent: Intent) {
        match intent {
            Intent::Show(popup) => state.pending = Some(popup),
            Intent::Dismiss | Intent::WindowClosed => state.pending = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;

    fn popup(message: &str) -> AgentPopup {
        AgentPopup {
            title: "Assistant".to_string(),
            message: message.to_string(),
            author: "tester".to_string(),
        }
    }

    /// The surface starts empty, so an application that never hears from the
    /// assistant paints no window.
    #[test]
    fn a_new_surface_has_nothing_to_show() {
        assert!(State::default().pending().is_none());
    }

    /// Last message wins, rather than queueing behind one the trader has not
    /// dismissed.
    #[test]
    fn a_second_popup_replaces_the_first() {
        let mut surface = State::default();
        Model::update(&mut surface, Intent::Show(popup("first")));
        Model::update(&mut surface, Intent::Show(popup("second")));
        assert_eq!(
            surface.pending().map(|p| p.message.as_str()),
            Some("second")
        );
    }
    #[test]
    fn migration_preserves_state_and_effects() {
        let mut state = State::default();
        for close in [Intent::Dismiss, Intent::WindowClosed] {
            Model::update(&mut state, Intent::Show(popup("first")));
            Model::update(&mut state, Intent::Show(popup("latest")));
            assert_eq!(state.pending().unwrap().message, "latest");
            assert_eq!(
                (Model::update(&mut state, close), state.pending()),
                ((), None)
            );
            Model::update(&mut state, Intent::Dismiss);
            assert!(state.pending().is_none());
        }
    }
}
