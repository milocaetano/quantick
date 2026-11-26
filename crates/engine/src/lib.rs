//! quantick-engine: raw trades in, alternative bars out. Headless and deterministic (no UI,
//! network, async or wall clock): same trades in, same bars out. [`bar_registry`] holds the
//! definitions ([`BarSpec`] adapts the enum); [`fixture`] feeds golden tests.

pub mod bar_registry;
pub mod bar_selection;
pub mod bar_timeline;
pub mod display;
pub mod fixture;
pub mod forming_run;
pub mod golden;
pub mod trade_tape;
pub mod work_meter;

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: work_meter::Counting = work_meter::Counting;

mod bar;
mod builder;
mod deals;
mod dollar;
mod footprint;
mod imbalance;
mod price_grid;
mod profile;
mod profile_fold;
mod renko;
mod spec;
pub mod threshold;
mod tick;
mod time;
pub mod time_bucket;
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
pub use renko::{MAX_BRICKS_PER_PRINT, RenkoBarBuilder, STEP_EVIDENCE_DISTANCES};
pub use spec::{
    BarKind, BarSpec, BarSpecError, DECIMAL_PARAM_FLOOR, DEFAULT_TIME_INTERVAL_MS,
    MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS, fmt_time_interval, parse_time_interval,
};
pub use threshold::{Measure, ThresholdBarBuilder};
pub use tick::{TickBarBuilder, TickMeasure};
pub use time::TimeBarBuilder;
pub use trade::{Side, Trade};
pub use volume::{VolumeBarBuilder, VolumeMeasure};

/// Source-to-consumer delay when an event arrives; `None` without an event timestamp.
/// Negative if the local clock is behind the source's: reported as-is, not clamped.
#[must_use]
pub fn feed_lag_ms(received_at_ms: i64, event_time_ms: Option<i64>) -> Option<i64> {
    event_time_ms.map(|timestamp_ms| received_at_ms.saturating_sub(timestamp_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lag_is_observation_minus_event_time() {
        assert_eq!(feed_lag_ms(1_000, Some(600)), Some(400));
        assert_eq!(feed_lag_ms(1_000, None), None);
        // Local clock behind the exchange: negative lag, reported honestly.
        assert_eq!(feed_lag_ms(500, Some(600)), Some(-100));
    }
}
