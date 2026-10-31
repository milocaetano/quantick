//! The flow pane's view over the pane owner's narrow port: allowed from any
//! view, and it reads and writes the view alone, never a fold.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_view_set::{TapeView, TapeViewSetInput, descriptor};
use serde_json::Value;

/// The view capability's narrow port.
pub trait TapeViewPort {
    /// Show `view` on the pane, from any view; answers whether it changed.
    /// Refuses a pane without an order-flow view.
    fn set_tape_view(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        view: TapeView,
    ) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: TapeViewPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: TapeViewPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeViewSetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let changed = host.set_tape_view(input.tab_id.get(), input.pane_id.get(), input.view)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "tape_view_set_tests.rs"]
mod tape_view_set_tests;
