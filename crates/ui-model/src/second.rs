//! Catalog docking fixture; available only to tests and test consumers.
pub struct Model;
impl crate::Model for Model {
    type State = i64;
    type Intent = (i64, u64);
    type Effects = (i64, u64);
    fn update(state: &mut i64, (delta, now): Self::Intent) -> Self::Effects {
        *state += delta * 2;
        (*state, now)
    }
}
