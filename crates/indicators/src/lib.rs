//! quantick-indicators — engine bars in, plot series out.
//!
//! The indicator runtime: the commit/preview execution contract, plot & input
//! models, and (in later milestones) the incremental `ta` kernels and the
//! multi-indicator host. A **pure domain crate** like the engine: no UI, no
//! network, no async, no threads, no wall clock.
//!
//! There is one place committed values live — the [`PlotBuffer`] a consumer
//! reads columns from. A second, general-purpose series store is deliberately
//! absent: an indicator that needs its own history (a script's `[n]` reads, a
//! kernel's window) keeps it in its own state and stages it with the same
//! commit/preview discipline. Per-bar color columns, when they land, extend
//! `PlotBuffer` rather than opening a second store beside it.
//!
//! # The contract
//!
//! An [`Indicator`] consumes [`IndicatorBar`]s — the f64 projection of engine
//! [`Bar`](quantick_engine::Bar)s, converted once at the crate boundary — and
//! commits plot rows into a [`PlotBuffer`]:
//!
//! - **Commit run** ([`Indicator::on_close`]): a closed bar evaluates once
//!   and persists exactly one plot row.
//! - **Preview run** ([`Indicator::preview`]): the forming bar evaluates
//!   against a snapshot of committed state and *everything rolls back*; only
//!   the returned [`PreviewFrame`] survives, and only for rendering. An EMA
//!   never advances twice inside one bar.
//!
//! The chart is just one consumer of this contract. A backtester or a bot
//! consumes the same trait and reads the same committed columns — never a
//! forked code path.
//!
//! # Determinism rules (non-negotiable)
//!
//! Same bars in, same plots out, bit-exact, on every platform:
//!
//! - no wall clock, no randomness, no iteration-order-dependent output;
//! - IEEE-754 `+ - * /`, `sqrt` and comparisons only; transcendentals go
//!   through the crate's libm wrapper [`fmath`] — never `f64::powf` & co.,
//!   which vary by platform (a test scans this crate's sources for them);
//! - `na` is `f64::NAN`; kernels propagate it Pine-style;
//! - guarded by [`golden`] tests that replay fixed fixtures twice and require
//!   byte-identical plot output.

pub mod fmath;
pub mod golden;
pub mod ta;

mod bar;
mod indicator;
mod input;
mod output;

pub use bar::IndicatorBar;
pub use indicator::{Ctx, EvalError, Indicator, IndicatorDescriptor};
pub use input::{InputSpec, InputValue, SourceId};
pub use output::{PlotBuffer, PlotId, PlotSpec, PlotStyle, PreviewFrame, Rgba8};
