//! Renko bars as ProfitChart cuts them: bricks of one price height that close
//! on price movement alone, never on time or volume.
//!
//! # The rule
//!
//! An `N`-tick brick is `S = (N - 1) × step` tall, `step` being the
//! instrument's price step — `50R` on WIN, whose step is 5 points, is 245
//! points. Brick edges sit on whole multiples of `S`, so where the bricks fall
//! does not depend on where the tape happened to start. After a brick closes
//! at `C`, the next one continues to `C ± S` or reverses to `C ∓ 2S`, and it
//! closes only once a print trades at least one step past that level: a touch
//! is not a break. The print that closes a brick opens the next one, so a
//! brick holds the prints that formed it and none that ended it. There is no
//! session reset; an overnight gap is one more move.
//!
//! A print far enough past the level clears several: one print closes a brick
//! per level, and the bricks it cleared on its way past hold none of its
//! prints — no volume, no trades, stamped with the time of the print that
//! cleared them. They record the move the print made, not trades that
//! happened.
//!
//! # A fresh builder
//!
//! With no brick behind it, the `S`-cell holding the first print,
//! `[floor(p / S) × S, +S]`, is the first brick's body, and whichever edge
//! the tape breaks first by a step names its direction.
//!
//! # Bar fields
//!
//! A closed brick's `open` and `close` are its levels; its `high` and `low`
//! are the extremes of its own prints and its body — the wicks ProfitChart
//! draws. The forming brick opens on the last close, its range includes that
//! level, and its close is the last print. Before the first brick there is no
//! level, and the forming bar is the plain fold of the prints.
//!
//! # The step
//!
//! The brick height is a number of price steps, and the builder reads the
//! step off its own prints by the rule the chart's tape grid uses
//! ([`PriceGrid`]): the largest step dividing the distance between any two
//! prints. It holds its prints — they form the in-progress bar — until
//! [`STEP_EVIDENCE_DISTANCES`] distances have shown the step, then cuts them
//! from the first, exactly as a builder told that step at the start would
//! have, and freezes it. A grid that narrowed later would make every brick
//! already shown the wrong height, so it is never followed: a print off the
//! frozen grid is cut on it and counted
//! ([`off_grid_prints`](crate::BarBuilderDiagnostics::off_grid_prints)),
//! never snapped onto it and never a reason to cut again. The chart, the
//! backtest and the bot each build this builder over the same prints, so they
//! cut the same bricks by construction.

use rust_decimal::Decimal;

use crate::{Bar, BarBuilder, BarBuilderDiagnostics, PriceGrid, Trade};

/// How many price distances a builder's prints must show before it freezes
/// its step.
///
/// A running GCD only narrows, so the risk is a step frozen too coarse: every
/// brick a whole multiple too tall for the rest of the tape. The chart's grid
/// reports after eight distances because a coarse answer there costs a
/// regroup; frozen here, it would cost every brick. Measured on WINV26's
/// 2026-10-01 and 2026-10-02 tapes (4,305,342 prints, 2,692,694 distances),
/// from every start: after eight distances 1.8e-4 of starts still read a step
/// coarser than the tick, after sixteen 1.7e-5, and the worst start needed
/// 29. Sixty-four is more than twice the worst, and the tail, falling about
/// tenfold every eight distances, puts a coarse freeze near 1e-11. WIN shows
/// sixty-four distances in under two seconds on average; a quieter market
/// only waits longer for its first brick and loses nothing, because the
/// bricks are cut from the first print held.
pub const STEP_EVIDENCE_DISTANCES: u32 = 64;

/// The most bricks one print may close.
///
/// Thousands of bricks in one trade is a corrupt tape, not a market; past
/// this the ladder stops, and the next print carries on from the last level
/// cleared. Nothing is skipped, and one print cannot exhaust memory.
pub const MAX_BRICKS_PER_PRINT: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
}

/// The forming brick: what [`BarBuilder::partial`] shows, and the extremes
/// of its own prints, which a closed brick's wicks are made of.
#[derive(Debug, Clone)]
struct Forming {
    bar: Bar,
    high: Decimal,
    low: Decimal,
}

