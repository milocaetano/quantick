//! Narrow alert service port. The implementation owns admission-safe CRUD.
use quantick_alerts::{AlertCommand, AlertError, AlertEvent, AlertSpec, AlertView, CommandResult};

pub trait AlertDefinitionsPort {
    fn alert_command(&mut self, command: AlertCommand) -> Result<CommandResult, AlertError>;
    fn alert_validate(&self, spec: &AlertSpec) -> Result<(), AlertError>;
    fn alert_views(&self) -> Result<Vec<AlertView>, AlertError>;
    fn alert_history(&self, after: u64, limit: usize) -> Result<Vec<AlertEvent>, AlertError>;
}

/// Reserved family registration, not an executable successful placeholder.
/// The next child supplies descriptors and handlers entirely in this module.
pub fn register<H: AlertDefinitionsPort + ?Sized, A>(
    _registry: &mut impl crate::dock::ActionDock<H, A>,
) -> Result<(), quantick_control::registry::RegistryError> {
    Ok(())
}
