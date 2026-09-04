//! The [`BarBuilder`] abstraction shared by every bar type.

use rust_decimal::Decimal;

use crate::{Bar, DealSample, Trade};

/// How far the in-progress bar is from closing.
///
/// Both figures are in the rule's own measure — trades for tick bars, quantity
/// for volume, notional for dollar, milliseconds for time — so a consumer can
/// render "37 of 50" without knowing which rule is running. Alternative bars
/// are not on a clock, so nothing on a chart otherwise says whether the next
/// print closes the bar or the fiftieth does; this is the only honest answer,
/// and it comes from the builder that owns the closing rule rather than from a
/// second copy of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarProgress {
    /// Measure accumulated by the trades since the last close.
    pub done: Decimal,
    /// Measure at which this bar closes.
    pub target: Decimal,
}

/// Turns a stream of [`Trade`]s into a stream of [`Bar`]s.
///
/// Every bar type — tick, volume, dollar, time — is a `BarBuilder`. Trades are
/// fed one at a time in occurrence order via [`push`](BarBuilder::push); a bar
/// is returned the moment its sampling bucket fills. This one-trade-in,
/// maybe-a-bar-out shape is what makes the same code path drive a chart, a
/// backtest and a bot ("one engine, three consumers").
///
/// A builder is a state machine: the trades seen since the last closed bar form
/// the **in-progress** bar, exposed by [`partial`](BarBuilder::partial) so a
/// chart can render the rightmost bar forming in real time. When a bucket fills,
/// that in-progress bar is finalised, returned from `push`, and the builder
/// starts a fresh one.
pub trait BarBuilder {
    /// Feed one trade, in occurrence order.
    ///
    /// Returns `Some(bar)` if this trade completed a bar, `None` if the trade
    /// only extended the in-progress bar. At most one bar closes per trade: a
    /// trade is an atomic market event and is never split across bars (see the
    /// boundary rule the threshold builder documents).
    fn push(&mut self, trade: &Trade) -> Option<Bar>;

    /// The in-progress bar — the trades seen since the last close — or `None`
    /// if no trade has arrived since the last bar closed.
    ///
    /// This bar is *not* closed: its `close`/`close_time` reflect only the
    /// trades so far and will keep moving until the bucket fills. Consumers that
    /// need finalised bars only should use the return value of
    /// [`push`](BarBuilder::push); `partial` is for rendering the forming bar.
    fn partial(&self) -> Option<&Bar>;

    /// How far the in-progress bar is from closing, in this rule's measure.
    ///
    /// `None` — the default — when the rule runs toward no fixed threshold, as
    /// an adaptive rule does. Reporting a countdown that is not the rule would
    /// tell the reader the bar closes at a moment it will not.
    fn progress(&self) -> Option<BarProgress> {
        None
    }

    /// Hand the builder one reading of the venue's session deal counter.
    ///
    /// Only a rule that counts what the venue counts listens
    /// ([`crate::DealBarBuilder`]); every other rule is fed by prints alone
    /// and ignores the reading, which is why the default does nothing. A
    /// consumer feeds every sample it holds through this one method whatever
    /// rule is running, so switching rules never has to know which of them
    /// wants the counter.
    fn observe_deals(&mut self, _sample: DealSample) {}

    /// Prints this builder could not place in any bar because the rule had
    /// nothing to count them against — a deal bar before the first counter
    /// reading. Zero for every rule fed by prints alone.
    ///
    /// Reported rather than hidden: a chart owes the trader the number of
    /// prints it is showing no bar for.
    fn uncounted_trades(&self) -> u64 {
        0
    }
}
