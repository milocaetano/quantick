//! `orderflow.tape.fold.set` over the pane owner's narrow port: the view by the
//! side it closes. No rule of its own: every word is allowed from every view, and
//! repeating one changes nothing.
use crate::dock::ActionDock;
use crate::tape_pane::PaneTapePort;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_fold::{TapeFold, TapeFoldInput, descriptor};
use serde_json::Value;

/// The fold capability's narrow port on the pane.
pub trait TapeFoldPort {
    /// Show the view `fold` names, refusing a pane without an order-flow
    /// view; answers whether it changed.
    fn set_tape_fold(&mut self, fold: TapeFold) -> Result<bool, ControlError>;
    /// Keep the asset's market-closed setting; answers whether it changed.
    fn set_tape_market_fold(&mut self, enabled: bool) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: PaneTapePort<Pane: TapeFoldPort> + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)?;
    crate::tape_market_fold::register(registry)
}

fn set<H: PaneTapePort<Pane: TapeFoldPort> + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeFoldInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    answer(host.pane_tape(input.tab_id, input.pane_id)?, input)
}

fn answer<P: TapeFoldPort + ?Sized>(
    pane: &mut P,
    input: TapeFoldInput,
) -> Result<Value, ControlError> {
    let changed = pane.set_tape_fold(input.fold)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "tape_fold_tests.rs"]
mod tape_fold_tests;
