//! `orderflow.tape.fold.set` over the pane owner's narrow port: the view by the
//! side it closes. No rule of its own: every word is allowed from every view, and
//! repeating one changes nothing.
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_fold::{TapeFold, TapeFoldInput, descriptor};
use serde_json::Value;

/// The fold capability's narrow port.
pub trait TapeFoldPort {
    /// Show the view `fold` names on the pane, refusing a pane without an
    /// order-flow view; answers whether it changed.
    fn set_tape_fold(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        fold: TapeFold,
    ) -> Result<bool, ControlError>;
    /// Keep the asset's market-closed setting; answers whether it changed.
    fn set_tape_market_fold(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        enabled: bool,
    ) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: TapeFoldPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)?;
    crate::tape_market_fold::register(registry)
}

fn set<H: TapeFoldPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeFoldInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let changed = host.set_tape_fold(input.tab_id.get(), input.pane_id.get(), input.fold)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "tape_fold_tests.rs"]
mod tape_fold_tests;
