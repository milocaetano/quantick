//! quantick-backtest — recorded sessions in, measured performance out.
//!
//! `CLAUDE.md` promises "one engine, three consumers: chart, backtest and
//! bot". This crate is the second one. It is the same code path the chart
//! runs, with the screen removed: raw prints from a recorded session go into
//! the very [`BarBuilder`] a tab uses, closed bars go into the very
//! [`IndicatorHost`] the panes read, and the same [`Simulator`] that fills a
//! paper trade on the chart fills the strategy's orders here.
//!
//! [`BarBuilder`]: quantick_engine::BarBuilder
//! [`IndicatorHost`]: quantick_indicators::IndicatorHost
//! [`Simulator`]: quantick_sim::Simulator
//!
//! # The loop
//!
//! One print at a time, in the order the tape printed them:
//!
//! ```text
//! for each trade:
//!     (a) sim.on_trade(trade)      resting orders meet the tape
//!     (b) builder.push(trade)      at most one bar closes per print
//!           -> host.push_closed_bar(bar)
//!           -> strategy.on_bar(view)     reads indicators, returns commands
//!     (c) sim.apply(command)       queued for the *next* print
//! ```
//!
//! A strategy is asked to decide only on a **closed** bar, and a market order
//! it places fills on the *next* print — the print it decided on has already
//! happened, and filling on it would be look-ahead. That rule lives in the
//! simulator, not here, which is why a backtest and the chart's paper trading
//! cannot drift apart.
//!
//! # What this crate refuses to do
//!
//! - **Invent data.** A ratio with no denominator prints `n/a`, never zero
//!   ([`report`]). A position still open when the recording ends is reported
//!   as open and left out of the metrics — it is not flattened at the last
//!   mark, because no print proves that fill.
//! - **Hide the tape's refusals.** Rejected commands and dropped brackets are
//!   counted per session and printed with the results ([`run::Anomalies`]).
//! - **Read a clock.** Every timestamp comes from `Trade::timestamp_ms`. The
//!   only wall-clock read in the crate is the stopwatch in `main.rs`, whose
//!   number goes to the stderr diagnostics and never into a report — see
//!   `tests/determinism_guard.rs`, which enforces exactly that.
//!
//! # Where the rules live
//!
//! Trading rules are **Rust**, not Pine: the dialect rejects `strategy.*` on
//! purpose. A script may *supply a signal* — the harness compiles it through
//! `quantick_pine` and the strategy reads its plot column like any other
//! indicator — but the order decision is code in [`strategy`]. See
//! [`strategies::PlotSignal`].

pub mod bars;
pub mod diagnostics;
pub mod report;
pub mod run;
pub mod strategies;
pub mod strategy;

pub use bars::BarSpec;
pub use report::{render_aggregate, render_run, render_session};
pub use run::{Anomalies, Distribution, RunOutcome, SessionRun, run_session};
pub use strategy::{Account, BarView, Signals, Strategy};
