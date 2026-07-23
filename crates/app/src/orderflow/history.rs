//! Honest, run-length encoded history derived from a synchronized order book.

use std::collections::{BTreeMap, VecDeque};
use std::mem::size_of;

use quantick_engine::{Side, Trade};
use quantick_orderbook::{ApplyOutcome, BookDelta, BookError, BookSide, BookSnapshot, OrderBook};
use rust_decimal::Decimal;

use super::config::HeatmapConfig;

/// Resting side represented by a liquidity run.
pub type RestingSide = BookSide;
/// Aggressor side represented by an execution.
pub type AggressorSide = Side;

/// One constant-quantity interval at one aggregated price bucket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidityRun {
    /// Synchronization generation. Runs never cross a generation boundary.
    pub generation: u64,
    /// Bid or ask liquidity.
    pub side: RestingSide,
    /// Inclusive lower edge of the exact Decimal price bucket.
    pub price_bucket: Decimal,
    /// Sum of displayed quantities inside the bucket.
    pub quantity: Decimal,
    /// Exchange timestamp at which this value became observable.
    pub start_ms: i64,
    /// Timestamp at which it changed; `None` while this is the current value.
    pub end_ms: Option<i64>,
}

/// One aggressive execution retained independently from the resting book.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Aggression {
    /// Aggregate-trade id.
    pub agg_id: u64,
    /// Exchange trade timestamp.
    pub timestamp_ms: i64,
    /// Exact execution price.
    pub price: Decimal,
    /// Exact executed quantity.
    pub quantity: Decimal,
    /// Taker side.
    pub side: AggressorSide,
    /// Book generation current when the trade was observed, if synchronized.
    pub generation: Option<u64>,
}

impl Aggression {
    /// The passive side this execution attempted to consume.
    #[must_use]
    pub fn consumed_side(&self) -> RestingSide {
        match self.side {
            Side::Buy => BookSide::Ask,
            Side::Sell => BookSide::Bid,
        }
    }
}

/// A contiguous interval known to come from one synchronized book generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageSegment {
    /// Monotonic generation supplied by the feed/controller.
    pub generation: u64,
    /// First synchronized exchange timestamp.
    pub start_ms: i64,
    /// End of this generation, or `None` while it remains current.
    pub end_ms: Option<i64>,
}

/// An interval across which the renderer must not connect liquidity runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageGap {
    /// Generation before the gap, if any.
    pub from_generation: Option<u64>,
    /// Generation installed after the gap, once known.
    pub to_generation: Option<u64>,
    /// First timestamp known not to have continuous coverage.
    pub start_ms: i64,
    /// First timestamp covered by the replacement snapshot.
    pub end_ms: Option<i64>,
    /// Stable human-readable diagnostic reason.
    pub reason: String,
}

/// Current synchronization state exposed to the UI/controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryStatus {
    /// No synchronized snapshot has been installed.
    Empty,
    /// A snapshot/delta generation is continuous.
    Synced {
        /// Active generation.
        generation: u64,
        /// Last update id held by the order book.
        last_update_id: Option<u64>,
        /// Latest exchange timestamp recorded for this generation.
        last_event_ms: i64,
    },
    /// Continuity was lost and a fresh snapshot is required.
    Gap {
        /// Last valid generation.
        from_generation: Option<u64>,
        /// Timestamp at which coverage stopped.
        start_ms: i64,
        /// Reason recorded in the coverage gap.
        reason: String,
    },
}

/// Monotonic counters suitable for structured summaries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HistoryCounters {
    /// Successfully installed snapshots.
    pub snapshots: u64,
    /// Non-stale deltas applied.
    pub deltas_applied: u64,
    /// Stale/redundant deltas ignored by the order book.
    pub deltas_stale: u64,
    /// Continuity gaps opened.
    pub gaps: u64,
    /// RLE runs created.
    pub runs_created: u64,
    /// RLE runs finalized.
    pub runs_closed: u64,
    /// Finalized runs removed by retention or capacity.
    pub runs_evicted: u64,
    /// Aggressions accepted.
    pub aggressions_recorded: u64,
    /// Aggressions removed by retention or capacity.
    pub aggressions_evicted: u64,
}

/// Summary of an explicit grouping reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupingReset {
    /// Previous exact bucket width.
    pub previous: Decimal,
    /// Replacement exact bucket width.
    pub current: Decimal,
    /// Finalized and active runs discarded.
    pub dropped_runs: usize,
    /// Aggressions discarded.
    pub dropped_aggressions: usize,
}

