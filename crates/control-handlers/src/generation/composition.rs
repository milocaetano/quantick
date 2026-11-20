//! Descriptor-only composition. No host, fake ports, or runtime handlers are constructed.
use quantick_control::registry::RegistryError;
use quantick_control_host::{authority, contract::CapabilityContract};

pub(super) fn contract() -> Result<CapabilityContract<()>, RegistryError> {
    let mut builder = authority::builder()?;
    let mut scopes = Vec::new();
    builder.register_module(super::system::module_descriptor())?;
    scopes.extend(super::system::scope_descriptors());
    builder.register_module(crate::appearance::module_descriptor())?;
    scopes.extend(crate::appearance::scope_descriptors());
    builder.register_module(super::workspace::module_descriptor())?;
    scopes.extend(super::workspace::scope_descriptors());
    scopes.extend(crate::market_windows::scope_descriptors());
    builder.register_module(super::feed::module_descriptor())?;
    scopes.extend(super::feed::scope_descriptors());
    builder.register_module(crate::chart::module_descriptor())?;
    scopes.extend(crate::chart::scope_descriptors());
    builder.register_module(crate::health::module_descriptor())?;
    scopes.extend(crate::health::scope_descriptors());
    builder.register_module(crate::analysis::module_descriptor())?;
    scopes.extend(crate::analysis::scope_descriptors());
    builder.register_module(super::interaction::module_descriptor())?;
    scopes.extend(super::interaction::scope_descriptors());
    builder.register_module(crate::orderflow::module_descriptor())?;
    scopes.extend(crate::orderflow::scope_descriptors());
    builder.register_module(crate::session::module_descriptor())?;
    scopes.extend(crate::session::scope_descriptors());
    builder.register_module(super::scene::module_descriptor())?;
    scopes.extend(super::scene::scope_descriptors());
    builder.register_module(crate::layers::module_descriptor())?;
    scopes.extend(crate::layers::scope_descriptors());
    let mut contract = builder.build_catalog(scopes)?;
    for descriptor in crate::observer::read_descriptors() {
        contract.register_read(descriptor, ())?;
    }
    for descriptor in crate::alerts::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::attention::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::appearance::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::annotate::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::drawing_text::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::layout::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::canvas::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::layers::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::price_axis::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::home_tab::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::widgets::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::market_palette::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::desk::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::workspace_book::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::native_window::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::market_windows::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::time_axis::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::opening_scale::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::price_tag_progress::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::bubble_preset::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::bubble_save::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::bubble_scale::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::tape_fold::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::orderflow_settings::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::tape_view_set::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::recovery::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::feed_source::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::contract_roll::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::history::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::journal_sessions::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::session_reports::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::pnl_display::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::deal_recording::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::drawing_magnet::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::chart_settings::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::indicator_picker::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::native_indicator::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::chart_context::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::script_alarms::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::script_strategy::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::script::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::script_editor::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    for descriptor in crate::trade::action_descriptors() {
        contract.register_external(descriptor)?;
    }
    macro_rules! family_actions {
        ($($id:literal {
            schemas: [$($schema:ident),*], actions: [$($action:ident),*],
            projections: [$($projection:ident),*], authority: [$($authority:ident),*],
            readbacks: [$($readback:ident),*], behaviours: [$($behaviour:ident),*],
            samples: [$($sample:ident),*],
        })*) => { $( $(
            for descriptor in crate::$action::action_descriptors() {
                contract.register_external(descriptor)?;
            }
        )* )* };
    }
    quantick_control::control_families!(family_actions);
    Ok(contract)
}
