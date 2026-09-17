//! One streaming fold for chart and runner, with optional retained history.
//!
//! [`SeriesFold`] returns each close by value and retains only the builder
//! and optional forming ladder. [`RetainedSeries`] composes it with the
//! original trade/deal evidence, provenance, revisions and rebuild policy.
//! Both use the engine's registered bar configuration; neither dispatches
//! on a bar-family identity. UI, feed scheduling and financial effects stay
//! with the caller. No clock is read here.

mod fold;
mod footprint;
mod retained;

pub use fold::{ClosedSeriesBar, SeriesFold};
pub use retained::RetainedSeries;

/// Capture width before a market supplies its grid. Enabling capture is
/// explicit; storing a width alone never allocates or folds a ladder.
pub const DEFAULT_FOOTPRINT_GROUP: rust_decimal::Decimal =
    rust_decimal::Decimal::from_parts(1, 0, 0, false, 2);

#[cfg(test)]
#[path = "tests/work_meter.rs"]
mod work_meter;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: work_meter::Counting = work_meter::Counting;
