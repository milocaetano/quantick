//! Intent-driven UI state. Domain decisions remain in their owning crates.
//! Time, snapshots and completion results arrive as intents; no clock is read.
mod catalog;

/// A deterministic transition over cached presentation state.
pub trait Model {
    type State;
    type Intent;
    type Effects;
    fn update(state: &mut Self::State, intent: Self::Intent) -> Self::Effects;
}

/// Compatibility transition while a view still owns its legacy state.
pub struct Legacy<E>(std::marker::PhantomData<E>);
impl<E> Model for Legacy<E> {
    type State = ();
    type Intent = E;
    type Effects = E;
    fn update(_: &mut (), intent: E) -> E {
        intent
    }
}

/// Existing frame locations available to catalog entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Overlay,
    PaneHeader,
    TitleBar,
    Toolbar,
    StatusLine,
}

macro_rules! models {
    (legacy { $($legacy:tt)* } models { $( $field:ident: $model:ident, $view:ident, $effects:path, $slot:ident; )* }
     fixtures { $( $fixture:ident: $fixture_slot:ident; )* }) => {
        $(pub mod $model;)*
        $(#[cfg(any(test, feature = "test-support"))] pub mod $fixture;)*
    };
}
surface_catalog!(models);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registered_models_update_deterministically() {
        fn run<M: Model<State = i64, Intent = (i64, u64), Effects = (i64, u64)>>()
        -> (i64, Vec<(i64, u64)>) {
            let mut state = 0;
            let effects = [(3, 10), (-1, 20), (7, 20)]
                .into_iter()
                .map(|intent| M::update(&mut state, intent))
                .collect();
            (state, effects)
        }
        assert_eq!(run::<first::Model>(), (9, vec![(3, 10), (2, 20), (9, 20)]));
        assert_eq!(
            run::<second::Model>(),
            (18, vec![(6, 10), (4, 20), (18, 20)])
        );
        assert_eq!(run::<first::Model>(), run::<first::Model>());
        assert_eq!(run::<second::Model>(), run::<second::Model>());
    }
}
