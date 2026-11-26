//! The [`BarBuilder`] abstraction shared by every bar type.

use rust_decimal::Decimal;

use crate::{Bar, DealSample, Trade};

/// Optional venue input (the deal counter), kept apart from the print-driven bar contract.
pub trait DealCounterInput {
    fn observe(&mut self, sample: DealSample);
}

/// What a builder could not do with its prints (uncounted, held, off-grid), counted rather than
/// hidden. All zero when every print is cut on arrival.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BarBuilderDiagnostics {
    /// Prints placed in no bar: the rule had nothing to count them against (deal bar before its
    /// first reading).
    pub uncounted_trades: u64,
    /// Prints held in the in-progress bar because the rule cannot measure them yet (Renko before
    /// its step is known). Prints still held when a tape ends were never cut.
    pub held_prints: u64,
    /// Prints off the frozen price grid: cut on that grid and counted, never snapped onto it.
    pub off_grid_prints: u64,
}

/// How far the in-progress bar is from closing. Both figures are in the rule's own measure (trades,
/// quantity, notional or milliseconds), so a consumer can render "37 of 50" without knowing the
/// rule. It comes from the builder that owns the closing rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarProgress {
    /// Measure accumulated by the trades since the last close.
    pub done: Decimal,
    /// Measure at which this bar closes.
    pub target: Decimal,
}

/// Turns a stream of [`Trade`]s into [`Bar`]s; every bar type is a `BarBuilder`.
/// Trades arrive one at a time in occurrence order via [`push_into`](BarBuilder::push_into); trades
/// since the last close form the in-progress bar ([`partial`](BarBuilder::partial)).
pub trait BarBuilder: Send {
    /// Feed one trade, in occurrence order, to a rule that names its own type; `Some(bar)` if it
    /// closed one. A configured `dyn BarBuilder` has no `push` (a print may close several bars):
    /// use `push_into`.
    ///
    /// ```compile_fail
    /// use quantick_engine::{Bar, BarBuilder, Trade, bar_registry::BUILTIN_BARS};
    /// fn first_brick(trade: &Trade) -> Option<Bar> {
    ///     let mut builder = BUILTIN_BARS.parse("renko:50").unwrap().build();
    ///     builder.push(trade)
    /// }
    /// ```
    fn push(&mut self, trade: &Trade) -> Option<Bar>
    where
        Self: Sized;

    /// Feed one trade, in occurrence order, and append every bar it closed to `closed`, oldest
    /// first.
    ///
    /// ```
    /// use quantick_engine::{Bar, BarBuilder, Trade, bar_registry::BUILTIN_BARS};
    /// fn every_brick(trade: &Trade) -> Vec<Bar> {
    ///     let mut builder = BUILTIN_BARS.parse("renko:50").unwrap().build();
    ///     let mut closed = Vec::new();
    ///     builder.push_into(trade, &mut closed);
    ///     closed
    /// }
    /// ```
    ///
    /// Appends what [`push`](BarBuilder::push) returns, except a Renko print clearing `k` levels
    /// closes `k`. Returns how many leading appended bars were cut from prints held earlier
    /// (history, never acted on).
    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) -> usize;

    /// The in-progress bar, or `None` if no trade arrived since the last close. Not closed:
    /// `close`/`close_time` keep moving. Use [`push_into`](BarBuilder::push_into) for finalised
    /// bars.
    fn partial(&self) -> Option<&Bar>;

    /// How far the in-progress bar is from closing, in this rule's measure. `None` (default) when
    /// the rule has no fixed threshold, as an adaptive rule: a false countdown would lie.
    fn progress(&self) -> Option<BarProgress> {
        None
    }

    /// The epoch-millisecond instant at which the in-progress bar's bucket ends. `None` (default)
    /// for a rule not on a clock, or while no bar forms. The caller brings the time.
    fn closes_at_ms(&self) -> Option<i64> {
        None
    }

    /// The signed accumulation an adaptive imbalance rule has run up since its last close (buys
    /// add, sells subtract). `None` (default) for other rules; there is no fixed target, so it is
    /// not a [`BarProgress`].
    fn running_imbalance(&self) -> Option<Decimal> {
        None
    }

    /// The optional deal-counter input port. `None` means the rule is driven entirely by prints.
    fn deal_counter_input(&mut self) -> Option<&mut dyn DealCounterInput> {
        None
    }

    /// Prints this builder could not place in any bar, held until measurable, or off its frozen
    /// grid. Reported rather than hidden. All zero when every print is cut on arrival.
    fn diagnostics(&self) -> BarBuilderDiagnostics {
        BarBuilderDiagnostics::default()
    }

    /// The price step this builder read off its own prints and cuts on (Renko, once its prints show
    /// it). Inferred, never venue-declared. `None` when the rule reads no step, is still reading,
    /// or was told it.
    fn inferred_price_step(&self) -> Option<Decimal> {
        None
    }
}
