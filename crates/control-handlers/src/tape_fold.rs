//! The flow pane's view over the pane owner's narrow port, by its own name and
//! by the older fold name, which says the same thing by the side it hides.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_fold::{
    TapeFoldInput, TapeView, TapeViewInput, descriptor, view_descriptor,
};
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
    registry.register(view_descriptor(), set_view)?;
    registry.register(descriptor(), set_fold)
}

fn set_view<H: TapeViewPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeViewInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let changed = host.set_tape_view(input.tab_id.get(), input.pane_id.get(), input.view)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn set_fold<H: TapeViewPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeFoldInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let view = input.fold.into();
    let changed = host.set_tape_view(input.tab_id.get(), input.pane_id.get(), view)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "tape_fold_tests.rs"]
mod tape_fold_tests;
