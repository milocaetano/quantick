//! The venue-neutral trading vocabulary, and the port every execution
//! backend implements.
//!
//! This crate answers one question: *what does a surface need to know about
//! execution before it knows which venue is executing?* Orders, brackets,
//! positions, fills, round trips and refusals are all facts about trading,
//! not about simulation — so they live here, and the deterministic paper
//! simulator in `quantick-sim` is one implementation of [`TradingVenue`]
//! rather than the definition of the domain. A real broker adapter docks at
//! the same trait without the chart, the ticket or the control plane
//! learning a second vocabulary.
//!
//! # Where the line is drawn
//!
//! The port covers the **order lifecycle**: submitting intents, amending
//! prices and brackets, cancelling, closing, and reading back what is
//! working and what is open. It deliberately stops short of two things that
//! belong to a *practice* venue rather than to venues in general — the
//! `quantick-trades` CSV journal and the performance report, both of which
//! stay in `quantick-sim`. A broker reports its own history; re-deriving it
//! from a simulated round trip would be the wrong kind of reuse.
//!
//! # A pure domain crate
//!
//! Like `engine`: no UI, no network, no async, no wall clock, no randomness.
//! Every timestamp in here is the venue time of a trade someone was shown.
//! It depends on `engine` for [`Side`](quantick_engine::Side) and
//! [`Trade`](quantick_engine::Trade), and on nothing else in the workspace.

mod events;
mod intent;
mod order;
mod position;
mod venue;

pub mod fake;

pub use events::{CancelReason, ExitReason, Fill, FillRole, RejectReason, VenueEvent};
pub use intent::{BracketTarget, CloseAmount, OrderIntent};
pub use order::{Bracket, EntryKind, Order, OrderId};
pub use position::{ClosedTrade, Position, signed_points};
pub use venue::TradingVenue;
