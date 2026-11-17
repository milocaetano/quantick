//! The flow pane's view over the pane owner's narrow port: allowed from any view.
//! Beside stands the market's hold down at the default width, as
//! `orderflow.tape.fold.set` open does.
use crate::addressed_pane::AddressedPanePort;
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_view_set::{TapeView, TapeViewSetInput, descriptor};
use serde_json::Value;

/// The view capability's narrow port on the pane.
pub trait TapeViewPort {
    /// Show `view`, from any view; answers whether it changed. Refuses a
    /// pane without an order-flow view.
    fn set_tape_view(&mut self, view: TapeView) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: AddressedPanePort<Pane: TapeViewPort> + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: AddressedPanePort<Pane: TapeViewPort> + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeViewSetInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    answer(host.addressed_pane(input.tab_id, input.pane_id)?, input)
}

fn answer<P: TapeViewPort + ?Sized>(
    pane: &mut P,
    input: TapeViewSetInput,
) -> Result<Value, ControlError> {
    let changed = pane.set_tape_view(input.view)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "tape_view_set_tests.rs"]
mod tape_view_set_tests;
