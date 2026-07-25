//! Deterministic local order-book state.
//!
//! This crate owns only market-depth domain rules: validated snapshots,
//! absolute level updates and exchange update-id continuity. It deliberately
//! contains no network, async runtime, wall clock, logging or UI code.
//!
//! It also owns the vocabulary feeds and consumers share when depth is
//! *streamed* ([`stream`]) and the adapter that lets an image-only venue speak
//! it ([`image`]). Both are pure domain code, and keeping them here is what
//! stops a second venue from either inventing its own dialect or forcing the
//! app to depend on an unrelated exchange's crate.

#![forbid(unsafe_code)]

mod book;
mod error;
pub mod image;
mod model;
pub mod stream;

pub use book::{ApplyOutcome, OrderBook};
pub use error::BookError;
pub use image::{ImageOutcome, SnapshotDiffer};
pub use model::{BookCoverage, BookDelta, BookLevel, BookSide, BookSnapshot};
pub use stream::{DepthEvent, DepthResyncReason, DepthStatus};
