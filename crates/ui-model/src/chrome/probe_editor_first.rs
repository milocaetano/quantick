//! Test-only item docked through the production catalog.
#[derive(Default)]
pub struct State {
    pub visible: bool,
    pub value: u64,
}
pub enum Intent {
    Show(bool),
    Add(u64),
}
#[derive(Debug, Default, PartialEq)]
pub struct Effects(pub Option<u64>);
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Show(visible) => {
                state.visible = visible;
                Effects(None)
            }
            Intent::Add(value) if state.visible => {
                state.value += value;
                Effects(Some(state.value))
            }
            Intent::Add(_) => Effects(None),
        }
    }
}
