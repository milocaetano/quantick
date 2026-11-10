//! Narrow alert service port. The implementation owns admission-safe CRUD.
use quantick_alerts::{AlertCommand, AlertError, AlertEvent, AlertSpec, AlertView, CommandResult};

pub trait AlertDefinitionsPort {
    fn alert_book(&self) -> &quantick_alerts::AlertBook;
    fn alert_book_at(&mut self) -> (&mut quantick_alerts::AlertBook, i64);
    fn alert_command(&mut self, command: AlertCommand) -> Result<CommandResult, AlertError> {
        let (book, utc_ms) = self.alert_book_at();
        book.command(command, utc_ms)
    }
    fn alert_validate(&self, spec: &AlertSpec) -> Result<(), AlertError> {
        self.alert_book().validate(spec)
    }
    fn alert_views(&self) -> Result<Vec<AlertView>, AlertError> {
        self.alert_book().views()
    }
    fn alert_history(&self, after: u64, limit: usize) -> Result<Vec<AlertEvent>, AlertError> {
        self.alert_book().history(after, limit)
    }
}

/// Reserved family registration, not an executable successful placeholder.
/// The next child supplies descriptors and handlers entirely in this module.
pub fn register<H: AlertDefinitionsPort + ?Sized, A>(
    _registry: &mut impl crate::dock::ActionDock<H, A>,
) -> Result<(), quantick_control::registry::RegistryError> {
    Ok(())
}
