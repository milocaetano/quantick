//! Domain fixtures moved with their owner; the legacy oracle stays test-only.
use super::RetainedSeries as ChartState;
use quantick_engine::bar_registry::BarConfiguration;
use quantick_engine::{Bar, BarFootprint, BarSpec, DealSample, ImbalanceUnit, Trade};
use rust_decimal::Decimal;
mod legacy_fold;
use legacy_fold as footprint_series;
use legacy_fold::{FootprintSeries, fold_print, seed_deal_counter};
mod bar_spec_parity_tests;
mod fold_work_tests;
mod footprint_tests;
mod lifecycle_characterization_tests;
mod seed_work_tests;
mod state_tests;
mod tape_identity_tests;
