//! Retained trade/deal evidence and the lifecycle derived from it.
//! Retention is unchanged: all supplied trades/readings remain available;
//! rebuild and prepend deliberately refold those inputs without a tape copy.
use crate::{DEFAULT_FOOTPRINT_GROUP, SeriesFold};
use quantick_engine::bar_registry::BarConfiguration;
use quantick_engine::trade_tape::TradeTape;
use quantick_engine::{Bar, BarFootprint, BarProgress, DealSample, PriceGrid, Trade};
use rust_decimal::Decimal;

/// Retained series with index-aligned closed outputs and live provenance.
/// All reads borrow; app rendering, scheduling and financial effects live
/// outside this owner. The streaming fold owns the sole forming bar.
pub struct RetainedSeries {
    spec: BarConfiguration,
    timeline_revision: u64,
    series_revision: u64,
    fold: SeriesFold,
    trades: TradeTape,
    backfill_trade_count: usize,
    backfill_done: bool,
    bars: Vec<Bar>,
    footprints: Vec<BarFootprint>,
    backfill_boundary: Option<usize>,
    price_grid: PriceGrid,
    /// First seen, not earliest in retained time: prepending never changes it.
    tape_reference_price: Option<Decimal>,
    /// Retained while capture is off; no ladder is allocated for this setting.
    footprint_group: Decimal,
    deal_samples: Vec<DealSample>,
    readings_held: bool,
}

impl RetainedSeries {
    /// A fresh chart building bars per `spec`.
    #[must_use]
    pub fn new(spec: impl Into<BarConfiguration>) -> Self {
        let spec = spec.into();
        let fold = SeriesFold::new(spec);
        Self {
            spec,
            fold,
            timeline_revision: 0,
            series_revision: 0,
            trades: TradeTape::new(),
            backfill_trade_count: 0,
            backfill_done: false,
            bars: Vec::new(),
            footprints: Vec::new(),
            backfill_boundary: None,
            price_grid: PriceGrid::new(),
            tape_reference_price: None,
            footprint_group: DEFAULT_FOOTPRINT_GROUP,
            deal_samples: Vec::new(),
            readings_held: false,
        }
    }

    /// Retain one venue counter reading and feed the live builder unless older.
    /// Older evidence is deduplicated and held until the next rebuild; every
    /// rebuild seeds readings before trades through the same engine port.
    pub fn observe_deals(&mut self, sample: DealSample) {
        match self.deal_samples.last() {
            Some(last) if sample.time_ms < last.time_ms => {
                // Preserve equal-millisecond arrival order. Older readings are
                // held, never used to re-cut the live edge per poll; a rebuild
                // replays them. A duplicate within that millisecond is ignored.
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
                self.fold.observe_deals(sample);
            }
        }
    }

    /// Stable-sort and deduplicate a batch into retained readings.
    /// This does not feed the live builder: the caller rebuilds when needed.
    /// One bulk merge avoids repeated insertion shifts for a recorded day.
    pub fn observe_deals_batch(&mut self, samples: &[DealSample]) {
        if samples.is_empty() {
            return;
        }
        self.deal_samples.extend_from_slice(samples);
        // Stable by time only: sorting equal-millisecond readings by value
        // would turn a late lower poll into a new counter window on rebuild.
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

    /// Reset trades/configuration to fresh defaults, retaining venue readings.
    /// Reload and seek need that evidence even when no recording holds it.
    pub fn reset_series(&mut self, spec: impl Into<BarConfiguration>) {
        let readings = std::mem::take(&mut self.deal_samples);
        *self = Self::new(spec);
        // Into the fresh builder too, ahead of the prints that will come:
        // the retained series is what a rebuild replays, and the live path
        // feeds the builder as each reading arrives.
        self.fold.seed_deals(&readings);
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
        self.fold.diagnostics().uncounted_trades
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
            if let Some(closed) = self.fold.push(trade) {
                self.bars.push(closed.bar);
                self.footprints.extend(closed.footprint);
            }
        }
        self.backfill_boundary = Some(self.bars.len());
        self.bump_series_revision();
    }