impl Forming {
    /// A brick opened by `trade`, on `level` once a brick has closed there.
    fn opened_by(trade: &Trade, level: Option<Decimal>) -> Self {
        let mut bar = Bar::opened_by(trade);
        if let Some(level) = level {
            bar.open = level;
            bar.high = bar.high.max(level);
            bar.low = bar.low.min(level);
        }
        Self {
            bar,
            high: trade.price,
            low: trade.price,
        }
    }

    fn extend(&mut self, trade: &Trade) {
        self.bar.extend(trade);
        self.high = self.high.max(trade.price);
        self.low = self.low.min(trade.price);
    }
}

/// The step bricks are cut on, and the height it makes them.
#[derive(Debug, Clone, Copy)]
struct Measure {
    step: Decimal,
    /// `(N - 1) × step`.
    height: Decimal,
}

impl Measure {
    /// `ticks`-tick bricks on `step`; `None` for a step that is not positive
    /// or a height `Decimal` cannot hold.
    fn of(ticks: u64, step: Decimal) -> Option<Self> {
        let height = Decimal::from(ticks - 1).checked_mul(step)?;
        (step > Decimal::ZERO).then_some(Self { step, height })
    }
}

/// Builds Renko bricks of `N` ticks. See the [module docs](self) for the
/// rule and for where its price step comes from.
#[derive(Debug, Clone)]
pub struct RenkoBarBuilder {
    ticks: u64,
    /// The frozen step and brick height; `None` while the prints are held.
    measure: Option<Measure>,
    /// The grid the held prints have shown, until the step is frozen.
    evidence: PriceGrid,
    /// Every print since the first, while the step is unknown.
    held: Vec<Trade>,
    /// The last closed brick's close level and direction.
    last: Option<(Decimal, Direction)>,
    /// Before the first brick: the lower edge of the cell holding the first
    /// print.
    cell: Option<Decimal>,
    forming: Option<Forming>,
    /// Prints cut on the frozen step although they are off its grid.
    off_grid: u64,
}

impl RenkoBarBuilder {
    /// A builder of `ticks`-tick bricks that reads the price step off its own
    /// prints: what the registry builds for every consumer.
    ///
    /// # Panics
    ///
    /// Panics if `ticks < 2`: a one-tick brick is zero steps tall and would
    /// close on every print. It is a configuration error, not feed input —
    /// the registry refuses it before a builder exists.
    #[must_use]
    pub fn new(ticks: u64) -> Self {
        assert!(
            ticks >= 2,
            "a Renko brick needs at least 2 ticks, got {ticks}"
        );
        Self {
            ticks,
            measure: None,
            evidence: PriceGrid::new(),
            held: Vec::new(),
            last: None,
            cell: None,
            forming: None,
            off_grid: 0,
        }
    }

    /// A builder of `ticks`-tick bricks on a step the caller states rather
    /// than reads, cutting from the first print: the rule apart from the
    /// inference, as a hand-computed golden pins it.
    ///
    /// # Panics
    ///
    /// As [`Self::new`], and if `step` is not positive or `ticks - 1` of it
    /// overflow — a configuration error, not feed input.
    #[must_use]
    pub fn with_step(ticks: u64, step: Decimal) -> Self {
        let mut builder = Self::new(ticks);
        builder.measure =
            Some(Measure::of(ticks, step).expect("a positive step a brick height can hold"));
        builder
    }

    /// The price step the bricks are cut on, once frozen.
    #[must_use]
    pub fn step(&self) -> Option<Decimal> {
        self.measure.map(|measure| measure.step)
    }

    /// The brick height in price, once the step is frozen.
    #[must_use]
    pub fn brick_height(&self) -> Option<Decimal> {
        self.measure.map(|measure| measure.height)
    }

    fn fold(&mut self, trade: &Trade) {
        match &mut self.forming {
            Some(forming) => forming.extend(trade),
            None => self.forming = Some(Forming::opened_by(trade, None)),
        }
    }

    /// The measure the held prints have earned, once they have shown
    /// [`STEP_EVIDENCE_DISTANCES`] distances.
    fn earned_measure(&self) -> Option<Measure> {
        if self.evidence.steps_seen() < STEP_EVIDENCE_DISTANCES {
            return None;
        }
        Measure::of(self.ticks, self.evidence.step()?)
    }