/// Failure to preserve an honest history.
#[derive(Debug)]
pub enum HistoryError {
    /// Snapshot/delta rejected by the deterministic order-book core.
    Book(BookError),
    /// A delta arrived before a synchronized snapshot.
    NotSynchronized,
    /// Exchange timestamps must not go backwards within the book stream.
    BackwardsTimestamp {
        /// Last accepted book timestamp.
        last_ms: i64,
        /// Timestamp supplied by the caller.
        got_ms: i64,
    },
    /// Every replacement snapshot must use a strictly newer generation.
    GenerationNotAdvanced {
        /// Last installed generation.
        previous: u64,
        /// Generation supplied by the caller.
        got: u64,
    },
    /// Price bucket width must be strictly positive.
    InvalidPriceGrouping(Decimal),
    /// A normal config update attempted to reinterpret existing price buckets.
    PriceGroupingResetRequired {
        /// Bucket width currently used by retained runs.
        current: Decimal,
        /// Requested replacement bucket width.
        requested: Decimal,
    },
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Book(error) => write!(f, "order-book update rejected: {error}"),
            Self::NotSynchronized => write!(f, "depth delta received before snapshot sync"),
            Self::BackwardsTimestamp { last_ms, got_ms } => {
                write!(f, "book timestamp went backwards: {got_ms} after {last_ms}")
            }
            Self::GenerationNotAdvanced { previous, got } => {
                write!(
                    f,
                    "book generation must advance beyond {previous}, got {got}"
                )
            }
            Self::InvalidPriceGrouping(grouping) => {
                write!(f, "price grouping must be positive, got {grouping}")
            }
            Self::PriceGroupingResetRequired { current, requested } => write!(
                f,
                "price grouping change from {current} to {requested} requires an explicit reset"
            ),
        }
    }
}

impl std::error::Error for HistoryError {}

