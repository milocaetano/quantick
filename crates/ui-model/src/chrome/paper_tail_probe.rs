//! Independent registered Paper tail fixture: one request per explicit arm.
#[derive(Default)]
pub struct State {
    pub armed: Option<(i64, i64, bool)>,
    pub visits: usize,
    pub report_open: Option<bool>,
}
pub enum Intent {
    Arm {
        opened: i64,
        closed: i64,
        mixed: bool,
    },
    Visit(bool),
}
#[derive(Default, Debug, PartialEq)]
pub struct Effects {
    pub navigate: Option<(i64, i64)>,
    pub mixed: bool,
}
pub struct Model;
impl crate::Model for Model {
    type State = State;
    type Intent = Intent;
    type Effects = Effects;
    fn update(state: &mut State, intent: Intent) -> Effects {
        match intent {
            Intent::Arm {
                opened,
                closed,
                mixed,
            } => state.armed = Some((opened, closed, mixed)),
            Intent::Visit(open) => {
                if let Some((opened, closed, mixed)) = state.armed.take() {
                    state.visits += 1;
                    state.report_open = Some(open);
                    return Effects {
                        navigate: Some((opened, closed)),
                        mixed,
                    };
                }
            }
        }
        Effects::default()
    }
}
