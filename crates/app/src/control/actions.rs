//! The action registry port: named, attributed calls that change session
//! state, and the one list of the families this build docks into it.
//!
//! Plan §2.2 and `CLAUDE.md`'s *Operable without a hand*: a capability a trader does
//! exists as a named call with an actor in its signature, not only inside a
//! click handler. The UI hotkey, the `QUANTICK_CONTROL_MARK` hook, a
//! deterministic test and an authorized agent all arrive at the same handler
//! through [`ActionRegistry`]. The gateway keeps every action here
//! unavailable to remote observer clients: an action's permissions are not in
//! the observer ceiling, so a remote invocation is refused before dispatch.

pub(crate) use quantick_control_schema::attention::*;

use quantick_control::registry::RegistryError;

use crate::app::ControlWindow;

use super::gateway::ControlAccess;

/// The registry over this application: the headless registry in
/// `quantick_control_host::actions` with the window as its host. Families
/// dock through `quantick_control_handlers::dock::ActionDock`, which the host registry implements.
pub(crate) type ActionRegistry =
    quantick_control_host::actions::ActionRegistry<ControlWindow, ControlAccess>;

/// The annotate tier's identifiers the mark docks into (contract §7).
pub(crate) use quantick_control_host::authority::{ANNOTATE_PERMISSION_ID, ANNOTATOR_PROFILE_ID};

/// The actions every instance registers. A later module adds one descriptor
/// and one handler here; nothing else opens.
#[cfg(test)]
pub(crate) fn standard_actions() -> Result<ActionRegistry, RegistryError> {
    actions_for_strategy_describe(false)
}

pub(super) fn actions_for_strategy_describe(
    available: bool,
) -> Result<ActionRegistry, RegistryError> {
    let mut registry = ActionRegistry::new();
    macro_rules! register_action_family {
        (strategy_describe) => {};
        ($action:ident) => {
            quantick_control_handlers::$action::register_actions(&mut registry)?;
        };
    }
    macro_rules! action_families {
    ($($id:literal {
            schemas: [$($schema:ident),*], actions: [$($action:ident),*],
            projections: [$($projection:ident),*], authority: [$($authority:ident),*],
            readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
            samples: [$($sample:ident),*],
        })*) => { $( $(register_action_family!($action);)* )* };
}
    quantick_control::control_families!(action_families);
    super::strategy_describe::register(&mut registry, available)?;
    Ok(registry)
}

#[cfg(test)]
mod tests;
