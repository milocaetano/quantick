//! The application adapter for desk control.

use crate::app::QuantickApp;

impl quantick_control_handlers::desk::DeskHost for QuantickApp {
    fn desk(&mut self) -> Option<Box<dyn quantick_control_handlers::desk::DeskOwner + '_>> {
        Some(Box::new(
            crate::app::control_host::control_roots_mut(self).bound_workspace_calls(),
        ))
    }
}
