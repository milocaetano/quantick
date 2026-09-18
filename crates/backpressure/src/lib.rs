//! Backpressure between an owner and the worker it feeds: bounded command
//! admission, parking and folding when the channel is full, and the
//! owner-local progress diagnostics that show it holding.
//!
//! [`backlog`] is the queue discipline — park, fold, count, never drop — and
//! [`progress`] publishes the counts. Neither reads a clock or spawns
//! anything: the owner installs a [`progress::ProgressClock`] and says, at
//! the end of a worker's life, whether its thread unwound. The chart's
//! indicator and book workers run on this; a backtest driving the same
//! workers without a window would too.

pub mod backlog;
pub mod progress;
