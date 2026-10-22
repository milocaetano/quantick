//! Folding the tape lane over the pane owner's narrow port. The rule — a
//! side folds only while the tape sits beside the candles, and opening is
//! always allowed — lives here; the port reads facts and keeps the fold.
use crate::bubble_scale::refused_precondition;
use crate::dock::ActionDock;
use quantick_control::{error::ControlError, registry::RegistryError, wire::ActorContext};
use quantick_control_schema::tape_fold::{
    BESIDE_CANDLES_PRECONDITION, TapeFold, TapeFoldInput, descriptor,
};
use serde_json::Value;

/// What the fold rule reads about a pane.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TapeFoldPane {
    /// The tape is switched on.
    pub tape_on: bool,
    /// The pane shows the tape alone, so there is no divider to fold.
    pub tape_only: bool,
}

impl TapeFoldPane {
    /// Whether `fold` may be kept on this pane.
    #[must_use]
    pub fn admits(self, fold: TapeFold) -> bool {
        fold == TapeFold::Open || (self.tape_on && !self.tape_only)
    }
}

/// The fold capability's narrow port.
pub trait TapeFoldPort {
    /// The addressed pane's facts; refuses a pane without an order-flow view.
    fn tape_fold_pane(&mut self, tab_id: u64, pane_id: u64) -> Result<TapeFoldPane, ControlError>;
    /// Keep `fold` on the pane; answers whether it changed.
    fn set_tape_fold(
        &mut self,
        tab_id: u64,
        pane_id: u64,
        fold: TapeFold,
    ) -> Result<bool, ControlError>;
}

pub fn register<D: ActionDock<H, A>, H: TapeFoldPort + ?Sized, A>(
    registry: &mut D,
) -> Result<(), RegistryError> {
    registry.register(descriptor(), set)
}

fn set<H: TapeFoldPort + ?Sized, A>(
    host: &mut H,
    _access: &mut A,
    _actor: &ActorContext,
    input: &Value,
) -> Result<Value, ControlError> {
    let input: TapeFoldInput = serde_json::from_value(input.clone()).map_err(invalid)?;
    let (tab, pane) = (input.tab_id.get(), input.pane_id.get());
    if !host.tape_fold_pane(tab, pane)?.admits(input.fold) {
        return Err(refused_precondition(
            BESIDE_CANDLES_PRECONDITION,
            "only a tape shown beside its candles can fold a side",
            "switch the tape on and tape only off (layers.visibility.set), or ask for open",
        ));
    }
    let changed = host.set_tape_fold(tab, pane, input.fold)?;
    serde_json::to_value(input.result(changed)).map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ControlError {
    ControlError::invalid_request(error.to_string())
}

#[cfg(test)]
#[path = "tape_fold_tests.rs"]
mod tape_fold_tests;
