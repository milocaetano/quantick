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
//!
//! The *region test* is read off the trigger bar's **body** — open to
//! close, its wicks ignored ([`BodyCut`]) — because a shadow poking into
//! the band is the level being probed and refused, not cut. That scope is
//! deliberate and it is only the region test: the **projection** that
//! prices both protective legs is still the bar's full range, wicks
//! included, so a long-shadowed force bar brackets wide. The body decides
//! whether the bar *reached* the region; the range then decides how wide
//! the bracket sits — and because a retest limit whose legs do not clear
//! the edge is refused rather than fired bare, the range has a say in
//! whether that entry happens at all. Two gates, not one.
//!
//! A bar that closes *inside* the region fires a market entry, whatever
//! its open did. One that cut **through** the region — opening on the
//! region's side of the edge the trade leaves by and closing beyond it —
//! follows [`BreakPolicy`]: hold fire (the default), or rest a limit at
//! the cut edge, bracketed off the trigger bar and cancelled if the tape
//! reaches the bar's projected target before returning for the retest —
//! or refused outright when those legs would not clear the edge, since a
//! resting entry is never armed unprotected. A body that finished beyond
//! an edge it never crossed cut nothing, and rests nothing.

mod armed;
mod force;
mod region;
mod trigger;

pub use armed::{ArmedState, ArmedStrategy, BreakPolicy, DisarmReason, Rearm, StrategyParams};
pub use force::{BarVerdict, ForceBar, ForceParams, ForceWindow};
pub use region::{BodyCut, Region};
pub use trigger::{ForceTrigger, Signal, Trigger};
