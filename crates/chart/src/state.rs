//! Chart state: trades in (backfill + live), bars out — for any bar type.
//!
//! This is the app's side of the "one engine, four consumers" boundary. It
//! retains every trade and feeds them through whichever [`BarBuilder`] the user
//! has selected; switching the bar type rebuilds the bars from the retained
//! trades through a freshly configured builder — the same deterministic engine
//! code path, just a different measure. It also records where backfilled data
//! ends and live data begins so the two can be labelled honestly.
//!
//! No egui, no async here, so the ingest, dispatch and rebuild logic is
//! unit-tested in CI.

pub use quantick_engine::ImbalanceUnit;
use quantick_engine::trade_tape::TradeTape;
use quantick_engine::{Bar, BarBuilder, BarFootprint, BarProgress, DealSample, PriceGrid, Trade};
/// The bar vocabulary lives in the engine, one definition for the chart, the
/// backtest and the bot. Re-exported so the chart's callers keep finding it
/// here, with the imbalance unit beside it.
pub use quantick_engine::{BarKind, BarSpec, MAX_TIME_INTERVAL_MS, MIN_TIME_INTERVAL_MS};
use rust_decimal::Decimal;

use crate::footprint_series::{self, FootprintSeries};

fn seed_deal_counter(builder: &mut dyn BarBuilder, samples: &[DealSample]) {
    let Some(input) = builder.deal_counter_input() else {
        return;
    };
    for sample in samples {
        input.observe(*sample);
    }
}

/// Any UI `f64` as a positive [`Decimal`].
///
/// Two kinds of number pass through here and they are not the same kind: a bar
/// rule's threshold from the toolbar, and — via `pane::strategy_region` — a
/// drawing's anchor *prices*. The floor below is its own constant for exactly
/// that reason: it is the positivity floor this conversion has always applied,
/// not the engine's bar-parameter floor
/// ([`DECIMAL_PARAM_FLOOR`](quantick_engine::DECIMAL_PARAM_FLOOR)), and tuning
/// that floor must not silently retune where a strategy region's bounds land.
///
/// The price case is why that floor is a wart rather than a guard: a region
/// drawn on a negative-valued band (a CVD) has both bounds collapsed onto it.
/// Pre-existing and outside this change; written down here so the next reader
/// finds it rather than trusting the constant's name.
#[must_use]
pub fn dec_from_f64(x: f64) -> Decimal {
    use rust_decimal::prelude::FromPrimitive as _;
    /// The smallest positive `Decimal` this conversion produces. A separate
    /// number from the engine's
    /// [`DECIMAL_PARAM_FLOOR`](quantick_engine::DECIMAL_PARAM_FLOOR), which it
    /// happens to equal: they answer to different callers.
    const POSITIVE_FLOOR: Decimal = Decimal::from_parts(1, 0, 0, false, 8);
    Decimal::from_f64(x)
        .unwrap_or(Decimal::ONE)
        .max(POSITIVE_FLOOR)
}

pub use quantick_engine::bar_registry::BarConfiguration;
/// The engine owns retained selection and rebuild debounce policy.
pub use quantick_engine::bar_selection::BarSelection as SpecSelector;

/// Milliseconds per drag point, shared by both controls so the same gesture
/// moves the same amount wherever it is made.
///
/// Kept at the fine end of the domain — the BARS group's existing feel —
/// because that is where dragging is the right gesture. Crossing to the
/// coarse end is what the time pane's presets and click-to-type are for, and
/// a speed that made an hour a short drag would make a second unreachable.
pub use quantick_engine::bar_registry::TIME_INTERVAL_DRAG_SPEED;

/// The bars derived from the retained trade stream, plus the backfill/live
/// boundary, for the currently selected [`BarSpec`].
pub struct ChartState {
    spec: BarConfiguration,
    /// O(1) identity of the current temporal bar partition.
    timeline_revision: u64,
    /// O(1) identity of the closed bars *and their ladders* — see
    /// [`Self::series_revision`].
    series_revision: u64,
    builder: Box<dyn BarBuilder>,
    trades: TradeTape,
    backfill_trade_count: usize,
    backfill_done: bool,
    bars: Vec<Bar>,
    partial: Option<Bar>,
    backfill_boundary: Option<usize>,
    /// Per-bar footprint ladders, index-aligned with `bars`; fed the same
    /// trades the bar builder folds (see [`FootprintSeries`]).
    footprints: FootprintSeries,
    /// The price grid the tape prints on, folded from every trade this chart
    /// has seen. Unconditional: it is a fact about the market, not about
    /// whether a layer that draws it is switched on, and a consumer sizing
    /// rows needs it before anything is drawn.
    price_grid: PriceGrid,
    /// The first price this chart ever saw, and never a later one.
    ///
    /// The row-sizing rule needs the magnitude a market trades at as well as
    /// the grid it trades on, and this is the chart's own answer to the first
    /// half. Deliberately frozen at the first print rather than tracking the
    /// last: the sizing it feeds re-buckets and re-folds every retained ladder
    /// when it changes, so a number that drifted with the tape would refold a
    /// long session repeatedly to say almost the same thing. The heatmap's
    /// snapshot path is frozen the same way, by only auto-sizing while its
    /// history is still empty.
    ///
    /// First *seen*, which is not first in time: paging older history in
    /// through [`prepend_history`](Self::prepend_history) feeds prints that
    /// predate this one and deliberately leaves it standing. A market's
    /// magnitude is the same an hour earlier, and re-grouping a chart because
    /// the trader scrolled left would be a refold with nothing to show for it.
    tape_reference_price: Option<Decimal>,
    /// Whether the ladders are being accumulated at all. Off (the default)
    /// costs nothing per trade and holds nothing per bar — a capability
    /// nobody asked for must not tax every ingest. Enabling refolds the
    /// retained trades, so nothing is lost by having been off.
    footprint_enabled: bool,
    /// Every reading of the venue's deal counter this chart has seen,
    /// ascending by time, retained beside the trades for the same reason
    /// they are: a rebuild replays both through a fresh builder. Empty on
    /// every feed without a counter, so it costs nothing there.
    deal_samples: Vec<DealSample>,
    /// Readings that landed in the series behind the newest without
    /// reaching the live builder: the bars were cut without them until the
    /// next rebuild, so anything replaying the series meanwhile — the
    /// footprint refold — leaves them out too, and cuts where the bars are.
    readings_held: bool,
}

