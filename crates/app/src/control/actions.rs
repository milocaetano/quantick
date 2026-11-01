//! The action registry port: named, attributed calls that change session
//! state, and the one list of the families this build docks into it.
//!
//! Plan §2.2 and `arch-review`'s second operator: a capability a trader does
//! exists as a named call with an actor in its signature, not only inside a
//! click handler. The UI hotkey, the `QUANTICK_CONTROL_MARK` hook, a
//! deterministic test and an authorized agent all arrive at the same handler
//! through [`ActionRegistry`]. The gateway keeps every action here
//! unavailable to remote observer clients: an action's permissions are not in
//! the observer ceiling, so a remote invocation is refused before dispatch.

pub(crate) use quantick_control_schema::attention::*;

use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use serde_json::Value;

use crate::app::ControlWindow;

use super::gateway::ControlAccess;

/// One action's handler over this application: it mutates the window and
/// journals through its control access.
pub(crate) type ActionHandler = fn(
    &mut ControlWindow,
    &mut ControlAccess,
    &ActorContext,
    &Value,
) -> Result<Value, ControlError>;

/// The registry over this application: the headless registry in
/// `quantick_control_host::actions` with the window as its host. Families
/// dock through `quantick_control_handlers::dock::ActionDock`, which the host registry implements.
pub(crate) type ActionRegistry =
    quantick_control_host::actions::ActionRegistry<ControlWindow, ControlAccess>;

/// The annotate tier's identifiers the mark docks into (contract §7).
pub(crate) use quantick_control_host::authority::{
    ANNOTATE_PERMISSION_ID, ANNOTATOR_PROFILE_ID, CAPABILITY_VERSION, NO_CONFIRMATION_ID,
    UI_BOUNDED_COST_ID,
};

/// The actions every instance registers. A later module adds one descriptor
/// and one handler here; nothing else opens.
pub(crate) fn standard_actions() -> Result<ActionRegistry, RegistryError> {
    let mut registry = ActionRegistry::new();
    quantick_control_handlers::attention::register(&mut registry)?;
    quantick_control_handlers::annotate::register(&mut registry)?;
    quantick_control_handlers::notify::register(&mut registry)?;
    super::layout::register(&mut registry)?;
    super::layers::register_action(&mut registry)?;
    super::price_axis::register(&mut registry)?;
    super::time_axis::register(&mut registry)?;
    quantick_control_handlers::opening_scale::register(&mut registry)?;
    quantick_control_handlers::bubble_preset::register_tick(&mut registry)?;
    quantick_control_handlers::bubble_preset::register(&mut registry)?;
    super::bubble_save::register(&mut registry)?;
    quantick_control_handlers::bubble_scale::register(&mut registry)?;
    quantick_control_handlers::tape_fold::register(&mut registry)?;
    quantick_control_handlers::tape_view_set::register(&mut registry)?;
    quantick_control_handlers::recovery::register(&mut registry)?;
    quantick_control_handlers::feed_source::register(&mut registry)?;
    quantick_control_handlers::history::register(&mut registry)?;
    quantick_control_handlers::journal_sessions::register(&mut registry)?;
    quantick_control_handlers::pnl_display::register(&mut registry)?;
    super::deal_recording::register(&mut registry)?;
    super::drawing_magnet::register(&mut registry)?;
    super::drawing_magnet::register_object(&mut registry)?;
    super::chart_settings::register(&mut registry)?;
    super::indicator_guide::register(&mut registry)?;
    super::indicator_picker::register(&mut registry)?;
    super::script::register(&mut registry)?;
    quantick_control_handlers::trade::register(&mut registry)?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_control_host::authority::{ANNOTATE_EFFECT_ID, TRADE_EFFECT_ID};

    /// The families that moved to `quantick_control_handlers` are still docked
    /// here, behind the effects and permissions the contract names.
    #[test]
    fn the_standard_actions_still_carry_the_moved_families() {
        let registry = standard_actions().unwrap();
        let mark = registry
            .lookup(MARK_CAPABILITY_ID, CAPABILITY_VERSION)
            .unwrap();
        assert_eq!(mark.descriptor.effect.as_str(), ANNOTATE_EFFECT_ID);
        assert!(
            mark.descriptor
                .required_permissions
                .iter()
                .any(|permission| {
                    permission.as_str()
                        == quantick_control_host::authority::ANNOTATE_ATTENTION_PERMISSION_ID
                })
        );
        for id in [
            quantick_control_handlers::trade::PLACE_CAPABILITY_ID,
            quantick_control_handlers::trade::BRACKET_CAPABILITY_ID,
            quantick_control_handlers::trade::CANCEL_CAPABILITY_ID,
        ] {
            let action = registry
                .lookup(id, CAPABILITY_VERSION)
                .unwrap_or_else(|| panic!("{id} is registered"));
            assert_eq!(action.descriptor.effect.as_str(), TRADE_EFFECT_ID);
            assert!(!action.descriptor.read_only);
            assert!(
                action
                    .descriptor
                    .required_permissions
                    .iter()
                    .any(|permission| {
                        permission.as_str() == quantick_control_handlers::trade::TRADE_PERMISSION_ID
                    })
            );
        }
        let cancel = registry
            .lookup(quantick_control_handlers::trade::CANCEL_CAPABILITY_ID, 1)
            .unwrap();
        assert!(!cancel.descriptor.destructive && cancel.descriptor.risk_reducing);
    }
}
