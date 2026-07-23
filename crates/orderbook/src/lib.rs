//! Deterministic local order-book state.
//!
//! This crate owns only market-depth domain rules: validated snapshots,
//! absolute level updates and exchange update-id continuity. It deliberately
//! contains no network, async runtime, wall clock, logging or UI code.

#![forbid(unsafe_code)]

mod book;
mod error;
mod model;

pub use book::{ApplyOutcome, OrderBook};
pub use error::BookError;
pub use model::{BookCoverage, BookDelta, BookLevel, BookSide, BookSnapshot};
