//! State for the strategy conversation over the focused chart.
pub use crate::strategy_describe_contract::*;

#[derive(Default)]
pub struct State {
    pub status: Status,
}

pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        update(state, intent)
    }
}
pub fn update(_state: &mut State, _intent: Intent) -> Effects {
    Effects::None
}
