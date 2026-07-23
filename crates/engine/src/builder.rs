//! The [`BarBuilder`] abstraction shared by every bar type.

use crate::{Bar, Trade};

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
}
