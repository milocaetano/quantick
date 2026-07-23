//! quantick-engine — raw trades in, alternative bars out.
//!
//! Headless and deterministic: no UI, no network, no async, no wall-clock
//! time. Same trades in, same bars out, always. See `CLAUDE.md` for the
//! non-negotiable design rules.
//!
//! # The input/output contract
//!
//! The engine consumes [`Trade`]s and produces [`Bar`]s. Everything else is a
//! consumer of this contract:
//!
//! - [`Trade`] — one executed (aggregate) trade: price, quantity, aggressor
//!   [`Side`], exchange id and timestamp. Prices and quantities are
//!   [`rust_decimal::Decimal`] for exact, deterministic arithmetic.
//! - [`Bar`] — the OHLCV + order-flow summary of the trades in one sampling
//!   bucket. The bucketing rule (tick / volume / dollar / time) lives in a bar
//!   *builder*; the summary shape is shared.
//!
//! The [`fixture`] module defines the plain-text trade format that golden tests
//! replay to guard determinism.

pub mod fixture;
pub mod golden;

mod bar;
mod builder;
pub mod threshold;
mod tick;
mod trade;

pub use bar::Bar;
pub use builder::BarBuilder;
pub use threshold::{Measure, ThresholdBarBuilder};
pub use tick::{TickBarBuilder, TickMeasure};
pub use trade::{Side, Trade};
