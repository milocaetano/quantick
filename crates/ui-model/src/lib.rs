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

/// Scoped model storage shared by every host. `V` contains host-only mechanics;
/// this crate never paints it or gives it authority over a domain owner.
pub struct Adapter<M: Model, V, E> {
    pub model_state: M::State,
    pub view: V,
    effects: fn(M::Effects) -> E,
    scopes: std::collections::BTreeMap<u64, (M::State, V)>,
}
impl<M: Model, V, E> Adapter<M, V, E> {
    pub fn new(view: V, effects: fn(M::Effects) -> E) -> Self
    where
        M::State: Default,
    {
        Self {
            model_state: Default::default(),
            view,
            effects,
            scopes: Default::default(),
        }
    }
    pub fn update(&mut self, intent: M::Intent) -> E {
        (self.effects)(M::update(&mut self.model_state, intent))
    }
    pub fn retain_scopes(&mut self, keep: impl Fn(u64) -> bool) {
        self.scopes.retain(|scope, _| keep(*scope));
    }
}
impl<M: Model, V: Default, E> Adapter<M, V, E>
where
    M::State: Default,
{
    pub fn scoped(&mut self, scope: u64) -> (&mut M::State, &mut V) {
        let (state, view) = self.scopes.entry(scope).or_default();
        (state, view)
    }
    pub fn update_scoped(&mut self, scope: u64, intent: M::Intent) -> E {
        let effects = self.effects;
        let (state, _) = self.scoped(scope);
        effects(M::update(state, intent))
    }
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
    MenuBar,
    TabStrip,
    StatusLine,
}

macro_rules! models {
    (legacy { $($legacy:tt)* } modules { $($modules:tt)* }
     models { $( [$($model_attr:meta),*] [$($view_attr:meta),*]
        $field:ident: $tag:ident => $model:ident = $model_file:literal, $view:ident = $view_file:literal,
        $effects:path, $slot:ident; )* }) => {
        $($(#[$model_attr])* #[path = $model_file] pub mod $model;)*
        /// Typed host effects can grow when a new catalog row docks.
        #[non_exhaustive]
        #[derive(Debug, PartialEq)]
        pub enum HostEffect {
            $($(#[$model_attr])* $tag(<$model::Model as Model>::Effects),)*
        }


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

#[cfg(test)]
pub mod chrome;