impl From<BookError> for HistoryError {
    fn from(value: BookError) -> Self {
        Self::Book(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SideKey {
    Bid,
    Ask,
}

impl SideKey {
    fn public(self) -> RestingSide {
        match self {
            Self::Bid => BookSide::Bid,
            Self::Ask => BookSide::Ask,
        }
    }
}

type LevelKey = (SideKey, Decimal);

/// RLE history plus the authoritative current [`OrderBook`].
///
/// A run changes only when the *aggregated bucket total* changes. Moving
/// quantity between exchange levels inside one bucket therefore does not create
/// noise. Closed runs are bounded; active levels are retained separately so a
/// capacity limit can never corrupt the current book view.
#[derive(Debug, Clone)]
pub struct LiquidityHistory {
    config: HeatmapConfig,
    book: OrderBook,
    generation: Option<u64>,
    last_generation: Option<u64>,
    latest_book_ms: Option<i64>,
    archived: VecDeque<LiquidityRun>,
    active: BTreeMap<LevelKey, LiquidityRun>,
    aggressions: VecDeque<Aggression>,
    coverage: VecDeque<CoverageSegment>,
    gaps: VecDeque<CoverageGap>,
    pending_gap: Option<usize>,
    counters: HistoryCounters,
}

impl LiquidityHistory {
    /// Construct empty history from sanitized settings.
    #[must_use]
    pub fn new(config: HeatmapConfig) -> Self {
        Self {
            config: config.sanitized(),
            book: OrderBook::new(),
            generation: None,
            last_generation: None,
            latest_book_ms: None,
            archived: VecDeque::new(),
            active: BTreeMap::new(),
            aggressions: VecDeque::new(),
            coverage: VecDeque::new(),
            gaps: VecDeque::new(),
            pending_gap: None,
            counters: HistoryCounters::default(),
        }
    }

    /// Sanitized immutable configuration.
    #[must_use]
    pub fn config(&self) -> &HeatmapConfig {
        &self.config
    }

    /// Apply non-structural settings and retention limits immediately.
    ///
    /// Changing `price_grouping` is rejected because retained runs are encoded
    /// in the old bucket space. Use
    /// [`reset_price_grouping`](Self::reset_price_grouping) first.
    pub fn update_config(&mut self, config: HeatmapConfig) -> Result<(), HistoryError> {
        let next = config.sanitized();
        if next.price_grouping != self.config.price_grouping {
            return Err(HistoryError::PriceGroupingResetRequired {
                current: self.config.price_grouping,
                requested: next.price_grouping,
            });
        }
        if !next.show_aggressions {
            let dropped = self.aggressions.len() as u64;
            self.aggressions.clear();
            self.counters.aggressions_evicted += dropped;
        }
        self.config = next;
        if let Some(now_ms) = self.latest_book_ms {
            self.prune(now_ms);
        } else if let Some(now_ms) = self.aggressions.back().map(|trade| trade.timestamp_ms) {
            self.prune(now_ms);
        }
        Ok(())
    }

    /// Current deterministic order book.
    #[must_use]
    pub fn book(&self) -> &OrderBook {
        &self.book
    }

    /// Current status for a UI badge or structured summary.
    #[must_use]
    pub fn status(&self) -> HistoryStatus {
        if let Some(index) = self.pending_gap
            && let Some(gap) = self.gaps.get(index)
        {
            return HistoryStatus::Gap {
                from_generation: gap.from_generation,
                start_ms: gap.start_ms,
                reason: gap.reason.clone(),
            };
        }
        match (self.generation, self.latest_book_ms) {
            (Some(generation), Some(last_event_ms)) => HistoryStatus::Synced {
                generation,
                last_update_id: self.book.last_update_id(),
                last_event_ms,
            },
            _ => HistoryStatus::Empty,
        }
    }

    /// Current counters.
    #[must_use]
    pub fn counters(&self) -> HistoryCounters {
        self.counters
    }

    /// Most recent accepted book timestamp.
    #[must_use]
    pub fn latest_book_ms(&self) -> Option<i64> {
        self.latest_book_ms
    }

    /// Oldest timestamp still renderable under the configured retention
    /// window. Long-lived archived runs keep their original metadata; callers
    /// clip them to this floor instead of rewriting the full archive on every
    /// depth update.
    #[must_use]
    pub fn retention_start_ms(&self) -> Option<i64> {
        self.latest_book_ms
            .map(|latest| latest.saturating_sub(self.config.retention_ms))
    }

    /// Number of finalized runs.
    #[must_use]
    pub fn archived_run_count(&self) -> usize {
        self.archived.len()
    }

    /// Number of currently non-zero aggregated levels.
    #[must_use]
    pub fn active_level_count(&self) -> usize {
        self.active.len()
    }

    /// Approximate bytes used by the bounded event history.
    #[must_use]
    pub fn approximate_history_bytes(&self) -> usize {
        self.archived.len() * size_of::<LiquidityRun>()
            + self.aggressions.len() * size_of::<Aggression>()
    }

    /// Finalized runs followed by active runs (`end_ms == None`).
    pub fn runs(&self) -> impl Iterator<Item = &LiquidityRun> {
        self.archived.iter().chain(self.active.values())
    }

    /// Runs intersecting `[start_ms, end_ms)` after applying the retention
    /// floor.
    ///
    /// Closed runs are appended in non-decreasing end-time order, so a binary
    /// search skips history that ended before the visible chart window. Active
    /// runs are always checked because they have no final end timestamp yet.
    pub fn runs_intersecting(
        &self,
        start_ms: i64,
        end_ms: i64,
    ) -> impl Iterator<Item = &LiquidityRun> {
        let effective_start = self
            .retention_start_ms()
            .map_or(start_ms, |retained| start_ms.max(retained));
        let mut low = 0;
        let mut high = self.archived.len();
        while low < high {
            let middle = low + (high - low) / 2;
            let run_end = self
                .archived
                .get(middle)
                .and_then(|run| run.end_ms)
                .unwrap_or(i64::MAX);
            if run_end <= effective_start {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let latest_ms = self.latest_book_ms.unwrap_or(end_ms);
        let valid_window = end_ms > effective_start;
        self.archived
            .iter()
            .skip(low)
            .chain(self.active.values())
            .filter(move |run| {
                let run_end = run.end_ms.unwrap_or(latest_ms);
                valid_window && run.start_ms < end_ms && run_end > effective_start
            })
    }

    /// Retained aggressive executions.
    pub fn aggressions(&self) -> impl Iterator<Item = &Aggression> {
        self.aggressions.iter()
    }

    /// Continuous synchronized intervals.
    pub fn coverage_segments(&self) -> impl Iterator<Item = &CoverageSegment> {
        self.coverage.iter()
    }

    /// Explicit intervals that must remain visually disconnected.
    pub fn coverage_gaps(&self) -> impl Iterator<Item = &CoverageGap> {
        self.gaps.iter()
    }

    /// Install a full snapshot and open `generation`.
    ///
    /// Coverage starts at this call's timestamp, never before it. Replacing an
    /// already synchronized generation automatically inserts a gap unless the
    /// caller already opened one with [`mark_gap`](Self::mark_gap).
    pub fn install_snapshot(
        &mut self,
        timestamp_ms: i64,
        generation: u64,
        snapshot: BookSnapshot,
    ) -> Result<(), HistoryError> {
        self.ensure_timestamp(timestamp_ms)?;
        if let Some(previous) = self.last_generation
            && generation <= previous
        {
            return Err(HistoryError::GenerationNotAdvanced {
                previous,
                got: generation,
            });
        }

        // Validate into a fresh core first. A bad replacement cannot damage the
        // last honest generation.
        let mut next_book = OrderBook::new();
        next_book.install_snapshot(snapshot)?;

        if self.generation.is_some() && self.pending_gap.is_none() {
            let gap_start = self.latest_book_ms.unwrap_or(timestamp_ms);
            self.open_gap(gap_start, "snapshot_replaced".to_owned());
        }

        self.book = next_book;
        self.generation = Some(generation);
        self.last_generation = Some(generation);
        self.latest_book_ms = Some(timestamp_ms);
        self.complete_pending_gap(timestamp_ms, generation);
        self.coverage.push_back(CoverageSegment {
            generation,
            start_ms: timestamp_ms,
            end_ms: None,
        });
        self.start_from_current_book(timestamp_ms, generation);
        self.counters.snapshots += 1;
        self.prune(timestamp_ms);
        Ok(())
    }

    /// Apply one already-sequenced delta through the deterministic core.
    ///
    /// A core rejection opens a gap and clears the live projection. A new
    /// snapshot is then required; no run is silently connected across it.
    pub fn apply_delta(
        &mut self,
        timestamp_ms: i64,
        delta: &BookDelta,
    ) -> Result<ApplyOutcome, HistoryError> {
        self.ensure_timestamp(timestamp_ms)?;
        let Some(generation) = self.generation else {
            return Err(HistoryError::NotSynchronized);
        };

        let outcome = match self.book.apply_delta(delta) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.open_gap(timestamp_ms, "orderbook_apply_error".to_owned());
                return Err(HistoryError::Book(error));
            }
        };

        self.latest_book_ms = Some(timestamp_ms);
        if matches!(outcome, ApplyOutcome::Stale { .. }) {
            self.counters.deltas_stale += 1;
        } else {
            self.reconcile_current_book(timestamp_ms, generation);
            self.counters.deltas_applied += 1;
        }
        self.prune(timestamp_ms);
        Ok(outcome)
    }

    /// Stop the current generation at `timestamp_ms` and require a new snapshot.
    pub fn mark_gap(
        &mut self,
        timestamp_ms: i64,
        reason: impl Into<String>,
    ) -> Result<(), HistoryError> {
        self.ensure_timestamp(timestamp_ms)?;
        self.open_gap(timestamp_ms, reason.into());
        self.prune(timestamp_ms);
        Ok(())
    }

    /// Retain one trade for the aggression overlay without touching the book.
    pub fn record_aggression(&mut self, trade: &Trade) {
        if !self.config.show_aggressions {
            return;
        }
        self.aggressions.push_back(Aggression {
            agg_id: trade.agg_id,
            timestamp_ms: trade.timestamp_ms,
            price: trade.price,
            quantity: trade.quantity,
            side: trade.side,
            generation: self.generation,
        });
        self.counters.aggressions_recorded += 1;
        let prune_at = self.latest_book_ms.map_or(trade.timestamp_ms, |book_ms| {
            book_ms.max(trade.timestamp_ms)
        });
        self.prune(prune_at);
    }

    /// Explicitly discard all history before changing the exact price bucket.
    ///
    /// There is intentionally no ordinary grouping setter: old RLE runs cannot
    /// be reinterpreted under a different bucket width.
    pub fn reset_price_grouping(
        &mut self,
        grouping: Decimal,
    ) -> Result<GroupingReset, HistoryError> {
        if grouping <= Decimal::ZERO {
            return Err(HistoryError::InvalidPriceGrouping(grouping));
        }
        let reset = GroupingReset {
            previous: self.config.price_grouping,
            current: grouping,
            dropped_runs: self.runs().count(),
            dropped_aggressions: self.aggressions.len(),
        };
        self.config.price_grouping = grouping;
        self.book = OrderBook::new();
        self.generation = None;
        self.latest_book_ms = None;
        self.archived.clear();
        self.active.clear();
        self.aggressions.clear();
        self.coverage.clear();
        self.gaps.clear();
        self.pending_gap = None;
        Ok(reset)
    }

    fn ensure_timestamp(&self, timestamp_ms: i64) -> Result<(), HistoryError> {
        if let Some(last_ms) = self.latest_book_ms
            && timestamp_ms < last_ms
        {
            return Err(HistoryError::BackwardsTimestamp {
                last_ms,
                got_ms: timestamp_ms,
            });
        }
        Ok(())
    }

    fn open_gap(&mut self, timestamp_ms: i64, reason: String) {
        if self.pending_gap.is_some() {
            return;
        }
        self.close_all_active(timestamp_ms);
        if let Some(segment) = self.coverage.back_mut()
            && segment.end_ms.is_none()
        {
            segment.end_ms = Some(timestamp_ms);
        }
        let from_generation = self.generation.take();
        self.book = OrderBook::new();
        self.latest_book_ms = Some(timestamp_ms);
        self.gaps.push_back(CoverageGap {
            from_generation,
            to_generation: None,
            start_ms: timestamp_ms,
            end_ms: None,
            reason,
        });
        self.pending_gap = Some(self.gaps.len() - 1);
        self.counters.gaps += 1;
    }

    fn complete_pending_gap(&mut self, timestamp_ms: i64, generation: u64) {
        if let Some(index) = self.pending_gap.take()
            && let Some(gap) = self.gaps.get_mut(index)
        {
            gap.end_ms = Some(timestamp_ms);
            gap.to_generation = Some(generation);
        }
    }

    fn close_all_active(&mut self, timestamp_ms: i64) {
        let active = std::mem::take(&mut self.active);
        for (_, mut run) in active {
            run.end_ms = Some(timestamp_ms);
            self.archive_run(run);
        }
    }

    fn archive_run(&mut self, run: LiquidityRun) {
        let end_ms = run
            .end_ms
            .expect("only finalized liquidity runs are archived");
        debug_assert!(
            self.archived
                .back()
                .and_then(|previous| previous.end_ms)
                .is_none_or(|previous_end| previous_end <= end_ms),
            "archived runs must remain ordered by end time"
        );
        self.archived.push_back(run);
        self.counters.runs_closed += 1;
    }

    fn start_from_current_book(&mut self, timestamp_ms: i64, generation: u64) {
        debug_assert!(self.active.is_empty());
        for (key, quantity) in aggregate_book(&self.book, self.config.price_grouping) {
            self.active.insert(
                key,
                LiquidityRun {
                    generation,
                    side: key.0.public(),
                    price_bucket: key.1,
                    quantity,
                    start_ms: timestamp_ms,
                    end_ms: None,
                },
            );
            self.counters.runs_created += 1;
        }
    }

    fn reconcile_current_book(&mut self, timestamp_ms: i64, generation: u64) {
        let next = aggregate_book(&self.book, self.config.price_grouping);
        let existing: Vec<LevelKey> = self.active.keys().copied().collect();

        for key in existing {
            let changed = next
                .get(&key)
                .is_none_or(|quantity| self.active[&key].quantity != *quantity);
            if changed {
                let mut run = self.active.remove(&key).expect("key came from active");
                run.end_ms = Some(timestamp_ms);
                self.archive_run(run);
            }
        }

        for (key, quantity) in next {
            if self.active.contains_key(&key) {
                continue;
            }
            self.active.insert(
                key,
                LiquidityRun {
                    generation,
                    side: key.0.public(),
                    price_bucket: key.1,
                    quantity,
                    start_ms: timestamp_ms,
                    end_ms: None,
                },
            );
            self.counters.runs_created += 1;
        }
    }

    fn prune(&mut self, now_ms: i64) {
        let cutoff = now_ms.saturating_sub(self.config.retention_ms);
        self.truncate_before(cutoff);

        while self.archived.len() > self.config.max_history_runs {
            let Some(end_ms) = self.archived.front().and_then(|run| run.end_ms) else {
                break;
            };
            self.truncate_before(end_ms);
        }
        while self.aggressions.len() > self.config.max_aggressions {
            self.pop_aggression_front();
        }

        while self.approximate_history_bytes() > self.config.max_history_bytes {
            let run_time = self.archived.front().and_then(|run| run.end_ms);
            let aggression_time = self.aggressions.front().map(|trade| trade.timestamp_ms);
            match (run_time, aggression_time) {
                (Some(run), Some(trade)) if trade < run => self.pop_aggression_front(),
                (Some(run), _) => self.truncate_before(run),
                (None, Some(_)) => self.pop_aggression_front(),
                (None, None) => break,
            }
        }
    }

    fn truncate_before(&mut self, cutoff: i64) {
        while self
            .archived
            .front()
            .and_then(|run| run.end_ms)
            .is_some_and(|end| end <= cutoff)
        {
            self.archived.pop_front();
            self.counters.runs_evicted += 1;
        }
        for run in self.active.values_mut() {
            run.start_ms = run.start_ms.max(cutoff);
        }

        while self
            .aggressions
            .front()
            .is_some_and(|trade| trade.timestamp_ms < cutoff)
        {
            self.pop_aggression_front();
        }

        while self
            .coverage
            .front()
            .and_then(|segment| segment.end_ms)
            .is_some_and(|end| end <= cutoff)
        {
            self.coverage.pop_front();
        }
        if let Some(segment) = self.coverage.front_mut() {
            segment.start_ms = segment.start_ms.max(cutoff);
        }

        while self
            .gaps
            .front()
            .and_then(|gap| gap.end_ms)
            .is_some_and(|end| end <= cutoff)
        {
            self.gaps.pop_front();
            self.pending_gap = self.pending_gap.map(|index| index.saturating_sub(1));
        }
        if let Some(gap) = self.gaps.front_mut() {
            gap.start_ms = gap.start_ms.max(cutoff);
        }
    }

    fn pop_aggression_front(&mut self) {
        if self.aggressions.pop_front().is_some() {
            self.counters.aggressions_evicted += 1;
        }
    }
}

fn aggregate_book(book: &OrderBook, grouping: Decimal) -> BTreeMap<LevelKey, Decimal> {
    let mut grouped = BTreeMap::new();
    for (&price, &quantity) in book.bids() {
        add_bucket(&mut grouped, SideKey::Bid, price, quantity, grouping);
    }
    for (&price, &quantity) in book.asks() {
        add_bucket(&mut grouped, SideKey::Ask, price, quantity, grouping);
    }
    grouped
}

fn add_bucket(
    grouped: &mut BTreeMap<LevelKey, Decimal>,
    side: SideKey,
    price: Decimal,
    quantity: Decimal,
    grouping: Decimal,
) {
    let bucket = (price / grouping).trunc() * grouping;
    *grouped.entry((side, bucket)).or_insert(Decimal::ZERO) += quantity;
}

#[cfg(test)]
mod tests {
    use super::*;
    use quantick_orderbook::{BookCoverage, BookLevel};
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn level(price: &str, quantity: &str) -> BookLevel {
        BookLevel::new(dec(price), dec(quantity)).unwrap()
    }

    fn snapshot(update_id: u64) -> BookSnapshot {
        BookSnapshot::new(
            update_id,
            vec![level("99", "2"), level("100", "3")],
            vec![level("101", "4"), level("102", "5")],
            BookCoverage::Full,
        )
    }

    fn enabled_config() -> HeatmapConfig {
        HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            ..HeatmapConfig::default()
        }
    }

    fn trade(id: u64, timestamp_ms: i64, side: Side) -> Trade {
        Trade {
            agg_id: id,
            timestamp_ms,
            price: dec("101"),
            quantity: dec("2"),
            side,
        }
    }

    #[test]
    fn coverage_and_runs_begin_only_when_snapshot_syncs() {
        let mut history = LiquidityHistory::new(enabled_config());
        assert_eq!(history.status(), HistoryStatus::Empty);
        assert_eq!(history.runs().count(), 0);
        assert_eq!(history.coverage_segments().count(), 0);

        let delta = BookDelta::new(1, 1, vec![], vec![]);
        assert!(matches!(
            history.apply_delta(90, &delta),
            Err(HistoryError::NotSynchronized)
        ));
        assert_eq!(history.coverage_segments().count(), 0);

        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        assert_eq!(
            history.status(),
            HistoryStatus::Synced {
                generation: 1,
                last_update_id: Some(10),
                last_event_ms: 100,
            }
        );
        assert_eq!(
            history.coverage_segments().collect::<Vec<_>>(),
            [&CoverageSegment {
                generation: 1,
                start_ms: 100,
                end_ms: None,
            }]
        );
        assert_eq!(history.active_level_count(), 4);
        assert!(history.runs().all(|run| run.start_ms == 100));
    }

    #[test]
    fn bucket_total_is_rle_encoded_not_individual_level_churn() {
        let config = HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            ..HeatmapConfig::default()
        };
        let mut history = LiquidityHistory::new(config);
        history
            .install_snapshot(
                100,
                1,
                BookSnapshot::new(
                    10,
                    vec![level("100.1", "2"), level("100.9", "3")],
                    vec![level("102", "1")],
                    BookCoverage::Full,
                ),
            )
            .unwrap();
        let initial_bid = history
            .runs()
            .find(|run| run.side == BookSide::Bid)
            .unwrap();
        assert_eq!(initial_bid.price_bucket, dec("100"));
        assert_eq!(initial_bid.quantity, dec("5"));

        // Both exchange levels change, but their bucket sum remains five.
        history
            .apply_delta(
                110,
                &BookDelta::new(
                    11,
                    11,
                    vec![level("100.1", "4"), level("100.9", "1")],
                    vec![],
                ),
            )
            .unwrap();
        assert_eq!(history.archived_run_count(), 0);
        let unchanged = history
            .runs()
            .find(|run| run.side == BookSide::Bid)
            .unwrap();
        assert_eq!(unchanged.start_ms, 100);
        assert_eq!(unchanged.quantity, dec("5"));

        history
            .apply_delta(
                120,
                &BookDelta::new(12, 12, vec![level("100.9", "2")], vec![]),
            )
            .unwrap();
        let bid_runs: Vec<_> = history
            .runs()
            .filter(|run| run.side == BookSide::Bid)
            .collect();
        assert_eq!(bid_runs.len(), 2);
        assert!(bid_runs.iter().any(|run| {
            run.quantity == dec("5") && run.start_ms == 100 && run.end_ms == Some(120)
        }));
        assert!(bid_runs.iter().any(|run| {
            run.quantity == dec("6") && run.start_ms == 120 && run.end_ms.is_none()
        }));
    }

    #[test]
    fn indexed_run_query_matches_full_intersection_filter() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history
            .apply_delta(120, &BookDelta::new(11, 11, vec![level("99", "7")], vec![]))
            .unwrap();
        history
            .apply_delta(
                130,
                &BookDelta::new(12, 12, vec![], vec![level("101", "8")]),
            )
            .unwrap();

        let start_ms = 121;
        let end_ms = 131;
        let latest_ms = history.latest_book_ms().unwrap();
        let expected: Vec<_> = history
            .runs()
            .filter(|run| run.start_ms < end_ms && run.end_ms.unwrap_or(latest_ms) > start_ms)
            .cloned()
            .collect();
        let indexed: Vec<_> = history
            .runs_intersecting(start_ms, end_ms)
            .cloned()
            .collect();
        assert_eq!(indexed, expected);
        assert!(
            indexed
                .iter()
                .all(|run| run.end_ms.is_none_or(|end| end > start_ms))
        );
    }

