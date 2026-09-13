//! quantick-paper — the paper account, headless.
//!
//! The deciding half of paper trading: the account that places, fills,
//! protects, sizes and journals ([`account`]), the risk-per-trade policy it
//! sizes with ([`risk_sizing`]), the exit ladders a trader keeps
//! ([`order_strategies`]), and the report numbers read back from the journal
//! it writes ([`report`]), on one civil-date law ([`civil`]).
//!
//! `CLAUDE.md` promises one engine and three consumers. For bars that is the
//! aggregator; for paper trading it is this crate. The chart's ticket, the
//! backtest harness and a bot drive the same [`PaperAccount`] against the
//! same `quantick_sim` venue, so a fill rule, a rounding or a journal byte
//! cannot differ between them.
//!
//! Headless like every crate below `app`: no UI, no thread, no async and no
//! wall clock (`crates/guards/src/headless.rs` scans it). The host *tells*
//! it what it cannot know — the journal folder, what the trader typed, a
//! launch hook's risk — and reads back what it cannot do itself through an
//! outbox ([`account::AccountResponse`]). Session files are named from
//! venue time, so the same replay writes the same bytes.

pub mod account;
pub mod civil;
pub mod format;
pub mod order_strategies;
pub mod report;
pub mod risk_sizing;

#[cfg(test)]
mod scratch;

pub use account::PaperAccount;
