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
//! - [`bar_registry`] — stable definitions, parameter contracts and factories.
//!   Chart, backtest and bot retain its resolved configurations. [`BarSpec`]
//!   preserves the original enum API as an adapter to those definitions.
//!
//! The [`fixture`] module defines the plain-text trade format that golden tests
//! replay to guard determinism.

pub mod bar_registry;
pub mod bar_selection;
pub mod bar_timeline;
pub mod fixture;
pub mod forming_run;
pub mod golden;
pub mod trade_tape;

mod bar;
mod builder;
mod deals;
mod dollar;
mod footprint;
mod imbalance;
mod price_grid;
mod profile;
mod profile_fold;
mod spec;
pub mod threshold;
mod tick;
mod time;
mod trade;
mod volume;

pub use bar::Bar;
pub use builder::{BarBuilder, BarBuilderDiagnostics, BarProgress, DealCounterInput};
pub use deals::{DealBarBuilder, DealSample, READING_MAX_AGE_MS};
pub use dollar::{DollarBarBuilder, DollarMeasure};
pub use footprint::{
    BarFootprint, DEFAULT_LEVEL_CAP, Extreme, FootprintBuilder, FootprintLevel, Imbalance,
    StackedZone,
};
pub use imbalance::{ImbalanceBarBuilder, ImbalanceUnit};
pub use price_grid::PriceGrid;
pub use profile::{ValueArea, VolumeProfile};
pub use profile_fold::ProfileFold;
pub use spec::{
    BarKind, BarSpec, BarSpecError, DECIMAL_PARAM_FLOOR, DEFAULT_TIME_INTERVAL_MS,
    MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS, fmt_time_interval,
};
pub use threshold::{Measure, ThresholdBarBuilder};
pub use tick::{TickBarBuilder, TickMeasure};
pub use time::TimeBarBuilder;
pub use trade::{Side, Trade};
pub use volume::{VolumeBarBuilder, VolumeMeasure};
