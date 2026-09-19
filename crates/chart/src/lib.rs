//! The headless chart model: what the chart *is* before anything draws it.
//!
//! [`state::ChartState`] is the chart's side of the one-engine boundary —
//! trades in, bars out, for whichever bar spec the trader picked.
//! [`geometry`] maps those bars to pixels, [`viewport`] decides which of them
//! are on screen, [`style`] says how a candle looks and [`live_strip`] shapes
//! the forming bar's aggression. None of it names a renderer: every module is
//! unit-tested in CI without a display, and a second consumer — a backtest
//! report, an agent reading the chart — reaches the same code the window
//! paints from.
//!
//! [`work_meter`] is the test instrument the budgets in this crate and in
//! `app` are held with: a counting allocator, installed by each test binary.

pub mod footprint_series;
pub mod geometry;
pub mod indicator_style;
pub mod live_strip;
pub mod price_view;
pub mod state;
pub mod style;
pub mod viewport;
pub mod work_meter;

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: work_meter::Counting = work_meter::Counting;