/// Push one print through `builder` and, with the footprint layer on, fold
/// it into the ladders — unless the builder left it *uncounted*. A deal bar
/// counts nothing before its first reading: such a print belongs to no bar,
/// so it belongs to no ladder either, or the ladders drift off the bars they
/// index by and the footprint series asserts on the first close.
fn fold_print<B: BarBuilder + ?Sized>(
    builder: &mut B,
    footprints: &mut FootprintSeries,
    footprint_enabled: bool,
    trade: &Trade,
) -> Option<Bar> {
    let uncounted_before = builder.diagnostics().uncounted_trades;
    let closed = builder.push(trade);
    if footprint_enabled {
        let uncounted = builder.diagnostics().uncounted_trades != uncounted_before;
        match (&closed, uncounted) {
            (_, false) => footprints.observe(trade, closed.as_ref()),
            // A rollover ended the bar and this print counts for nothing:
            // the ladder closes on what it held, the print folds nowhere.
            (Some(bar), true) => footprints.close_without(bar),
            (None, true) => {}
        }
    }
    closed
}

impl ChartState {
    /// A fresh chart building bars per `spec`.
    #[must_use]
    pub fn new(spec: impl Into<BarConfiguration>) -> Self {
        let spec = spec.into();
        let builder = spec.build();
        Self {
            spec,
            timeline_revision: 0,
            series_revision: 0,
            builder,
            trades: TradeTape::new(),
            backfill_trade_count: 0,
            backfill_done: false,
            bars: Vec::new(),
            partial: None,
            backfill_boundary: None,
            footprints: FootprintSeries::new(footprint_series::default_group()),
            price_grid: PriceGrid::new(),
            tape_reference_price: None,
            footprint_enabled: false,
            deal_samples: Vec::new(),
            readings_held: false,
        }
    }

    /// Hand the chart one reading of the venue's deal counter.
    ///
    /// Retained and fed to the builder in one motion: a deal builder joins
    /// the prints that follow to it, every other builder ignores it, and a
    /// later rebuild — a spec switch, a page of history — replays the
    /// retained series ahead of the trades so the join is the same one.
    /// The live feed delivers samples in time order and they append; a
    /// recorded day loaded behind the live tape is older, and lands at its
    /// place in the series instead — the next rebuild replays the whole
    /// series in order, which is how those readings reach the bars. A
    /// reading already held is not held twice.
    pub fn observe_deals(&mut self, sample: DealSample) {
        match self.deal_samples.last() {
            Some(last) if sample.time_ms < last.time_ms => {
                // Older than the newest held — a bridge that restarted with
                // another clock offset. The live builder does not place it
                // (the engine drops a sample going back in time), and the
                // live edge is not re-cut for it either: a clock that went
                // backwards can deliver one such reading per poll for an
                // hour, and a rebuild per reading would freeze the chart.
                // It lands at its place in the series, which the next
                // rebuild replays in order — held once, whichever of the
                // readings sharing its millisecond it equals. Its place is
                // by time *and* reading, the order the batch path sorts
                // into. A lower reading at the newest millisecond is fed as
                // any other: the builder reads a small dip as a late poll.
                // After every reading of its millisecond — the series is
                // ordered by time alone, and a run of one millisecond keeps
                // arrival order — unless the run already holds it.
                let at = self
                    .deal_samples
                    .partition_point(|held| held.time_ms <= sample.time_ms);
                let same_ms = self.deal_samples[..at]
                    .iter()
                    .rev()
                    .take_while(|held| held.time_ms == sample.time_ms);
                if !same_ms.into_iter().any(|held| *held == sample) {
                    self.deal_samples.insert(at, sample);
                    self.readings_held = true;
                }
            }
            Some(last) if *last == sample => {}
            _ => {
                self.deal_samples.push(sample);
                if let Some(input) = self.builder.deal_counter_input() {
                    input.observe(sample);
                }
            }
        }
    }

    /// Hand the chart a whole series of readings at once — a recorded day,
    /// a resumed file. One sort and one dedup over the union, never an
    /// insert per reading: a morning's file against an afternoon's live
    /// readings is half a million inserts into a hundred thousand, each
    /// shifting the rest.
    ///
    /// The builder is not fed here. A pane cutting by deals is rebuilt by
    /// the caller straight after, and every other rule replays the retained
    /// series on its next switch.
    pub fn observe_deals_batch(&mut self, samples: &[DealSample]) {
        if samples.is_empty() {
            return;
        }
        self.deal_samples.extend_from_slice(samples);
        // By time, and stable: two readings can share a millisecond (ticks
        // do, across poll rounds), and the live builder saw them in arrival
        // order — a lower one after a higher one is a late poll it ignored.
        // Sorted by reading as well, a rebuild would replay the lower first
        // and read the higher as a window, cutting where the live edge did
        // not. A pair the union holds twice is harmless the same way: the
        // repeat is a dip or an unchanged reading, and neither is a window.
        self.deal_samples.sort_by_key(|sample| sample.time_ms);
        // A reading the union holds twice — the file batched over a live
        // series that already has it — is held once, wherever inside its
        // millisecond's run the repeat landed; the order of the run stays.
        let mut kept: Vec<DealSample> = Vec::with_capacity(self.deal_samples.len());
        for sample in self.deal_samples.drain(..) {
            let repeat = kept
                .iter()
                .rev()
                .take_while(|held| held.time_ms == sample.time_ms)
                .any(|held| *held == sample);
            if !repeat {
                kept.push(sample);
            }
        }
        self.deal_samples = kept;
    }

    /// Start the series over under `spec`, keeping the counter readings.
    ///
    /// They are the venue's history, not this series': a feed reload or a
    /// replay seek replays prints that join to the same readings the first
    /// pass joined to, and dropping them would leave the morning uncounted
    /// wherever nothing on disk holds them — REC off, or a day not yet
    /// flushed.
    pub fn reset_series(&mut self, spec: impl Into<BarConfiguration>) {
        let readings = std::mem::take(&mut self.deal_samples);
        *self = Self::new(spec);
        // Into the fresh builder too, ahead of the prints that will come:
        // the retained series is what a rebuild replays, and the live path
        // feeds the builder as each reading arrives.
        seed_deal_counter(&mut *self.builder, &readings);
        self.deal_samples = readings;
    }

    /// Cut the bars again from the retained prints and readings — after a
    /// recorded day's readings were loaded under prints already folded.
    pub fn rebuild_bars(&mut self) {
        self.rebuild();
    }

    /// The counter readings this chart holds, ascending by time.
    #[must_use]
    pub fn deal_samples(&self) -> &[DealSample] {
        &self.deal_samples
    }

    /// Prints the current rule could place in no bar — a deal bar's prints
    /// before its first counter reading. Zero for every other rule.
    #[must_use]
    pub fn uncounted_trades(&self) -> u64 {
        self.builder.diagnostics().uncounted_trades
    }

