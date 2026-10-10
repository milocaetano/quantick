//! The headless chart model: what the chart *is* before anything draws it.
//!
//! [`state::ChartState`] is the chart's side of the one-engine boundary —
//! trades in, bars out, for whichever bar spec the trader picked.
//! [`geometry`] maps those bars to pixels, [`viewport`] decides which of them
//! are on screen, [`style`] says how a candle looks and [`live_strip`] shapes
//! the forming bar's aggression. [`indicator_views`] is the UI's copy of the
//! indicator session's output, and the height each indicator pane asks for.
//! None of it names a renderer: every module is
//! unit-tested in CI without a display, and a second consumer — a backtest
//! report, an agent reading the chart — reaches the same code the window
//! paints from.
//!
//! [`work_meter`] is the test instrument the budgets in this crate and in
//! `app` are held with: a counting allocator, installed by each test binary.

mod constants;
pub mod day_turn;
pub mod flow_execution;
pub mod footprint_lod;
pub mod footprint_projection;
pub mod footprint_series;
pub mod geometry;
pub mod history_publication;
pub mod indicator_style;
pub mod indicator_views;
pub mod live_strip;
pub mod price_axis_fit;
pub mod price_view;
pub mod state;
pub mod style;
pub mod tick_membership;
pub mod viewport;
pub mod work_meter;

#[cfg(test)]
#[global_allocator]
static TEST_ALLOCATOR: work_meter::Counting = work_meter::Counting;
