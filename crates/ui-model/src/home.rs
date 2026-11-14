//! Home page actions. Market identity and navigation stay with workspace/chrome.

#[derive(Default, Debug, PartialEq, Eq)]
pub struct State;

pub enum Intent {
    OpenMarket,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Effects {
    OpenMarket,
}

pub struct Model;

impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;

    fn update(_: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::OpenMarket => Effects::OpenMarket,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;

    #[test]
    fn migration_preserves_state_and_effects() {
        let mut state = State;
        // Opening the picker does not leave Home or mutate market identity.
        // Closing it and asking again remains available without a reset.
        for _ in 0..2 {
            assert_eq!(
                Model::update(&mut state, Intent::OpenMarket),
                Effects::OpenMarket
            );
            assert_eq!(state, State);
        }
    }
}
