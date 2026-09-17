//! Deterministic indicator sessions over the existing host and Pine frontend.
//! The caller owns admission, transport, clocks and persistence. Effects run
//! synchronously at the original transitions, without buffering output history.
mod batch;
mod lane;
mod protocol;
mod publication;
mod session;
mod source;
pub use batch::{Applying, Publishing};
pub use lane::{LaneDiagnostics, MAX_LANE_RUNGS};
pub use protocol::{IndicatorCommand, IndicatorEvent, LaneSample, SlotId, fold_parked};
pub use session::IndicatorSession;
pub use source::IndicatorSource;

/// Positional rebinding facts, reported before constructing the new script.
#[derive(Debug)]
pub struct InputsRebound<'a> {
    pub script: &'a str,
    pub saved: usize,
    pub declared: usize,
    pub kept: usize,
    pub count_changed: bool,
}
/// Synchronous outputs, without a session, clock or host-control capability.
/// Blocking or unwinding stops the transition at that emission. The recipient
/// owns delivery-failure policy; the domain does not cancel on disconnection.
pub trait SessionEffects {
    fn event(&mut self, event: IndicatorEvent);
    fn inputs_rebound(&mut self, diagnostic: InputsRebound<'_>);
}
