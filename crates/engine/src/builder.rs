//! The [`BarBuilder`] abstraction shared by every bar type.

use rust_decimal::Decimal;

use crate::{Bar, DealSample, Trade};

/// Optional venue input kept separate from the universal print-driven bar
/// contract. A builder that needs the deal counter exposes this port; other
/// builders implement no pretend no-op operation.
pub trait DealCounterInput {
    fn observe(&mut self, sample: DealSample);
}

/// What a builder could not do with the prints it was fed, counted rather
/// than hidden. All zero for a rule that cuts every print as it arrives.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BarBuilderDiagnostics {
    /// Prints placed in no bar: the rule had nothing to count them against
    /// — a deal bar before its first counter reading.
    pub uncounted_trades: u64,
    /// Prints in no closed bar yet because the rule cannot measure them —
    /// a Renko builder before its own prints have shown its price step.
    /// They form the in-progress bar and are cut once it is known; prints
    /// still held when a tape ends were never cut.
    pub held_prints: u64,
    /// Prints off the price grid the rule froze: cut on that grid and
    /// counted, never snapped onto it.
    pub off_grid_prints: u64,
}

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
/// Every bar type — tick, volume, dollar, time, Renko — is a `BarBuilder`.
/// Trades are fed one at a time in occurrence order via
/// [`push_into`](BarBuilder::push_into); a bar is handed out the moment its
/// sampling bucket fills. This one-trade-in, bars-out shape is what makes the
/// same code path drive a chart, a backtest and a bot ("one engine, three
/// consumers").
///
/// A builder is a state machine: the trades seen since the last closed bar form
/// the **in-progress** bar, exposed by [`partial`](BarBuilder::partial) so a
/// chart can render the rightmost bar forming in real time. When a bucket fills,
/// that in-progress bar is finalised, handed out, and the builder starts a
/// fresh one.
pub trait BarBuilder {
    /// Feed one trade, in occurrence order.
    ///
    /// Returns `Some(bar)` if this trade completed a bar, `None` if the trade
    /// only extended the in-progress bar. A trade is an atomic market event and
    /// is never split across bars (see the boundary rule the threshold builder
    /// documents).
    ///
    /// One bar at most: a rule whose single print can complete several
    /// overrides [`push_into`](BarBuilder::push_into), and that is the call
    /// to make wherever such a rule can be configured.
    fn push(&mut self, trade: &Trade) -> Option<Bar>;

    /// Feed one trade, in occurrence order, and append every bar it closed to
    /// `closed`, oldest first.
    ///
    /// The call the shared aggregator path makes — chart, backtest and bot.
    /// The default is [`push`](BarBuilder::push): at most one bar per print,
    /// so a rule that never closes two cuts exactly what it always cut. A rule
    /// whose one print can complete several bars overrides it: a Renko print
    /// clearing `k` brick levels closes `k` bricks, and the ones it cleared on
    /// its way past hold none of its prints; the print that shows a Renko
    /// builder its price step closes every brick of the prints it held.
    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) {
        closed.extend(self.push(trade));
    }

    /// The in-progress bar — the trades seen since the last close — or `None`
    /// if no trade has arrived since the last bar closed.
    ///
    /// This bar is *not* closed: its `close`/`close_time` reflect only the
    /// trades so far and will keep moving until the bucket fills. Consumers that
    /// need finalised bars only should use what
    /// [`push_into`](BarBuilder::push_into) hands out; `partial` is for
    /// rendering the forming bar.
    fn partial(&self) -> Option<&Bar>;

    /// How far the in-progress bar is from closing, in this rule's measure.
    ///
    /// `None` — the default — when the rule runs toward no fixed threshold, as
    /// an adaptive rule does. Reporting a countdown that is not the rule would
    /// tell the reader the bar closes at a moment it will not.
    fn progress(&self) -> Option<BarProgress> {
        None
    }

    /// The optional deal-counter input port. `None` means this rule is driven
    /// entirely by prints.
    fn deal_counter_input(&mut self) -> Option<&mut dyn DealCounterInput> {
        None
    }

    /// Prints this builder could not place in any bar because the rule had
    /// nothing to count them against — a deal bar before the first counter
    /// reading — prints it holds until it can measure them, and prints off
    /// the grid it froze. All zero for a rule that cuts every print as it
    /// arrives.
    ///
    /// Reported rather than hidden: a chart owes the trader the number of
    /// prints it is showing no bar for.
    fn diagnostics(&self) -> BarBuilderDiagnostics {
        BarBuilderDiagnostics::default()
    }
}
