//! Distinct catalog fixture with cached presentation observations only.
#[derive(Default)]
pub struct State {
    pub enabled: bool,
    pub observed: Option<bool>,
}
pub enum Intent {
    Enable,
    Observe(bool),
    Clear,
}
#[derive(Debug, PartialEq)]
pub struct Effects(pub Option<(bool, &'static str)>);
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Enable => state.enabled = true,
            Intent::Clear => state.observed = None,
            Intent::Observe(value) => {
                state.observed = Some(value);
                return Effects(Some((value, "session")));
            }
        }
        Effects(None)
    }
}