    #[test]
    fn resync_closes_generation_and_preserves_explicit_gap() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        history.mark_gap(150, "sequence_gap").unwrap();

        assert!(matches!(
            history.status(),
            HistoryStatus::Gap {
                from_generation: Some(1),
                start_ms: 150,
                ..
            }
        ));
        assert_eq!(history.active_level_count(), 0);
        assert!(!history.book().is_initialized());

        history.install_snapshot(200, 2, snapshot(50)).unwrap();
        let segments: Vec<_> = history.coverage_segments().cloned().collect();
        assert_eq!(
            segments,
            [
                CoverageSegment {
                    generation: 1,
                    start_ms: 100,
                    end_ms: Some(150),
                },
                CoverageSegment {
                    generation: 2,
                    start_ms: 200,
                    end_ms: None,
                },
            ]
        );
        let gaps: Vec<_> = history.coverage_gaps().cloned().collect();
        assert_eq!(
            gaps,
            [CoverageGap {
                from_generation: Some(1),
                to_generation: Some(2),
                start_ms: 150,
                end_ms: Some(200),
                reason: "sequence_gap".to_owned(),
            }]
        );
        assert!(
            history
                .runs()
                .filter(|run| run.generation == 1)
                .all(|run| run.end_ms == Some(150))
        );
        assert!(
            history
                .runs()
                .filter(|run| run.generation == 2)
                .all(|run| run.start_ms == 200)
        );
    }

    #[test]
    fn rejected_delta_opens_gap_and_requires_new_snapshot() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        let error = history
            .apply_delta(
                120,
                &BookDelta::new(12, 12, vec![level("100", "9")], vec![]),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            HistoryError::Book(BookError::SequenceGap { .. })
        ));
        assert!(matches!(history.status(), HistoryStatus::Gap { .. }));
        assert!(matches!(
            history.apply_delta(130, &BookDelta::new(13, 13, vec![], vec![])),
            Err(HistoryError::NotSynchronized)
        ));
    }

    #[test]
    fn retention_clips_runs_and_coverage_to_the_honest_window() {
        let config = HeatmapConfig {
            enabled: true,
            retention_ms: 1_000,
            price_grouping: Decimal::ONE,
            ..HeatmapConfig::default()
        };
        let mut history = LiquidityHistory::new(config);
        history.install_snapshot(0, 1, snapshot(10)).unwrap();
        history
            .apply_delta(
                500,
                &BookDelta::new(11, 11, vec![level("100", "7")], vec![]),
            )
            .unwrap();
        history
            .apply_delta(
                1_500,
                &BookDelta::new(12, 12, vec![level("100", "8")], vec![]),
            )
            .unwrap();

        assert!(history.runs().all(|run| run.start_ms >= 500));
        assert_eq!(history.coverage_segments().next().unwrap().start_ms, 500);
        assert!(history.counters().runs_evicted > 0);
    }

    #[test]
    fn run_and_aggression_caps_evict_oldest_records() {
        let config = HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            max_history_runs: 1,
            max_aggressions: 2,
            ..HeatmapConfig::default()
        };
        let mut history = LiquidityHistory::new(config);
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        for (id, timestamp, quantity) in [(11, 110, "6"), (12, 120, "7"), (13, 130, "8")] {
            history
                .apply_delta(
                    timestamp,
                    &BookDelta::new(id, id, vec![level("100", quantity)], vec![]),
                )
                .unwrap();
        }
        assert!(history.archived_run_count() <= 1);
        assert!(history.counters().runs_evicted > 0);

        history.record_aggression(&trade(1, 131, Side::Buy));
        history.record_aggression(&trade(2, 132, Side::Sell));
        history.record_aggression(&trade(3, 133, Side::Buy));
        assert_eq!(
            history
                .aggressions()
                .map(|aggression| aggression.agg_id)
                .collect::<Vec<_>>(),
            [2, 3]
        );
        assert_eq!(history.counters().aggressions_evicted, 1);
    }

    #[test]
    fn approximate_byte_budget_truncates_oldest_complete_history() {
        let config = HeatmapConfig {
            enabled: true,
            price_grouping: Decimal::ONE,
            max_history_bytes: 1_024,
            max_history_runs: 10_000,
            ..HeatmapConfig::default()
        };
        let mut history = LiquidityHistory::new(config);
        history.install_snapshot(100, 1, snapshot(10)).unwrap();

        for step in 1_u64..=20 {
            let quantity = Decimal::from(step + 10);
            history
                .apply_delta(
                    100 + step as i64,
                    &BookDelta::new(
                        10 + step,
                        10 + step,
                        vec![
                            BookLevel::new(dec("99"), quantity).unwrap(),
                            BookLevel::new(dec("100"), quantity).unwrap(),
                        ],
                        vec![
                            BookLevel::new(dec("101"), quantity).unwrap(),
                            BookLevel::new(dec("102"), quantity).unwrap(),
                        ],
                    ),
                )
                .unwrap();
        }

        assert!(history.approximate_history_bytes() <= 1_024);
        assert!(history.counters().runs_evicted > 0);
        let coverage_start = history.coverage_segments().next().unwrap().start_ms;
        assert!(
            history.runs().all(|run| run.start_ms >= coverage_start),
            "coverage may not claim history older than retained runs"
        );
    }

    #[test]
    fn aggression_mapping_never_mutates_the_order_book() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        let before = history.book().clone();

        history.record_aggression(&trade(1, 101, Side::Buy));
        history.record_aggression(&trade(2, 102, Side::Sell));

        assert_eq!(history.book(), &before);
        let aggressions: Vec<_> = history.aggressions().collect();
        assert_eq!(aggressions[0].consumed_side(), BookSide::Ask);
        assert_eq!(aggressions[1].consumed_side(), BookSide::Bid);
    }

    #[test]
    fn grouping_change_requires_explicit_reset() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();

        let requested = HeatmapConfig {
            price_grouping: dec("0.5"),
            ..history.config().clone()
        };
        assert!(matches!(
            history.update_config(requested),
            Err(HistoryError::PriceGroupingResetRequired {
                current,
                requested
            }) if current == Decimal::ONE && requested == dec("0.5")
        ));
        assert!(history.book().is_initialized());

        let reset = history.reset_price_grouping(dec("0.5")).unwrap();
        assert_eq!(reset.previous, Decimal::ONE);
        assert_eq!(reset.current, dec("0.5"));
        assert!(reset.dropped_runs > 0);
        assert_eq!(history.status(), HistoryStatus::Empty);
        assert_eq!(history.runs().count(), 0);
        assert!(!history.book().is_initialized());
    }

    #[test]
    fn update_config_applies_caps_and_aggression_toggle_immediately() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.record_aggression(&trade(1, 100, Side::Buy));
        history.record_aggression(&trade(2, 101, Side::Buy));
        assert_eq!(history.aggressions().count(), 2);

        let config = HeatmapConfig {
            show_aggressions: false,
            max_aggressions: 1,
            ..history.config().clone()
        };
        history.update_config(config).unwrap();
        assert_eq!(history.aggressions().count(), 0);
        history.record_aggression(&trade(3, 102, Side::Sell));
        assert_eq!(history.aggressions().count(), 0);
    }

    #[test]
    fn stale_delta_does_not_split_runs() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        let runs_before = history.runs().count();
        let outcome = history
            .apply_delta(
                110,
                &BookDelta::new(9, 10, vec![level("100", "999")], vec![]),
            )
            .unwrap();
        assert!(matches!(outcome, ApplyOutcome::Stale { .. }));
        assert_eq!(history.runs().count(), runs_before);
        assert_eq!(history.counters().deltas_stale, 1);
    }

    #[test]
    fn rejects_backwards_book_time_and_reused_generation() {
        let mut history = LiquidityHistory::new(enabled_config());
        history.install_snapshot(100, 1, snapshot(10)).unwrap();
        assert!(matches!(
            history.mark_gap(99, "late"),
            Err(HistoryError::BackwardsTimestamp {
                last_ms: 100,
                got_ms: 99
            })
        ));
        history.mark_gap(110, "resync").unwrap();
        assert!(matches!(
            history.install_snapshot(120, 1, snapshot(20)),
            Err(HistoryError::GenerationNotAdvanced {
                previous: 1,
                got: 1
            })
        ));
    }
}