    /// Prepend strictly older trades, refolding all retained evidence because
    /// count-based boundaries realign. Recomputes history/live provenance;
    /// returns the net added bars so the caller can preserve its viewport.
    pub fn prepend_history(&mut self, trades: &[Trade]) -> usize {
        if trades.is_empty() {
            return 0;
        }
        let bars_before = self.bars.len();
        // Older evidence refines the grid even on paused or closed markets.
        for trade in trades {
            self.observe_price(trade.price);
        }
        self.trades.prepend(trades);
        self.backfill_trade_count += trades.len();
        self.rebuild();
        self.bars.len().saturating_sub(bars_before)
    }

    /// Ingest one live trade, incrementally (no full rebuild).
    #[inline]
    pub fn ingest_live(&mut self, trade: &Trade) {
        self.trades.push(trade.clone());
        self.observe_price(trade.price);
        if let Some(closed) = self.fold.push(trade) {
            self.bars.push(closed.bar);
            self.footprints.extend(closed.footprint);
        }
        // Append-only closes leave earlier bars and ladders unchanged.
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

    /// Update grid and first-seen magnitude together on every ingest path.
    #[inline]
    fn observe_price(&mut self, price: Decimal) {
        self.price_grid.observe(price);
        self.tape_reference_price.get_or_insert(price);
    }

    /// Replay every retained trade through a fresh builder for the current spec,
    /// recomputing the bars and the backfill/live boundary.
    fn rebuild(&mut self) {
        self.readings_held = false;
        let mut fold = if self.fold.footprint_enabled() {
            SeriesFold::with_footprints(self.spec, self.footprint_group)
        } else {
            SeriesFold::new(self.spec)
        };
        fold.seed_deals(&self.deal_samples);
        let mut bars = Vec::new();
        let mut boundary = None;
        self.footprints.clear();
        for (i, trade) in self.trades.iter().enumerate() {
            if self.backfill_done && i == self.backfill_trade_count {
                boundary = Some(bars.len());
            }
            if let Some(closed) = fold.push(trade) {
                bars.push(closed.bar);
                self.footprints.extend(closed.footprint);
            }
        }
        if self.backfill_done && boundary.is_none() {
            boundary = Some(bars.len());
        }
        self.fold = fold;
        self.bars = bars;
        self.backfill_boundary = boundary;
        self.bump_series_revision();
    }

    fn bump_timeline_revision(&mut self) {
        self.timeline_revision = self.timeline_revision.saturating_add(1);
    }

    /// Invalidate historical projections and timeline together; live append
    /// uses only `bump_timeline_revision`, even when it closes a bar.
    fn bump_series_revision(&mut self) {
        self.series_revision = self.series_revision.saturating_add(1);
        self.bump_timeline_revision();
    }

    /// Monotonic identity for order-flow projections keyed by these bar bounds.
    #[must_use]
    pub fn timeline_revision(&self) -> u64 {
        self.timeline_revision
    }

    /// Identity of rebuilt/shifted closed bars and ladders: spec, prepend,
    /// refold and backfill change it. Live append/close does not invalidate
    /// a fixed historical range; only `timeline_revision` moves per print.
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
        self.fold.partial()
    }

    /// The number of purely-backfilled bars (the backfill/live divider index).
    #[must_use]
    pub fn backfill_boundary(&self) -> Option<usize> {
        self.backfill_boundary
    }

    /// One footprint ladder per closed bar, same indices as [`Self::bars`].
    #[must_use]
    pub fn bar_footprints(&self) -> &[BarFootprint] {
        &self.footprints
    }

    /// The forming bar's footprint ladder, the counterpart of
    /// [`Self::partial`].
    #[must_use]
    pub fn partial_footprint(&self) -> Option<&BarFootprint> {
        self.fold.partial_footprint()
    }

