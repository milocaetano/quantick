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
pub(crate) fn standard_actions() -> Result<ActionRegistry, RegistryError> {
    actions_for_strategy_describe(false)
}

pub(super) fn actions_for_strategy_describe(
    available: bool,
) -> Result<ActionRegistry, RegistryError> {
    let mut registry = ActionRegistry::new();
    quantick_control_handlers::alerts::register(&mut registry)?;
    quantick_control_handlers::attention::register(&mut registry)?;
    quantick_control_handlers::appearance::register_actions(&mut registry)?;
    quantick_control_handlers::annotate::register(&mut registry)?;
    quantick_control_handlers::drawing_text::register(&mut registry)?;
    quantick_control_handlers::notify::register(&mut registry)?;
    quantick_control_handlers::layout::register(&mut registry)?;
    quantick_control_handlers::canvas::register(&mut registry)?;
    quantick_control_handlers::layers::register(&mut registry)?;
    quantick_control_handlers::price_axis::register(&mut registry)?;
    quantick_control_handlers::home_tab::register(&mut registry)?;
    quantick_control_handlers::widgets::register(&mut registry)?;
    quantick_control_handlers::market_palette::register(&mut registry)?;
    quantick_control_handlers::desk::register(&mut registry)?;
    quantick_control_handlers::workspace_book::register(&mut registry)?;
    quantick_control_handlers::native_window::register(&mut registry)?;
    quantick_control_handlers::market_windows::register(&mut registry)?;
    quantick_control_handlers::time_axis::register(&mut registry)?;
    quantick_control_handlers::opening_scale::register(&mut registry)?;
    quantick_control_handlers::price_tag_progress::register(&mut registry)?;
    quantick_control_handlers::bubble_preset::register_tick(&mut registry)?;
    quantick_control_handlers::bubble_preset::register(&mut registry)?;
    quantick_control_handlers::bubble_save::register(&mut registry)?;
    quantick_control_handlers::bubble_scale::register(&mut registry)?;
    quantick_control_handlers::tape_fold::register(&mut registry)?;
    quantick_control_handlers::orderflow_settings::register(&mut registry)?;
    quantick_control_handlers::tape_view_set::register(&mut registry)?;
    quantick_control_handlers::recovery::register(&mut registry)?;
    quantick_control_handlers::feed_source::register(&mut registry)?;
    quantick_control_handlers::contract_roll::register(&mut registry)?;
    quantick_control_handlers::history::register(&mut registry)?;
    quantick_control_handlers::journal_sessions::register(&mut registry)?;
    quantick_control_handlers::session_reports::register(&mut registry)?;
    quantick_control_handlers::pnl_display::register(&mut registry)?;
    quantick_control_handlers::deal_recording::register(&mut registry)?;
    quantick_control_handlers::drawing_magnet::register(&mut registry)?;
    quantick_control_handlers::chart_settings::register(&mut registry)?;
    quantick_control_handlers::indicator_guide::register(&mut registry)?;
    quantick_control_handlers::indicator_picker::register(&mut registry)?;
    quantick_control_handlers::native_indicator::register(&mut registry)?;
    quantick_control_handlers::chart_context::register(&mut registry)?;
    quantick_control_handlers::script_alarms::register(&mut registry)?;
    quantick_control_handlers::script_strategy::register(&mut registry)?;
    quantick_control_handlers::script::register(&mut registry)?;
    quantick_control_handlers::script_editor::register(&mut registry)?;
    quantick_control_handlers::trade::register(&mut registry)?;
    super::strategy_describe::register(&mut registry, available)?;
    Ok(registry)
}

#[cfg(test)]
mod tests;
