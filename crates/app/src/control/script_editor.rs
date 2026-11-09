//! The editor family's registry seam. No editor capability is exposed yet.
use super::actions::ActionRegistry;
use quantick_control::registry::RegistryError;

pub(crate) fn register(registry: &mut ActionRegistry) -> Result<(), RegistryError> {
    quantick_control_handlers::script_editor::register(registry)
}
