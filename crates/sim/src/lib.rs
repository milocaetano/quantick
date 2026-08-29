//! Deterministic paper-trading simulator: trades in, simulated fills out.
//!
//! [`Simulator`] consumes the same trade stream the bar engine consumes, plus
//! order [`Command`]s issued between prints, and produces simulated fills,
//! one net [`Position`], and [`ClosedTrade`] records. It is a pure domain
//! crate like `engine`: no UI, no network, no async, no wall clock — every
//! timestamp it emits is the venue time of a print it was shown.
//!
//! # Fill model (tape-based, conservative)
//!
//! The tape is the only evidence. Nothing fills at a price the tape has not
//! printed — quotes, book depth and queue position are unknown to a trade
//! stream, so the simulator never invents them:
//!
//! - A **market** order fills at the *next* print, whatever its price. The
//!   print that was already on screen when the user clicked has happened;
//!   trading on it would be look-ahead.
//! - A **limit** order rests until a print trades at or through its price and
//!   fills *at the limit price*. A print through the price proves the level
//!   cleared; a print exactly at it assumes front-of-queue priority — the one
//!   documented optimism in the model.
//! - A **stop** (entry or protective stop loss) triggers on the print that
//!   trades at or through it and fills *at that print's price* — on a gap the
//!   fill is honestly worse than the trigger.
//! - A **take profit** is a resting limit and fills at its own price.
//!
//! A limit that would fill immediately (a buy limit at or above the market)
//! is rejected with advice rather than silently filled — a didactic
//! simulator teaches the difference instead of papering over it.
//!
//! A **limit with a cancel-at price** ([`Command::PlaceLimit`]'s
//! `cancel_at`) removes itself, unfilled, on a print trading at or through
//! that level ([`CancelReason::PriceTouched`]) — "the move completed
//! without the retest, so stop waiting for one". Validation keeps the
//! cancel level on the far side of the market from the limit price, so no
//! single print can ever satisfy both fill and cancel.
//!
//! A **flat-only limit** ([`Command::PlaceLimit`]'s `flat_only`) fills only
//! into an account with no open position: if its fill print arrives while
//! one is open, the order stands down instead
//! ([`CancelReason::AccountOccupied`]). The strategy kernel rests its
//! entries this way — an order placed under a flat-account promise must
//! never net a position a human opened while it rested.
//!
//! A protective level attached to a market or stop entry is re-checked
//! against the *actual* fill price: validation ran against the mark, but
//! the fill lands on a later print, and when the tape outran the level in
//! between it is dropped and reported ([`VenueEvent::BracketDropped`]) —
//! kept, it would exit on the next print wearing a lying label.
//!
//! # Processing order within one print
//!
//! Deterministic and fixed: (1) position brackets — stop loss, then take
//! profit; (2) queued market actions, in command order; (3) resting orders,
//! in placement order. An entry filled by a print arms its bracket starting
//! from the *next* print, so a position can never be stopped out by the print
//! that opened it.
//!
//! # Determinism
//!
//! Same prints + same commands at the same points in the stream → same
//! fills, always. No wall clock, no randomness, `Vec` iteration only. The
//! simulator never reads batch boundaries or arrival times — the only time
//! it knows is `Trade::timestamp_ms`.
//!
//! # Honesty
//!
//! Everything the simulator reports is *simulated* and labeled so by the
//! consumer. P&L is reported in **points** (price units × quantity): the
//! workspace has no per-instrument tick value or currency table, and a
//! number the simulator cannot compute honestly is a number it does not
//! show. Simulated positions and pending orders do not survive a session;
//! only [`ClosedTrade`] history is meant to be persisted (see [`history`]).

mod simulator;
mod venue;

pub mod history;
pub mod report;

// The order vocabulary is not the simulator's: orders, brackets, positions
// and round trips are facts about trading, and `quantick-trading` owns them
// so a real venue can speak them too. They are re-exported here because a
// consumer that already talks to the paper simulator should not have to
// learn where each type happens to live.
pub use quantick_trading::{
    Bracket, BracketTarget, CancelReason, CloseAmount, ClosedTrade, EntryKind, ExitReason, Fill,
    FillRole, Order, OrderId, OrderIntent, Position, RejectReason, TradingVenue, VenueEvent,
    signed_points,
};
pub use report::{PerformanceReport, ReasonReport, SideReport};
pub use simulator::{Command, QueuedAction, Simulator};