    /// Cut one print on the frozen measure, appending every brick it closes.
    fn cut(&mut self, trade: &Trade, measure: Measure, closed: &mut Vec<Bar>) {
        let Measure { step, height } = measure;
        if !trade
            .price
            .checked_rem(step)
            .is_some_and(|rest| rest.is_zero())
        {
            self.off_grid = self.off_grid.saturating_add(1);
        }
        if self.last.is_none() && self.cell.is_none() {
            self.cell = trade
                .price
                .checked_div(height)
                .and_then(|cells| cells.floor().checked_mul(height));
        }
        let Some((direction, open, close)) = self.first_break(trade.price, height, step) else {
            self.fold(trade);
            return;
        };
        // The forming brick closes on its own prints: this one ends it and
        // opens the next.
        let first = closed.len();
        closed.push(brick(self.forming.take(), open, close, trade.timestamp_ms));
        let mut level = close;
        while closed.len() - first < MAX_BRICKS_PER_PRINT {
            let Some(next) = next_level(level, height, direction)
                .filter(|next| past(trade.price, *next, step, direction))
            else {
                break;
            };
            closed.push(brick(None, level, next, trade.timestamp_ms));
            level = next;
        }
        self.last = Some((level, direction));
        self.forming = Some(Forming::opened_by(trade, Some(level)));
    }

    /// The brick a print at `price` breaks out of the forming one, if any:
    /// its direction, open and close. `None` as well where a level cannot be
    /// represented — a level past `Decimal`'s range is one no print crosses.
    fn first_break(
        &self,
        price: Decimal,
        height: Decimal,
        step: Decimal,
    ) -> Option<(Direction, Decimal, Decimal)> {
        let ((up_open, up_close), (down_open, down_close)) = match self.last {
            None => {
                let low = self.cell?;
                let high = low.checked_add(height)?;
                ((low, high), (high, low))
            }
            Some((close, Direction::Up)) => {
                let below = close.checked_sub(height)?;
                (
                    (close, close.checked_add(height)?),
                    (below, below.checked_sub(height)?),
                )
            }
            Some((close, Direction::Down)) => {
                let above = close.checked_add(height)?;
                (
                    (above, above.checked_add(height)?),
                    (close, close.checked_sub(height)?),
                )
            }
        };
        if past(price, up_close, step, Direction::Up) {
            Some((Direction::Up, up_open, up_close))
        } else if past(price, down_close, step, Direction::Down) {
            Some((Direction::Down, down_open, down_close))
        } else {
            None
        }
    }
}

/// Whether `price` trades at least one `step` past `level`, going `direction`.
fn past(price: Decimal, level: Decimal, step: Decimal, direction: Direction) -> bool {
    match direction {
        Direction::Up => level.checked_add(step).is_some_and(|edge| price >= edge),
        Direction::Down => level.checked_sub(step).is_some_and(|edge| price <= edge),
    }
}

/// The level one brick on from `level`, going `direction`.
fn next_level(level: Decimal, height: Decimal, direction: Direction) -> Option<Decimal> {
    match direction {
        Direction::Up => level.checked_add(height),
        Direction::Down => level.checked_sub(height),
    }
}

/// A closed brick from its levels and the prints it holds — none for a brick
/// a print cleared on its way past, stamped `cleared_at_ms`.
fn brick(own: Option<Forming>, open: Decimal, close: Decimal, cleared_at_ms: i64) -> Bar {
    let (top, bottom) = (open.max(close), open.min(close));
    match own {
        Some(Forming { bar, high, low }) => Bar {
            open,
            high: high.max(top),
            low: low.min(bottom),
            close,
            ..bar
        },
        None => Bar {
            open_time: cleared_at_ms,
            close_time: cleared_at_ms,
            open,
            high: top,
            low: bottom,
            close,
            buy_volume: Decimal::ZERO,
            sell_volume: Decimal::ZERO,
            trade_count: 0,
        },
    }
}

impl BarBuilder for RenkoBarBuilder {
    /// The first brick `trade` closes. One print can close several; every
    /// shared consumer calls [`push_into`](BarBuilder::push_into), and this
    /// exists for the single-bar callers of the trait.
    fn push(&mut self, trade: &Trade) -> Option<Bar> {
        let mut closed = Vec::new();
        self.push_into(trade, &mut closed);
        debug_assert!(
            closed.len() <= 1,
            "one print closed {} Renko bricks; call push_into",
            closed.len()
        );
        closed.into_iter().next()
    }

