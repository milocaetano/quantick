//! Intent-driven UI state. Domain decisions remain in their owning crates.
//! Time, snapshots and completion results arrive as intents; no clock is read.
mod catalog;
#[cfg(test)]
mod scratch;

/// A deterministic transition over cached presentation state.
pub trait Model {
    type State;
    type Intent;
    type Effects;
    fn update(state: &mut Self::State, intent: Self::Intent) -> Self::Effects;
}

/// A local host operation returns residual effects and at most one terminal receipt.
pub type LocalEffects<'a, M> =
    dyn FnMut(<M as Model>::Effects) -> (<M as Model>::Effects, Option<<M as Model>::Intent>) + 'a;

/// A bounded input/paint transaction over the adapter's actual state.
/// Views can emit an intent and borrow the updated projection before painting
/// dependent widgets. Effects are delivered immediately and exactly once.
pub struct UpdateSession<'a, M: Model> {
    state: &'a mut M::State,
    sink: &'a mut dyn FnMut(M::Effects),
    local: Option<&'a mut LocalEffects<'a, M>>,
}
impl<M: Model> UpdateSession<'_, M> {
    /// Reborrow a bounded transaction for a view helper without exposing mutable state.
    pub fn reborrow(&mut self) -> UpdateSession<'_, M> {
        UpdateSession {
            state: self.state,
            sink: self.sink,
            local: self
                .local
                .as_mut()
                .map(|handler| &mut **handler as &mut LocalEffects<'_, M>),
        }
    }
    pub fn state(&self) -> &M::State {
        self.state
    }
    pub fn update(&mut self, intent: M::Intent) {
        let effects = M::update(self.state, intent);
        let (effects, receipt) = match self.local.as_mut() {
            Some(handler) => handler(effects),
            None => (effects, None),
        };
        (self.sink)(effects);
        // A receipt updates the projection synchronously, but does not re-enter
        // local execution. It must be terminal rather than another IO request.
        if let Some(receipt) = receipt {
            (self.sink)(M::update(self.state, receipt));
        }
    }
    /// Install a local host handler for this paint transaction only. Nested
    /// installation is unsupported; ordinary reborrow preserves the handler.
    /// A returned receipt is terminal: its effects use the original sink and
    /// never re-enter this handler. The receipt settles before update returns.
    pub fn with_local_effects<R>(
        &mut self,
        handler: &mut LocalEffects<'_, M>,
        paint: impl FnOnce(&mut UpdateSession<'_, M>) -> R,
    ) -> R {
        assert!(
            self.local.is_none(),
            "local effect handlers cannot be nested"
        );
        paint(&mut UpdateSession {
            state: self.state,
            sink: self.sink,
            local: Some(handler),
        })
    }
}

/// Scoped model storage shared by every host. `V` contains host-only mechanics;
/// this crate never paints it or gives it authority over a domain owner.
pub struct Adapter<M: Model, V, E> {
    pub model_state: M::State,
    pub view: V,
    effects: fn(M::Effects) -> E,
    slots: &'static [Slot],
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
            slots: &[],
            scopes: Default::default(),
        }
    }
    /// Bind the existing frame locations declared by this adapter's catalog row.
    pub fn in_slots(mut self, slots: &'static [Slot]) -> Self {
        self.slots = slots;
        self
    }
    pub fn supports_pass(&self, slot: Slot) -> bool {
        self.slots.contains(&slot)
    }
    /// Borrow host-specific facts for one declared pass without storing or
    /// cloning them. Every pass updates the same state and delivers each effect once.
    pub fn with_pass<R>(
        &mut self,
        slot: Slot,
        sink: impl FnMut(E),
        paint: impl FnOnce(&mut V, &mut UpdateSession<'_, M>) -> R,
    ) -> Option<R> {
        self.supports_pass(slot)
            .then(|| self.with_updates(sink, paint))
    }
    pub fn update(&mut self, intent: M::Intent) -> E {
        (self.effects)(M::update(&mut self.model_state, intent))
    }
    pub fn with_updates<R>(
        &mut self,
        mut sink: impl FnMut(E),
        paint: impl FnOnce(&mut V, &mut UpdateSession<'_, M>) -> R,
    ) -> R {
        let convert = self.effects;
        let mut forward = |effects| sink(convert(effects));
        paint(
            &mut self.view,
            &mut UpdateSession {
                state: &mut self.model_state,
                sink: &mut forward,
                local: None,
            },
        )
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
    pub fn with_scoped_updates<R>(
        &mut self,
        scope: u64,
        mut sink: impl FnMut(E),
        paint: impl FnOnce(&mut V, &mut UpdateSession<'_, M>) -> R,
    ) -> R {
        let convert = self.effects;
        let (state, view) = self.scoped(scope);
        let mut forward = |effects| sink(convert(effects));
        paint(
            view,
            &mut UpdateSession {
                state,
                sink: &mut forward,
                local: None,
            },
        )
    }
    pub fn update_scoped(&mut self, scope: u64, intent: M::Intent) -> E {
        let effects = self.effects;
        let (state, _) = self.scoped(scope);
        effects(M::update(state, intent))
    }
}

/// Existing frame locations available to catalog entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Leading subpass of the existing overlay stage; takes precedence over Overlay.
    LeadingOverlay,
    Overlay,
    HomeCanvas,
    PaneHeader,
    TitleBar,
    Toolbar,
    MenuBar,
    TabStrip,
    StatusLine,
    PinnedInspector,
    DrawingChrome,
    EarlyEdgePanels,
    EdgePanels,
}

macro_rules! models {
    (modules { $($modules:tt)* }
     models { $( [$($model_attr:meta),*] [$($view_attr:meta),*]
        $field:ident: $tag:ident => $model:ident = $model_file:literal, $view:ident = $view_file:literal,
        $effects:path, $slot:ident $(| $additional_slot:ident)*, $dispatch:ident; )* }) => {
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

#[cfg(test)]
mod local_effects_tests;