    /// Ingest backfilled history (a slice, or another chart's tape) as one
    /// batch — once, before any live trades — then mark the boundary.
    pub fn ingest_backfill<'a>(&mut self, trades: impl IntoIterator<Item = &'a Trade> + Clone) {
        self.trades.extend(trades.clone());
        for trade in trades.clone() {
            self.observe_price(trade.price);
        }
        self.backfill_trade_count = self.trades.len();
        self.backfill_done = true;
        for trade in trades {
            let closed = fold_print(
                &mut *self.builder,
                &mut self.footprints,
                self.footprint_enabled,
                trade,
            );
            if let Some(bar) = closed {
                self.bars.push(bar);
            }
        }
        self.backfill_boundary = Some(self.bars.len());
        self.refresh_partial();
        self.bump_series_revision();
    }

    /// Prepend older backfilled history to the front of the retained stream.
    ///
    /// `trades` must be strictly older than everything already retained (the
    /// feed guarantees this by paging backward from the earliest known
    /// `agg_id`). Because count-based bars (tick/volume/dollar) are grouped from
    /// the first trade, adding older trades re-aligns every bar — so this rebuilds
    /// the whole series through the same deterministic engine path rather than
    /// pretending the existing bars are untouched (data-honesty rule). The
    /// backfill/live boundary is recomputed. Returns how many net bars were added
    /// so the caller can keep the visible window steady.
    pub fn prepend_history(&mut self, trades: &[Trade]) -> usize {
        if trades.is_empty() {
            return 0;
        }
        let bars_before = self.bars.len();
        // Older prints are evidence about the grid like any other. Skipping
        // them would leave a chart that paged left into a busier stretch
        // drawing wide rows over history visibly trading finer, with only a
        // future live print able to correct it — and on a paused replay or a
        // closed market that print never comes.
        for trade in trades {
            self.observe_price(trade.price);
        }
        self.trades.prepend(trades);
        self.backfill_trade_count += trades.len();
        self.rebuild();
        self.bars.len().saturating_sub(bars_before)
    }

    /// Ingest one live trade, incrementally (no full rebuild).
    pub fn ingest_live(&mut self, trade: &Trade) {
        self.trades.push(trade.clone());
        self.observe_price(trade.price);
        let closed = fold_print(
            &mut *self.builder,
            &mut self.footprints,
            self.footprint_enabled,
            trade,
        );
        if let Some(bar) = closed {
            self.bars.push(bar);
        }
        self.refresh_partial();
        // Live ingest only ever *appends*: no bar already closed changes, and
        // no ladder already built is touched. So the series identity stands
        // and a consumer folding a fixed set of closed bars keeps its work —
        // see [`Self::series_revision`].
        self.bump_timeline_revision();
    }

    /// Switch the bar type/parameter, rebuilding all bars from the retained
    /// trades. A no-op if `spec` is unchanged.
    pub fn set_spec(&mut self, spec: impl Into<BarConfiguration>) {
        let spec = spec.into();
        if spec == self.spec {
            return;
        }
        self.spec = spec;
        self.rebuild();
    }

    /// Fold one print into everything the chart learns from prices alone: the
    /// grid the tape prints on, and the magnitude it prints at.
    ///
    /// One function rather than two calls at each ingest site, so a fourth
    /// ingest path cannot pick up the grid and quietly forget the magnitude.
    ///
    /// Per-trade, and both halves are cheap — a modulo on the settled grid, and
    /// an `Option` test that stores once per chart.
    fn observe_price(&mut self, price: Decimal) {
        self.price_grid.observe(price);
        self.tape_reference_price.get_or_insert(price);
    }

    /// Replay every retained trade through a fresh builder for the current spec,
    /// recomputing the bars and the backfill/live boundary.
    fn rebuild(&mut self) {
        self.readings_held = false;
        let mut builder = self.spec.build();
        // Readings first, prints after: the builder joins each print to the
        // newest reading strictly before it, so the order between the two
        // streams is immaterial as long as every reading is in hand.
        seed_deal_counter(&mut *builder, &self.deal_samples);
        let mut bars = Vec::new();
        let mut boundary = None;
        self.footprints.reset(self.footprints.base_group());
        for (i, trade) in self.trades.iter().enumerate() {
            if self.backfill_done && i == self.backfill_trade_count {
                boundary = Some(bars.len());
            }
            let closed = fold_print(
                &mut *builder,
                &mut self.footprints,
                self.footprint_enabled,
                trade,
            );
            if let Some(bar) = closed {
                bars.push(bar);
            }
        }
        // Backfill covered every retained trade (no live yet): boundary is the
        // end of the bar list.
        if self.backfill_done && boundary.is_none() {
            boundary = Some(bars.len());
        }
        self.partial = builder.partial().cloned();
        self.builder = builder;
        self.bars = bars;
        self.backfill_boundary = boundary;
        self.bump_series_revision();
    }

    fn refresh_partial(&mut self) {
        self.partial = self.builder.partial().cloned();
    }

    fn bump_timeline_revision(&mut self) {
        self.timeline_revision = self.timeline_revision.saturating_add(1);
    }

    /// The closed bars changed: one closed, history was prepended, or the
    /// series was rebuilt. Always bumps the timeline too — a change to the
    /// closed bars is a change to the timeline; never the other way round.
    fn bump_series_revision(&mut self) {
        self.series_revision = self.series_revision.saturating_add(1);
        self.bump_timeline_revision();
    }

    /// Monotonic identity for order-flow projections keyed by these bar bounds.
    #[must_use]
    pub fn timeline_revision(&self) -> u64 {
        self.timeline_revision
    }

    /// Monotonic identity of the closed bars **and their ladders** — the
    /// inputs a range fold reads.
    ///
    /// It moves when those inputs are *rebuilt or shifted*: a spec change, a
    /// page of history prepended, a footprint refold, the initial backfill.
    /// It deliberately does **not** move when a print lands or a bar closes,
    /// because live ingest only appends: no bar already closed changes, and no
    /// ladder already built is touched, so a fold over a fixed set of bars is
    /// still valid.
    ///
    /// That is the whole difference from
    /// [`timeline_revision`](Self::timeline_revision), which answers "did
    /// anything about the bars move" and steps on every single print. Keying
    /// a kept fold on *that* is what made a range profile over a long history
    /// restart tens of times a second and never finish.
    #[must_use]
    pub fn series_revision(&self) -> u64 {
        self.series_revision
    }

    /// The current bar spec.
    #[must_use]
    pub fn spec(&self) -> &BarConfiguration {
        &self.spec
    }

    /// The closed bars.
    #[must_use]
    pub fn bars(&self) -> &[Bar] {
        &self.bars
    }

    /// The forming (in-progress) bar, if any.
    #[must_use]
    pub fn partial(&self) -> Option<&Bar> {
        self.partial.as_ref()
    }

    /// The number of purely-backfilled bars (the backfill/live divider index).
    #[must_use]
    pub fn backfill_boundary(&self) -> Option<usize> {
        self.backfill_boundary
    }

    /// One footprint ladder per closed bar, same indices as [`Self::bars`].
    #[must_use]
    pub fn bar_footprints(&self) -> &[BarFootprint] {
        self.footprints.closed()
    }

    /// The forming bar's footprint ladder, the counterpart of
    /// [`Self::partial`].
    #[must_use]
    pub fn partial_footprint(&self) -> Option<&BarFootprint> {
        self.footprints.partial()
    }

    /// The price magnitude this chart's tape trades at — the first price it
    /// showed. See [the field](Self::tape_reference_price) for why the first
    /// and not the latest.
    ///
    /// [`None`] only before the first print. Sizing rows needs this beside
    /// [`tape_price_step`](Self::tape_price_step): a tick is a true fact about
    /// an instrument and still a useless row on a market whose price dwarfs
    /// it — BTCUSDT quotes in cents near $80k, and a profile at cent rows is
    /// tens of thousands of them.
    #[must_use]
    pub fn tape_reference_price(&self) -> Option<Decimal> {
        self.tape_reference_price
    }

    /// The price grid this chart's tape prints on, once enough prints have
    /// shown one.
    ///
    /// [`None`] means the tape has not said yet — a chart that has seen one
    /// print, or a run of prints all at one price. It never means "the grid is
    /// fine": a caller sizing rows from this keeps whatever it had until an
    /// answer arrives.
    #[must_use]
    pub fn tape_price_step(&self) -> Option<Decimal> {
        self.price_grid.step()
    }

    /// The row width footprints are captured at. Rendering reads the width
    /// off each ladder ([`BarFootprint::group`]); this is the capture side of
    /// that round trip — what the range-profile cache keys on to notice a
    /// refold, and what the tests assert against.
    #[must_use]
    pub fn footprint_group(&self) -> Decimal {
        self.footprints.base_group()
    }

    /// Switch footprint accumulation on or off. Off is free; turning it on
    /// refolds the retained trades so the ladders appear fully populated,
    /// and turning it off drops them (the trades can always rebuild them).
    /// A no-op when nothing changes.
    pub fn set_footprint_enabled(&mut self, enabled: bool) {
        if enabled == self.footprint_enabled {
            return;
        }
        self.footprint_enabled = enabled;
        if enabled {
            self.refold_footprints(self.footprints.base_group());
        } else {
            self.footprints.reset(self.footprints.base_group());
            self.bump_series_revision();
        }
    }

    /// Re-capture the footprints at row width `group` (normally the
    /// instrument's `price_step` once the feed reports it), replaying the
    /// retained trades through a scratch builder of the current spec so the
    /// ladders re-align with the very same bar boundaries. The bars — and the
    /// timeline revision projections key on — are untouched. A no-op when the
    /// group is unchanged or not positive; while accumulation is off only
    /// the width is stored, for the refold that happens on enable.
    pub fn set_footprint_group(&mut self, group: Decimal) {
        if group <= Decimal::ZERO || group == self.footprints.base_group() {
            return;
        }
        if self.footprint_enabled {
            self.refold_footprints(group);
        } else {
            self.footprints.reset(group);
            self.bump_series_revision();
        }
    }

    /// Replay every retained trade through a scratch builder of the current
    /// spec, rebuilding the ladders on `group`-wide rows against the very
    /// same bar boundaries the real builder produced.
    fn refold_footprints(&mut self, group: Decimal) {
        // The ladders are rebuilt wholesale, so anything folding them is
        // reading different inputs from this point on.
        self.bump_series_revision();
        self.footprints.reset(group);
        // Readings held for the next rebuild: this is one. The ladders must
        // close where the bars are, and a bar cut without a reading the
        // series now holds is a bar the next rebuild would move anyway.
        if self.readings_held {
            self.rebuild();
            return;
        }
        let mut builder = self.spec.build();
        // Readings first, as `rebuild` does: a deal bar with no readings
        // counts every print as uncounted, and the ladders would be empty
        // under bars that are not.
        seed_deal_counter(&mut *builder, &self.deal_samples);
        for trade in &self.trades {
            fold_print(&mut *builder, &mut self.footprints, true, trade);
        }
    }

    /// Every trade this chart still holds, oldest first — what a rebuild
    /// replays, and what a second view of the same market is seeded from.
    #[must_use]
    pub fn trades(&self) -> &TradeTape {
        &self.trades
    }

    /// How many of [`Self::trades`] arrived as backfilled history rather than
    /// live. Seeding another chart needs this: a trade that was streamed live
    /// must not become "history" just because the second view opened late.
    #[must_use]
    pub fn backfill_trade_count(&self) -> usize {
        self.backfill_trade_count
    }

    /// When the bar in chart slot `index` opened — a closed bar, or the forming
    /// bar in the slot right after them (the chart draws it as one more).
    ///
    /// Bar indices are only meaningful for one spec at a time; the market time
    /// under them survives a rebuild, so this is how a caller remembers where
    /// the user was looking across one. `None` for a slot the series has not
    /// got.
    #[must_use]
    pub fn slot_open_time(&self, index: usize) -> Option<i64> {
        quantick_engine::bar_timeline::BarTimeline::new(&self.bars, self.partial.as_ref())
            .slot_open_time(index)
    }

    /// The slot showing market time `timestamp_ms`: the newest bar that opened
    /// at or before it, or slot 0 when the whole series is younger than it.
    ///
    /// Bars are pushed in trade order, so their open times are non-decreasing
    /// and this is a binary search. The forming bar counts as the slot after
    /// the closed ones, matching [`Self::slot_open_time`]. `None` when there is
    /// no series to point into.
    #[must_use]
    pub fn slot_at_time(&self, timestamp_ms: i64) -> Option<usize> {
        quantick_engine::bar_timeline::BarTimeline::new(&self.bars, self.partial.as_ref())
            .slot_at_time(timestamp_ms)
    }

    /// How far the forming bar is from closing, in the rule's own measure, and
    /// the unit to print it in.
    ///
    /// Straight from the builder that owns the closing rule — the chart never
    /// re-derives "a tick bar closes at N". `None` for a rule with no fixed
    /// threshold to count toward.
    #[must_use]
    pub fn progress(&self) -> Option<(BarProgress, &'static str)> {
        Some((self.builder.progress()?, self.spec.kind().progress_unit()))
    }
}