    fn push_into(&mut self, trade: &Trade, closed: &mut Vec<Bar>) {
        if let Some(measure) = self.measure {
            self.cut(trade, measure, closed);
            return;
        }
        self.evidence.observe(trade.price);
        self.held.push(trade.clone());
        let Some(measure) = self.earned_measure() else {
            self.fold(trade);
            return;
        };
        // The step is known: every print held is cut from the first, as a
        // builder told the step at the start would have cut it, and the step
        // is never read again.
        self.measure = Some(measure);
        self.forming = None;
        for print in std::mem::take(&mut self.held) {
            self.cut(&print, measure, closed);
        }
    }

    fn partial(&self) -> Option<&Bar> {
        self.forming.as_ref().map(|forming| &forming.bar)
    }

    fn diagnostics(&self) -> BarBuilderDiagnostics {
        BarBuilderDiagnostics {
            held_prints: self.held.len() as u64,
            off_grid_prints: self.off_grid,
            ..BarBuilderDiagnostics::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Side;

    fn print(agg_id: u64, price: i64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1_000 + agg_id as i64 * 100,
            price: Decimal::from(price),
            quantity: Decimal::ONE,
            side: Side::Buy,
        }
    }

    /// Every brick `prices` close, on a 1-point step with 3-tick bricks
    /// (2 points tall).
    fn bricks(prices: &[i64]) -> (RenkoBarBuilder, Vec<Bar>) {
        let mut builder = RenkoBarBuilder::with_step(3, Decimal::ONE);
        let mut closed = Vec::new();
        for (i, price) in prices.iter().enumerate() {
            builder.push_into(&print(i as u64, *price), &mut closed);
        }
        (builder, closed)
    }

    fn levels(bar: &Bar) -> (Decimal, Decimal) {
        (bar.open, bar.close)
    }

    #[test]
    fn a_fresh_builder_breaking_down_first_closes_the_cell_downward() {
        // 10 sits in [10, 12]; 9 is a step under its bottom.
        let (_, closed) = bricks(&[10, 9]);
        assert_eq!(closed.len(), 1);
        assert_eq!(levels(&closed[0]), (Decimal::from(12), Decimal::from(10)));
        assert_eq!(closed[0].high, Decimal::from(12), "the body is a wick too");
    }

    #[test]
    fn a_touch_of_the_level_is_not_a_break() {
        // Up 10->12 on 13; 14 touches the next level, 15 trades past it.
        let (_, touched) = bricks(&[10, 13, 14]);
        assert_eq!(touched.len(), 1);
        let (_, broken) = bricks(&[10, 13, 14, 15]);
        assert_eq!(broken.len(), 2);
        assert_eq!(levels(&broken[1]), (Decimal::from(12), Decimal::from(14)));
    }

    #[test]
    fn a_reversal_needs_two_bricks_and_a_step() {
        // After up 10->12, down needs 12 - 4 - 1 = 7: 8 is not enough.
        let (_, held) = bricks(&[10, 13, 8]);
        assert_eq!(held.len(), 1);
        let (_, reversed) = bricks(&[10, 13, 7]);
        assert_eq!(reversed.len(), 2);
        assert_eq!(levels(&reversed[1]), (Decimal::from(10), Decimal::from(8)));
    }

    #[test]
    fn push_reports_a_single_close() {
        let mut builder = RenkoBarBuilder::with_step(3, Decimal::ONE);
        assert!(builder.push(&print(0, 10)).is_none());
        let brick = builder.push(&print(1, 13)).expect("13 breaks the cell");
        assert_eq!(levels(&brick), (Decimal::from(10), Decimal::from(12)));
    }

    #[test]
    fn one_absurd_print_closes_at_most_the_cap_and_the_next_carries_on() {
        let mut builder = RenkoBarBuilder::with_step(3, Decimal::ONE);
        let mut closed = Vec::new();
        builder.push_into(&print(0, 10), &mut closed);
        builder.push_into(&print(1, 10_000_000), &mut closed);
        assert_eq!(closed.len(), MAX_BRICKS_PER_PRINT);
        let last = closed.last().unwrap().close;
        builder.push_into(&print(2, 10_000_000), &mut closed);
        assert_eq!(closed.len(), 2 * MAX_BRICKS_PER_PRINT);
        assert_eq!(
            closed[MAX_BRICKS_PER_PRINT].open, last,
            "the next print continues from the last level cleared"
        );
    }

    #[test]
    fn a_price_at_the_edge_of_decimal_does_not_panic() {
        let mut builder = RenkoBarBuilder::with_step(3, Decimal::ONE);
        let mut closed = Vec::new();
        builder.push_into(&print(0, 10), &mut closed);
        let mut extreme = print(1, 0);
        extreme.price = Decimal::MAX;
        builder.push_into(&extreme, &mut closed);
        extreme.price = Decimal::MIN;
        builder.push_into(&extreme, &mut closed);
        assert!(closed.len() <= 2 * MAX_BRICKS_PER_PRINT);
    }

    #[test]
    fn the_brick_height_is_n_minus_one_steps() {
        let builder = RenkoBarBuilder::with_step(50, Decimal::from(5));
        assert_eq!(builder.brick_height(), Some(Decimal::from(245)));
        assert_eq!(RenkoBarBuilder::new(50).brick_height(), None);
    }

    #[test]
    #[should_panic(expected = "a Renko brick needs at least 2 ticks")]
    fn one_tick_is_refused() {
        let _ = RenkoBarBuilder::new(1);
    }

    /// Every brick `prices` close through a builder that reads its own step.
    fn read(prices: &[i64]) -> (RenkoBarBuilder, Vec<Bar>) {
        let mut builder = RenkoBarBuilder::new(3);
        let mut closed = Vec::new();
        for (i, price) in prices.iter().enumerate() {
            builder.push_into(&print(i as u64, *price), &mut closed);
        }
        (builder, closed)
    }

    #[test]
    fn sixty_three_distances_hold_every_print_and_the_sixty_fourth_reads_the_step() {
        // `STEP_EVIDENCE_DISTANCES` is pinned here, so the counts are written
        // out: phrased against the constant, this would pass at any value.
        // One-point moves inside the cell [100, 102] break no level.
        let mut prices: Vec<i64> = (0..64).map(|i| 100 + i % 2).collect();
        let (held, closed) = read(&prices);
        assert!(closed.is_empty());
        assert_eq!(held.step(), None, "sixty-three distances");
        assert_eq!(held.diagnostics().held_prints, 64);
        assert_eq!(held.partial().map(|bar| bar.trade_count), Some(64));

        prices.push(100);
        let (read, closed) = read(&prices);
        assert!(closed.is_empty(), "the prints still break no level");
        assert_eq!(read.step(), Some(Decimal::ONE), "the sixty-fourth distance");
        assert_eq!(read.diagnostics(), BarBuilderDiagnostics::default());
        assert_eq!(read.partial().map(|bar| bar.trade_count), Some(65));
    }

    #[test]
    fn the_held_prints_are_cut_from_the_first_as_a_told_builder_cuts_them() {
        // A climb of one point a print: the sixty-fifth print shows the step
        // and cuts every brick the climb closed, each holding its own prints.
        let prices: Vec<i64> = (100..=164).collect();
        let (read, closed) = read(&prices);
        let (told, expected) = bricks(&prices);
        assert_eq!(closed.len(), 31);
        assert_eq!(closed, expected);
        assert_eq!(read.partial(), told.partial());
        assert_eq!(closed[0].trade_count, 3, "the cell held 100, 101 and 102");
        assert!(closed[1..].iter().all(|brick| brick.trade_count == 2));
    }

    #[test]
    fn a_print_off_the_frozen_grid_is_counted_and_cut_on_it() {
        // Two-point moves freeze a two-point step: four-point bricks. The
        // one-point print after it is evidence the step was too coarse; it
        // is counted, not followed, so nothing already cut moves.
        let mut prices: Vec<i64> = (0..65).map(|i| 100 + 2 * (i % 2)).collect();
        prices.extend([105, 110]);
        let (read, closed) = read(&prices);
        assert_eq!(read.step(), Some(Decimal::TWO));
        assert_eq!(read.diagnostics().off_grid_prints, 1);
        let mut told = RenkoBarBuilder::with_step(3, Decimal::TWO);
        let mut expected = Vec::new();
        for (i, price) in prices.iter().enumerate() {
            told.push_into(&print(i as u64, *price), &mut expected);
        }
        assert_eq!(closed, expected);
        assert_eq!(levels(&closed[0]), (Decimal::from(100), Decimal::from(104)));
    }
}