    /// First-seen tape price, unaffected by prepend; `None` before any print.
    /// Row sizing combines this magnitude with the observed price grid.
    #[must_use]
    pub fn tape_reference_price(&self) -> Option<Decimal> {
        self.tape_reference_price
    }

    /// Observed tape grid, or `None` until distinct prices establish one.
    /// Unknown does not mean fine: callers retain their current row size.
    #[must_use]
    pub fn tape_price_step(&self) -> Option<Decimal> {
        self.price_grid.step()
    }

    /// Capture row width; each resulting ladder also records its own group.
    #[must_use]
    pub fn footprint_group(&self) -> Decimal {
        self.footprint_group
    }

    /// Enable by refolding retained trades; disable drops all ladders.
    /// Unchanged settings are a no-op; disabled capture allocates no ladder.
    pub fn set_footprint_enabled(&mut self, enabled: bool) {
        if enabled == self.fold.footprint_enabled() {
            return;
        }
        self.fold
            .set_footprint_capture(enabled.then_some(self.footprint_group));
        if enabled {
            self.refold_footprints(self.footprint_group);
        } else {
            self.footprints.clear();
            self.bump_series_revision();
        }
    }

    /// Refold ladders at a positive new width; when disabled, store only it.
    /// Bars stay unchanged unless held readings require rebuilding them too.
    /// Both revision identities advance on a change, including disabled capture.
    pub fn set_footprint_group(&mut self, group: Decimal) {
        if group <= Decimal::ZERO || group == self.footprint_group {
            return;
        }
        if self.fold.footprint_enabled() {
            self.refold_footprints(group);
        } else {
            self.footprint_group = group;
            self.bump_series_revision();
        }
    }

    /// Replay every retained trade through a scratch builder of the current
    /// spec, rebuilding the ladders on `group`-wide rows against the very
    /// same bar boundaries the real builder produced.
    fn refold_footprints(&mut self, group: Decimal) {
        self.bump_series_revision();
        self.footprint_group = group;
        self.footprints.clear();
        if self.readings_held {
            self.rebuild();
            return;
        }
        let mut folded = SeriesFold::with_footprints(self.spec, group);
        folded.seed_deals(&self.deal_samples);
        for trade in &self.trades {
            if let Some(closed) = folded.push(trade) {
                self.footprints.extend(closed.footprint);
            }
        }
        self.fold.adopt_footprint(folded);
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

    /// Opening time of a closed slot or its following forming slot, if present.
    /// Market time survives a rebuild even when slot indices change.
    #[must_use]
    pub fn slot_open_time(&self, index: usize) -> Option<i64> {
        quantick_engine::bar_timeline::BarTimeline::new(&self.bars, self.fold.partial())
            .slot_open_time(index)
    }

    /// Newest slot opening at/before the time, clamped to 0 before the series.
    /// Includes the forming slot; `None` for an empty series.
    #[must_use]
    pub fn slot_at_time(&self, timestamp_ms: i64) -> Option<usize> {
        quantick_engine::bar_timeline::BarTimeline::new(&self.bars, self.fold.partial())
            .slot_at_time(timestamp_ms)
    }

    /// Builder-owned closing progress and registered unit, or `None` when
    /// the rule has no fixed threshold. No consumer re-derives that rule.
    #[must_use]
    pub fn progress(&self) -> Option<(BarProgress, &'static str)> {
        Some((self.fold.progress()?, self.spec.kind().progress_unit()))
    }
    /// Seed a second retained view without relabeling live evidence as history.
    /// The split is clamped to the available tape, including an empty tape.
    /// Readings are retained in the same deferred batch order as a pane seed.
    pub fn seed_from(
        &mut self,
        trades: &TradeTape,
        backfill_count: usize,
        deal_samples: &[DealSample],
    ) {
        self.observe_deals_batch(deal_samples);
        let split = backfill_count.min(trades.len());
        self.ingest_backfill(trades.range(..split));
        for trade in trades.since(split) {
            self.ingest_live(trade);
        }
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
