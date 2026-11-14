//! Selection of the watermark target; the host owns preview authority and geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    None,
    Preview,
    Focused,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct State {
    pub forced: bool,
    pub preview_available: bool,
    pub focused_available: bool,
    pub target: Target,
}

pub enum Intent {
    Force(bool),
    Refresh {
        preview_available: bool,
        focused_available: bool,
    },
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = ();

    fn update(state: &mut State, intent: Intent) {
        match intent {
            Intent::Force(forced) => state.forced = forced,
            Intent::Refresh {
                preview_available,
                focused_available,
            } => {
                state.preview_available = preview_available;
                state.focused_available = focused_available;
            }
        }
        state.target = if state.preview_available {
            Target::Preview
        } else if state.forced && state.focused_available {
            Target::Focused
        } else {
            Target::None
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Model as _;

    #[test]
    fn migration_preserves_state_and_effects() {
        for forced in [false, true] {
            for (preview_available, focused_available, expected) in [
                (false, false, Target::None),
                (
                    false,
                    true,
                    if forced {
                        Target::Focused
                    } else {
                        Target::None
                    },
                ),
                (true, false, Target::Preview),
                (true, true, Target::Preview),
            ] {
                let mut state = State::default();
                Model::update(&mut state, Intent::Force(forced));
                let refresh = || Intent::Refresh {
                    preview_available,
                    focused_available,
                };
                Model::update(&mut state, refresh());
                assert_eq!(state.target, expected);
                Model::update(&mut state, refresh());
                assert_eq!(state.target, expected);
                Model::update(&mut state, Intent::Force(false));
                assert!(!state.forced);
                assert_eq!(
                    state.target,
                    if preview_available {
                        Target::Preview
                    } else {
                        Target::None
                    }
                );
                Model::update(
                    &mut state,
                    Intent::Refresh {
                        preview_available: false,
                        focused_available: false,
                    },
                );
                assert_eq!(state.target, Target::None);
            }
        }
    }
}
