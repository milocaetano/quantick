//! The application adapter for the workspace book.

use crate::app::QuantickApp;
use quantick_control_handlers::workspace_book::{WorkspaceBookHost, WorkspaceOwner};

impl WorkspaceBookHost for QuantickApp {
    fn workspaces(&mut self) -> Option<Box<dyn WorkspaceOwner + '_>> {
        Some(Box::new(
            crate::app::control_host::control_roots_mut(self).workspace_calls(),
        ))
    }
}
