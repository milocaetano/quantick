//! The strategy port: the one seam a trading rule docks into.
//!
//! There is deliberately no `Signal`, `Rule` or `Entry` abstraction beside
//! it. A strategy is asked one question — *this bar just closed; what do you
//! want to do?* — and answers with the simulator's own [`Command`] vocabulary.
//! Everything it may look at travels in the [`BarView`] it is handed, so a
//! strategy cannot reach for state the harness would not be able to give a
//! live bot later.
//!
//! # Reading indicators
//!
//! A strategy declares the indicators it needs in [`Strategy::indicators`];
//! the harness registers them in the host **in that order** and hands the
//! readings back under the same indices. Every read returns `f64`, and `NaN`
//! is `na` — warmup, a conditional plot, a slot that does not exist. Test
//! `is_nan()` before trusting a number; a comparison against `NaN` is always
//! false, which silently means "no signal" rather than a wrong one.

use quantick_engine::Bar;
use quantick_indicators::{Indicator, IndicatorHost, InstanceId, PlotId};
use quantick_sim::{Command, Order, Position, VenueEvent};
use rust_decimal::Decimal;

/// A trading rule the harness can run.
///
/// Implementations live in [`crate::strategies`] (and, from WP-03, in the
/// operational rule crate). A test double is a legitimate implementation too:
/// `tests/harness.rs` docks one to prove the port takes an implementation it
/// has never heard of.
pub trait Strategy {
    /// Name printed in the report header, so a run says which rule produced
    /// the numbers.
    fn name(&self) -> &str;

    /// The indicators this strategy reads, fresh for one session.
    ///
    /// Called once per session, before the first print, because indicator
    /// state must not leak from one recorded day into the next. The returned
    /// order is the slot order used by [`Signals`].
    ///
    /// The default is "none" — a strategy may read nothing but price.
    fn indicators(&mut self) -> Vec<Box<dyn Indicator>> {
        Vec::new()
    }

    /// A bar just closed. Return zero or more commands, applied in order.
    ///
    /// Commands take effect from the **next** print onwards: this bar has
    /// already happened. A strategy that returns nothing is the common case
    /// and costs nothing — an empty `Vec` does not allocate.
    fn on_bar(&mut self, view: &BarView<'_>) -> Vec<Command>;

    /// What the simulator reported — fills, placements, rejections,
    /// closures — in tape order: once after each print, and once after the
    /// commands a bar's [`Strategy::on_bar`] issued were applied. The live
    /// chart feeds its armed instances the same stream, so a strategy that
    /// follows its own operation (the `force-region` kernel does) behaves
    /// identically under both consumers.
    ///
    /// The returned commands are the strategy protecting itself mid-print —
    /// the kernel closes an operation whose bracket the simulator dropped
    /// at fill time. The run loop applies them immediately; applying such a
    /// command emits no events of its own, so one echo settles it. The
    /// default returns nothing, which is exactly right for the stateless
    /// signal-followers.
    fn on_events(&mut self, events: &[VenueEvent]) -> Vec<Command> {
        let _ = events;
        Vec::new()
    }

    /// Called once after the last print of a session, before the next one.
    /// The default does nothing; a stateful rule resets here.
    fn end_of_session(&mut self) {}
}

/// Everything a strategy may look at when a bar closes.
pub struct BarView<'a> {
    /// The bar that just closed — the same `Bar` the chart would render.
    pub bar: &'a Bar,
    /// Its index among this session's closed bars, 0-based. Also the row
    /// index of every indicator reading for this bar.
    pub index: usize,
    /// Committed indicator columns, in the order [`Strategy::indicators`]
    /// declared them.
    pub signals: Signals<'a>,
    /// What the simulator holds right now.
    pub account: Account<'a>,
}

/// Read access to the committed indicator columns.
///
/// Every accessor answers `NaN` instead of panicking when asked for something
/// that does not exist: an out-of-range slot, an undeclared plot, a row
/// before warmup finished. `PlotBuffer::column` panics on a bad plot id; this
/// wrapper exists so a strategy bug is a missing signal, not a crashed run
/// halfway through a session library.
#[derive(Clone, Copy)]
pub struct Signals<'a> {
    host: &'a IndicatorHost,
    slots: &'a [InstanceId],
}

impl<'a> Signals<'a> {
    pub(crate) fn new(host: &'a IndicatorHost, slots: &'a [InstanceId]) -> Self {
        Self { host, slots }
    }

    /// How many indicator slots this strategy declared.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// True when the strategy declared no indicators.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Committed rows so far — one per closed bar the host has evaluated.
    #[must_use]
    pub fn rows(&self) -> usize {
        self.host.bar_count()
    }

    /// One committed cell, or `NaN` when it does not exist.
    #[must_use]
    pub fn value(&self, slot: usize, plot: PlotId, row: usize) -> f64 {
        let Some(id) = self.slots.get(slot) else {
            return f64::NAN;
        };
        let Some(plots) = self.host.plots(*id) else {
            return f64::NAN;
        };
        if plot.index() >= plots.plot_count() {
            return f64::NAN;
        }
        plots.value(plot, row)
    }

    /// The most recently committed cell of `plot`, or `NaN`.
    #[must_use]
    pub fn latest(&self, slot: usize, plot: PlotId) -> f64 {
        match self.rows().checked_sub(1) {
            Some(row) => self.value(slot, plot, row),
            None => f64::NAN,
        }
    }

    /// `latest` shifted `bars_ago` bars into the past (`0` is `latest`), or
    /// `NaN` when the history is not that long yet.
    #[must_use]
    pub fn back(&self, slot: usize, plot: PlotId, bars_ago: usize) -> f64 {
        match self.rows().checked_sub(bars_ago + 1) {
            Some(row) => self.value(slot, plot, row),
            None => f64::NAN,
        }
    }
}

/// The simulator's state, as a strategy sees it.
///
/// Read-only by construction: a strategy changes the account by returning
/// commands, never by reaching into it.
#[derive(Clone, Copy)]
pub struct Account<'a> {
    /// The open net position, if any.
    pub position: Option<&'a Position>,
    /// Resting limit/stop entries, in placement order.
    pub orders: &'a [Order],
    /// Price of the last print seen — the only "now" that exists here.
    pub mark_price: Option<Decimal>,
    /// Venue time of that print.
    pub mark_timestamp_ms: Option<i64>,
    /// Round trips closed so far in this session.
    pub closed_trades: usize,
    /// Realized result so far, in points (price units × quantity).
    pub realized_points: Decimal,
}

impl Account<'_> {
    /// True when there is no open position and nothing resting — the state a
    /// rule usually requires before opening a new trade.
    ///
    /// The simulator's queue of market actions is not consulted because it is
    /// always empty here: `on_trade` drains it, and the harness runs
    /// `on_trade` on a print before the bar that print closed is handed over.
    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.position.is_none() && self.orders.is_empty()
    }
}