#[cfg(test)]
mod bar_spec_parity_tests;

#[cfg(test)]
mod tape_identity_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_engine::Side;
    use std::str::FromStr as _;

    fn dec(s: &str) -> Decimal {
        Decimal::from_str(s).unwrap()
    }

    fn trade(agg_id: u64) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64 * 100,
            price: dec("100"),
            quantity: dec("1.0"),
            side: Side::Buy,
        }
    }

    /// A print at `price`, with everything else fixed — these tests are about
    /// the price grid and nothing else.
    fn print_at(agg_id: u64, price: &str) -> Trade {
        Trade {
            agg_id,
            timestamp_ms: 1000 + agg_id as i64 * 100,
            price: dec(price),
            quantity: dec("1.0"),
            side: Side::Buy,
        }
    }

    #[test]
    fn a_chart_names_the_grid_its_own_tape_prints_on() {
        // B3's mini index moves in five-point steps. Nothing states that here:
        // the chart is told only the prints, exactly as a market replay tells
        // it, and the grid is what it works out.
        let mut chart = ChartState::new(BarSpec::Tick(1000));
        for (i, price) in [
            "174565", "174570", "174560", "174570", "174585", "174580", "174570", "174575",
            "174590", "174585",
        ]
        .iter()
        .enumerate()
        {
            chart.ingest_live(&print_at(i as u64, price));
        }
        assert_eq!(chart.tape_price_step(), Some(dec("5")));
    }

    #[test]
    fn a_chart_names_the_price_its_tape_trades_at_and_then_holds_still() {
        // The other half of sizing a row. A tick is a true fact about an
        // instrument and still a useless row where the price dwarfs it, so the
        // rule needs the magnitude too — and it has to hold still, because
        // every change to it re-buckets and re-folds every retained ladder.
        let mut chart = ChartState::new(BarSpec::Tick(1000));
        assert_eq!(chart.tape_reference_price(), None, "nothing has printed");

        chart.ingest_live(&print_at(0, "80000"));
        assert_eq!(chart.tape_reference_price(), Some(dec("80000")));

        // A session that runs to a very different price is still the same
        // market at the same magnitude, and re-grouping it mid-session would
        // be a refold that says almost nothing new.
        for (i, price) in ["81000", "84000", "88000"].iter().enumerate() {
            chart.ingest_live(&print_at(i as u64 + 1, price));
        }
        assert_eq!(
            chart.tape_reference_price(),
            Some(dec("80000")),
            "the magnitude drifted with the tape instead of holding at the first print",
        );
    }

    #[test]
    fn backfilled_history_names_the_price_as_well_as_the_grid() {
        // History arrives as one batch before the first live print, and it is
        // what a chart's very first screen is drawn from. A magnitude learned
        // only from live prints would size that screen from nothing.
        let mut chart = ChartState::new(BarSpec::Tick(1000));
        chart.ingest_backfill(&[print_at(0, "140000"), print_at(1, "140005")]);
        assert_eq!(chart.tape_reference_price(), Some(dec("140000")));
    }

    #[test]
    fn a_chart_that_has_not_been_told_enough_names_nothing() {
        // Silence is not an answer of "fine" — a caller sizing rows from this
        // has to keep what it had, and that only works if it can tell the two
        // apart.
        let mut chart = ChartState::new(BarSpec::Tick(1000));
        assert_eq!(chart.tape_price_step(), None);
        for i in 0..20 {
            chart.ingest_live(&print_at(i, "174565"));
        }
        assert_eq!(
            chart.tape_price_step(),
            None,
            "a run of prints at one price shows no distance and so no grid",
        );
    }

    #[test]
    fn backfilled_history_names_the_grid_as_well_as_live_prints_do() {
        // History arrives as one batch before the first live print. A chart
        // that only learned the grid from live prints would draw its first
        // screen — the whole backfill — on the wrong rows.
        let mut chart = ChartState::new(BarSpec::Tick(1000));
        let history: Vec<Trade> = [
            "5216.0", "5216.5", "5217.0", "5216.5", "5215.5", "5217.5", "5218.0", "5217.0",
            "5216.0", "5215.5",
        ]
        .iter()
        .enumerate()
        .map(|(i, price)| print_at(i as u64, price))
        .collect();
        chart.ingest_backfill(&history);
        assert_eq!(chart.tape_price_step(), Some(dec("0.5")));
    }

    /// A recorded day loaded behind the live tape is older than the live
    /// readings: it lands in order and the rebuild cuts from the whole
    /// series, while a reading delivered twice is held once.
    /// A reading older than the newest held is kept for the next rebuild,
    /// not cut in at once — the live edge is never re-cut per reading —
    /// and is held once however many readings share its millisecond.
    /// A reading at the newest held millisecond with a *lower* count is a
    /// regression the sampler forwards — a late poll. The builder reads a
    /// small dip as exactly that, live and rebuilt alike: no bar ends.
    #[test]
    fn a_lower_reading_at_the_same_millisecond_is_a_late_poll_live_and_rebuilt() {
        let mut s = ChartState::new(BarSpec::Trades(2));
        let at = |time_ms: i64, session_deals: u64| DealSample {
            time_ms,
            session_deals,
        };
        s.observe_deals(at(1_099, 5));
        s.observe_deals(at(1_099, 4));
        s.ingest_live(&trade(1));
        assert!(s.bars().is_empty(), "no rollover bar: {:?}", s.bars());
        assert_eq!(
            s.deal_samples().len(),
            2,
            "held, for the file and the rebuild"
        );
        s.rebuild_bars();
        assert!(s.bars().is_empty(), "the rebuild agrees: {:?}", s.bars());
    }

    /// A rollover that ends the bar on a print the builder leaves uncounted
    /// — last night's bar, this morning's first print — closes the ladder
    /// on what it held and folds the print nowhere, so ladders and bars
    /// keep the same indices.
    #[test]
    fn a_rollover_on_an_uncounted_print_keeps_the_ladders_aligned() {
        let mut s = ChartState::new(BarSpec::Trades(1_000));
        s.set_footprint_enabled(true);
        // Two readings around the first print give the rate (100 per
        // contract); the second print is credited it and forms a bar.
        s.observe_deals(DealSample {
            time_ms: 999,
            session_deals: 5_000_300,
        });
        s.ingest_live(&trade(1));
        s.observe_deals(DealSample {
            time_ms: 1_199,
            session_deals: 5_000_400,
        });
        s.ingest_live(&trade(2));
        assert_eq!(
            s.partial_footprint().map(|_| ()),
            Some(()),
            "a ladder is forming"
        );
        // The session restarts, and the next print is far behind the
        // reading that ended it — beyond what a reading holds for.
        s.observe_deals(DealSample {
            time_ms: 1_299,
            session_deals: 3,
        });
        s.ingest_live(&Trade {
            agg_id: 3,
            timestamp_ms: 1_300 + 11 * 60_000,
            price: dec("100"),
            quantity: dec("1.0"),
            side: Side::Buy,
        });
        assert_eq!(s.bars().len(), 1, "last night's bar ended");
        assert_eq!(s.bar_footprints().len(), 1, "and its ladder with it");
        assert_eq!(s.uncounted_trades(), 2, "the first print, and this one");
        assert!(s.partial_footprint().is_none(), "nothing opened the next");
    }

    /// A lower reading at the same millisecond, batched in behind a live
    /// series that already saw the higher one, is replayed in arrival order:
    /// the rebuild ignores the dip as the live edge did.
    #[test]
    fn a_same_millisecond_dip_batched_in_cuts_as_the_live_edge_did() {
        let at = |time_ms: i64, session_deals: u64| DealSample {
            time_ms,
            session_deals,
        };
        // Prints at 1100..1400; a reading between each pair, and at 1 299
        // the higher reading first, the dip after it.
        let mut live = ChartState::new(BarSpec::Trades(1_000));
        live.observe_deals(at(999, 1_000));
        live.ingest_live(&trade(1));
        live.observe_deals(at(1_199, 1_500));
        live.ingest_live(&trade(2));
        live.observe_deals(at(1_299, 2_600));
        live.observe_deals(at(1_299, 1_700));
        live.ingest_live(&trade(3));
        live.ingest_live(&trade(4));
        assert_eq!(live.bars().len(), 3, "{:?}", live.bars());

        let mut rebuilt = ChartState::new(BarSpec::Trades(1_000));
        rebuilt.observe_deals_batch(live.deal_samples());
        rebuilt.ingest_backfill(&[trade(1), trade(2), trade(3), trade(4)]);
        rebuilt.rebuild_bars();
        assert_eq!(rebuilt.bars(), live.bars());
        assert_eq!(rebuilt.uncounted_trades(), live.uncounted_trades());
    }

    /// A reading held for the next rebuild reaches the ladders only with
    /// the bars: a refold meanwhile leaves it out, and the ladders close
    /// where the bars are.
    #[test]
    fn a_refold_with_a_held_reading_rebuilds_the_bars_too() {
        let mut s = ChartState::new(BarSpec::Trades(2));
        s.set_footprint_enabled(true);
        let at = |time_ms: i64, session_deals: u64| DealSample {
            time_ms,
            session_deals,
        };
        s.observe_deals(at(1_099, 1));
        s.observe_deals(at(1_299, 6));
        for id in 1..=3 {
            s.ingest_live(&trade(id));
        }
        s.observe_deals(at(1_199, 4)); // held
        s.set_footprint_group(dec("2"));
        assert_eq!(s.bar_footprints().len(), s.bars().len());
    }

    #[test]
    fn an_out_of_order_reading_is_held_for_the_next_rebuild() {
        let mut s = ChartState::new(BarSpec::Trades(2));
        let at = |time_ms: i64, session_deals: u64| DealSample {
            time_ms,
            session_deals,
        };
        s.observe_deals(at(1_099, 1));
        s.observe_deals(at(1_299, 6));
        for id in 1..=3 {
            s.ingest_live(&trade(id));
        }
        assert_eq!(s.bars().len(), 1, "the print at 1 300 sees 6 and closes 2");
        // The reading the bridge had missed: 1 199, at 4. Held, not cut in.
        s.observe_deals(at(1_199, 4));
        assert_eq!(s.bars().len(), 1, "the live edge is not re-cut");
        assert_eq!(s.deal_samples().len(), 3);
        // A reading re-delivered is held once, whatever else shares its
        // millisecond.
        s.observe_deals(at(1_099, 1));
        s.observe_deals(at(1_099, 1));
        assert_eq!(s.deal_samples().len(), 3);
        // The next rebuild: the print at 1 200 crosses 4 and closes 2, the
        // one at 1 300 crosses 6.
        s.rebuild_bars();
        assert_eq!(s.bars().len(), 2, "{:?}", s.bars());
    }

    #[test]
    fn older_readings_land_in_order_and_duplicates_are_held_once() {
        let mut s = ChartState::new(BarSpec::Trades(2));
        let at = |time_ms: i64, session_deals: u64| DealSample {
            time_ms,
            session_deals,
        };
        s.observe_deals(at(1_500, 10));
        s.observe_deals(at(1_500, 10));
        s.observe_deals(at(1_100, 3));
        s.observe_deals(at(1_300, 6));
        s.observe_deals(at(1_100, 3));
        let times: Vec<i64> = s.deal_samples().iter().map(|d| d.time_ms).collect();
        assert_eq!(times, [1_100, 1_300, 1_500]);
        // A whole file behind the live readings: one sort, the same series.
        s.observe_deals_batch(&[at(1_300, 6), at(1_000, 1), at(1_200, 4)]);
        let times: Vec<i64> = s.deal_samples().iter().map(|d| d.time_ms).collect();
        assert_eq!(times, [1_000, 1_100, 1_200, 1_300, 1_500]);
        // Two readings at one millisecond, held live and batched again from
        // the file: each pair folds onto itself, in reading order, so the
        // rebuild never sees the lower one return.
        s.observe_deals(at(1_600, 12));
        s.observe_deals(at(1_600, 14));
        s.observe_deals_batch(&[at(1_600, 12), at(1_600, 14)]);
        let tail: Vec<(i64, u64)> = s
            .deal_samples()
            .iter()
            .skip(5)
            .map(|d| (d.time_ms, d.session_deals))
            .collect();
        assert_eq!(tail, [(1_600, 12), (1_600, 14)]);
        // Prints at 1100..1500 (see `trade`): after a rebuild the first —
        // under the first window, no rate yet — is uncounted, and the
        // readings cut three bars from the rest.
        for id in 1..=5 {
            s.ingest_live(&trade(id));
        }
        s.rebuild_bars();
        assert_eq!(s.uncounted_trades(), 1);
        assert_eq!(s.bars().len(), 3, "{:?}", s.bars());
    }

    /// Recording belongs to the asset: the readings a tab retains survive
    /// every switch of the bar rule, so `tick → trades → tick → trades`
    /// cuts the same deal bars each time, and the prints before the first
    /// reading are counted rather than folded into a bar nobody cut.
    /// A series reset — a feed reload, a replay seek — keeps the readings:
    /// the prints replayed afterwards join to them as the first pass did.
    /// A print a deal bar leaves uncounted belongs to no bar, so it belongs
    /// to no ladder either: the footprint series stays aligned with the bars
    /// instead of drifting — and asserting — on the first print before a
    /// reading. Found by the trader's own workspace, footprint on.
    #[test]
    fn uncounted_prints_form_no_footprint_ladder() {
        let mut s = ChartState::new(BarSpec::Trades(2_000));
        s.set_footprint_enabled(true);
        s.ingest_backfill(&[trade(1), trade(2)]);
        assert_eq!(s.uncounted_trades(), 2);
        assert!(
            s.partial_footprint().is_none(),
            "nothing folds before a reading"
        );
        s.observe_deals(DealSample {
            time_ms: 1_299,
            session_deals: 3_990,
        });
        s.ingest_live(&trade(3));
        // The window completes at 9 deals over one contract: the next
        // print is credited 9 and crosses 4 000.
        s.observe_deals(DealSample {
            time_ms: 1_399,
            session_deals: 3_999,
        });
        s.ingest_live(&trade(4));
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.bars()[0].trade_count, 1);
        assert_eq!(
            s.uncounted_trades(),
            3,
            "two before any reading, one before a rate"
        );
        assert_eq!(s.bar_footprints().len(), 1);
        // A refold and a rebuild keep the alignment too.
        s.set_footprint_group(dec("2"));
        assert_eq!(s.bar_footprints().len(), 1);
        s.set_spec(BarSpec::Tick(2));
        s.set_spec(BarSpec::Trades(2_000));
        assert_eq!(s.bar_footprints().len(), s.bars().len());
    }

    #[test]
    fn a_series_reset_keeps_the_readings() {
        let mut s = ChartState::new(BarSpec::Trades(2_000));
        s.observe_deals(DealSample {
            time_ms: 1_000,
            session_deals: 3_990,
        });
        s.observe_deals(DealSample {
            time_ms: 1_150,
            session_deals: 3_999,
        });
        s.ingest_live(&trade(1));
        s.ingest_live(&trade(2));
        assert_eq!(
            s.bars().len(),
            1,
            "the print at 1 200 is credited the window's 9 and crosses 4 000"
        );

        s.reset_series(BarSpec::Trades(2_000));
        assert!(s.bars().is_empty(), "the series is gone");
        assert_eq!(s.deal_samples().len(), 2, "the readings are not");
        s.ingest_backfill(&[trade(1), trade(2)]);
        assert_eq!(s.bars().len(), 1, "the replayed prints cut the same bar");
    }

    #[test]
    fn deal_readings_survive_a_switch_of_the_bar_rule() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        // Prints at 1100, 1200, ... (see `trade`); readings just ahead of
        // the prints at 1300 and 1500, as the feed sends them.
        s.ingest_backfill(&[trade(1), trade(2)]);
        s.observe_deals(DealSample {
            time_ms: 1299,
            session_deals: 3_990,
        });
        s.ingest_live(&trade(3));
        s.ingest_live(&trade(4));
        s.observe_deals(DealSample {
            time_ms: 1499,
            session_deals: 4_003,
        });
        s.ingest_live(&trade(5));
        assert_eq!(s.bars().len(), 2, "two tick bars of two prints");
        assert_eq!(
            s.uncounted_trades(),
            0,
            "a tick rule counts nothing as uncounted"
        );

        s.set_spec(BarSpec::Trades(2_000));
        assert!(
            s.bars().is_empty(),
            "4 003 plus one print's estimate reaches no multiple"
        );
        assert_eq!(
            s.partial().map(|bar| bar.trade_count),
            Some(1),
            "the print at 1500, credited the first window's rate"
        );
        assert_eq!(
            s.uncounted_trades(),
            4,
            "two before the first reading, two before the first rate"
        );
        assert_eq!(s.deal_samples().len(), 2);

        s.set_spec(BarSpec::Tick(2));
        assert_eq!(s.bars().len(), 2);
        s.set_spec(BarSpec::Trades(2_000));
        assert_eq!(
            s.uncounted_trades(),
            4,
            "the readings were retained across both switches"
        );
        // Prints with a rate advance the countdown in estimated deals, not
        // prints: the window's 13 deals over two contracts credit the print
        // at 1500 six and a half, past the re-anchored 4 003.
        let (progress, unit) = s.progress().expect("a deal bar runs toward a fixed count");
        assert_eq!(unit, "deals");
        // From where the forming bar opened — the re-anchored 4 003 — not
        // from the previous multiple.
        assert_eq!(progress.done, dec("6.5"));
    }

    #[test]
    fn backfill_and_live_go_through_the_same_builder() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2), trade(3)]);
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.backfill_boundary(), Some(1));

        s.ingest_live(&trade(4));
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(1), "boundary does not move");
    }

    #[test]
    fn switching_bar_type_rebuilds_from_retained_trades() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        let trades: Vec<Trade> = (1..=6).map(trade).collect();
        s.ingest_backfill(&trades); // tick(2): 6 trades -> 3 bars
        assert_eq!(s.bars().len(), 3);

        s.set_spec(BarSpec::Tick(3)); // rebuild: 6 trades -> 2 bars
        assert_eq!(s.bars().len(), 2);
        assert_eq!(
            s.backfill_boundary(),
            Some(2),
            "all six are backfill -> boundary at the end"
        );
    }

    #[test]
    fn timeline_revision_tracks_ingest_prepend_and_rebuilds() {
        let mut state = ChartState::new(BarSpec::Tick(2));
        assert_eq!(state.timeline_revision(), 0);

        state.ingest_backfill(&(5..=8).map(trade).collect::<Vec<_>>());
        assert_eq!(state.timeline_revision(), 1);
        state.ingest_live(&trade(9));
        assert_eq!(state.timeline_revision(), 2);
        state.prepend_history(&(1..=4).map(trade).collect::<Vec<_>>());
        assert_eq!(state.timeline_revision(), 3);

        state.set_spec(BarSpec::Tick(2));
        assert_eq!(state.timeline_revision(), 3, "an unchanged spec is a no-op");
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(state.timeline_revision(), 4);
    }

    /// The series identity is the *narrow* question — "were the bars and
    /// ladders a fold reads rebuilt?" — and it has to stay narrow. Live
    /// ingest only appends, so neither a print nor a bar closing on the right
    /// edge changes a bar that a range already folded; a consumer keeping that
    /// fold must not be told otherwise, or a long range restarts on every tick
    /// and never finishes.
    #[test]
    fn series_revision_ignores_prints_and_closes_and_tracks_rebuilds() {
        let mut state = ChartState::new(BarSpec::Tick(2));
        assert_eq!(state.series_revision(), 0);

        // Two trades: the first opens the forming bar, the second closes it.
        // Neither rewrites a bar that was already there.
        state.ingest_live(&trade(1));
        state.ingest_live(&trade(2));
        assert_eq!(state.bars().len(), 1, "a bar did close");
        assert_eq!(
            state.series_revision(),
            0,
            "appending never rewrites what was already folded"
        );
        assert_eq!(
            state.timeline_revision(),
            2,
            "the timeline moved on both prints all the same"
        );

        // Every way the series is genuinely rebuilt does move it.
        state.ingest_backfill(&(5..=8).map(trade).collect::<Vec<_>>());
        assert_eq!(state.series_revision(), 1, "the backfill rebuilt the bars");
        state.prepend_history(&(3..=4).map(trade).collect::<Vec<_>>());
        assert_eq!(state.series_revision(), 2, "so does a prepended page");
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(state.series_revision(), 3, "and a new bar rule");
        state.set_spec(BarSpec::Tick(3));
        assert_eq!(state.series_revision(), 3, "an unchanged spec is a no-op");
        state.set_footprint_enabled(true);
        assert_eq!(
            state.series_revision(),
            4,
            "switching the ladders on rebuilds them, which is a fold's inputs"
        );
        state.set_footprint_group(dec("2"));
        assert_eq!(state.series_revision(), 5, "and so does a refold");
    }

    #[test]
    fn boundary_is_recomputed_across_a_switch() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2), trade(3)]); // 1 bar + partial
        s.ingest_live(&trade(4)); // closes bar 2 (backfill 3 + live 4)
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(1));

        // tick(4): 4 trades -> 1 bar. The first 3 (backfill) close 0 bars.
        s.set_spec(BarSpec::Tick(4));
        assert_eq!(s.bars().len(), 1);
        assert_eq!(s.backfill_boundary(), Some(0));
    }

    #[test]
    fn prepend_history_adds_older_bars_and_keeps_boundary() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(5), trade(6), trade(7), trade(8)]); // 2 bars
        s.ingest_live(&trade(9)); // opens a partial, still 2 closed bars
        assert_eq!(s.bars().len(), 2);
        assert_eq!(s.backfill_boundary(), Some(2));

        // Pull in the four older trades 1..=4.
        let added = s.prepend_history(&[trade(1), trade(2), trade(3), trade(4)]);
        // tick(2) over 1..=8 backfill = 4 closed bars; trade 9 is the partial.
        assert_eq!(s.bars().len(), 4);
        assert_eq!(added, 2, "two net bars were prepended");
        assert_eq!(
            s.backfill_boundary(),
            Some(4),
            "all eight retained backfill trades are history"
        );
    }

    #[test]
    fn prepend_empty_history_is_a_noop() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2)]);
        let before = s.bars().len();
        let added = s.prepend_history(&[]);
        assert_eq!(added, 0);
        assert_eq!(s.bars().len(), before);
    }

    /// Bar indices mean a different thing per spec; market time does not. This
    /// is the lookup that carries the user's position across a rebuild.
    #[test]
    fn a_slot_is_found_by_the_market_time_it_shows() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        // trade(n) is stamped at 1000 + n*100, so tick(2) bars open at 1100,
        // 1300, 1500 and the partial (trade 7) at 1700.
        let trades: Vec<Trade> = (1..=7).map(trade).collect();
        s.ingest_backfill(&trades);
        assert_eq!(s.bars().len(), 3);
        assert!(s.partial().is_some());

        assert_eq!(s.slot_open_time(0), Some(1100));
        assert_eq!(s.slot_open_time(3), Some(1700), "the forming bar's slot");
        assert_eq!(s.slot_open_time(4), None, "no such slot");

        assert_eq!(s.slot_at_time(1300), Some(1), "exactly on an open");
        assert_eq!(s.slot_at_time(1400), Some(1), "inside a bar");
        assert_eq!(s.slot_at_time(9_999), Some(3), "past the end: the newest");
        assert_eq!(s.slot_at_time(0), Some(0), "before the start: the oldest");
    }

    #[test]
    fn an_empty_series_has_no_slot_to_point_at() {
        let s = ChartState::new(BarSpec::Tick(2));
        assert_eq!(s.slot_at_time(1_000), None);
        assert_eq!(s.slot_open_time(0), None);
    }

    /// The rebuild is what makes the lookup necessary: the same market time
    /// lands on a different index once the bars are re-cut.
    #[test]
    fn the_slot_of_a_time_moves_when_the_spec_changes() {
        let mut s = ChartState::new(BarSpec::Tick(1));
        let trades: Vec<Trade> = (1..=8).map(trade).collect();
        s.ingest_backfill(&trades);
        let slot = s.slot_at_time(1500).expect("a slot for trade 5");
        assert_eq!(slot, 4, "tick(1): one bar per trade");

        s.set_spec(BarSpec::Tick(4));
        assert_eq!(s.slot_at_time(1500), Some(1), "tick(4): the second bar");
    }

    #[test]
    fn setting_the_same_spec_is_a_noop() {
        let mut s = ChartState::new(BarSpec::Tick(2));
        s.ingest_backfill(&[trade(1), trade(2)]);
        let before = s.bars().len();
        s.set_spec(BarSpec::Tick(2));
        assert_eq!(s.bars().len(), before);
    }

    /// The first live print after a backfill must not copy the backfill.
    ///
    /// Filled exactly, a contiguous tape made the next push reallocate the
    /// whole loaded session on the UI thread — about 95 MB for a recovered
    /// 1.7 M-print day, one dropped frame the moment the chart went live. The
    /// chunked tape never moves a print it holds, so that push copies
    /// nothing, and it holds at most one chunk it does not use.
    #[test]
    fn the_first_live_print_after_a_backfill_copies_nothing() {
        let history: Vec<Trade> = (5_001..=15_000).map(trade).collect();
        let older: Vec<Trade> = (1..=5_000).map(trade).collect();
        let mut s = ChartState::new(BarSpec::Tick(50));
        let first_live = |s: &mut ChartState, id: u64| {
            let before = crate::work_meter::tally();
            s.ingest_live(&trade(id));
            let copied = crate::work_meter::tally().since(before).realloc_copy_bytes;
            let tape_bytes = (s.trades.len() * std::mem::size_of::<Trade>()) as u64;
            assert!(
                copied < tape_bytes / 2,
                "the first live print copied {copied} bytes of a {tape_bytes}-byte tape"
            );
            assert!(
                s.trades.capacity() - s.trades.len() < quantick_engine::trade_tape::CHUNK_TRADES,
                "no more than one chunk is held unused"
            );
        };
        s.ingest_backfill(&history);
        first_live(&mut s, 15_001);
        // Paging older history in joins a new tape: chunked the same way.
        s.prepend_history(&older);
        first_live(&mut s, 15_002);
    }
}
