//! Deterministic strategy kernel — the brain both the chart and the backtest
//! arm.
//!
//! The division of labour is the product's thesis: **a human marks the
//! region, the machine pulls the trigger**. A congestion zone is judgement a
//! human reads off the chart and no rule mechanises honestly; the force-bar
//! trigger inside it is a reaction-time race a human always loses. This
//! crate owns everything between those two facts: the trigger ruler, the
//! region test, the projected protective bracket and the armed-instance
//! state machine. What it deliberately does not own is geometry resolution
//! (the app translates a drawing into a [`Region`] and a time window) and
//! execution (commands go to `quantick-sim`, which fills them against the
//! tape).
//!
//! Like `engine` and `sim`, this is a pure domain crate: no UI, no network,
//! no async, no wall clock, no randomness. Same bars + same account states
//! in → same commands out, always. The chart's live paper trading and the
//! backtest harness consume the *same* kernel — never fork strategy logic
//! per consumer.
//!
//! Triggers are a port ([`Trigger`]): the shipped ruler is the force bar
//! (the `force_bar.pine` band: body between `min_factor`× and `max_factor`×
//! the average body), and later rulers — the operational document's BEI is
//! the expected next tenant — dock beside it without surgery on the state
//! machine.

mod armed;
mod force;
mod region;
mod trigger;

pub use armed::{ArmedState, ArmedStrategy, DisarmReason, Rearm, StrategyParams};
pub use force::{BarVerdict, ForceBar, ForceParams, ForceWindow};
pub use region::Region;
pub use trigger::{ForceTrigger, Signal, Trigger};
