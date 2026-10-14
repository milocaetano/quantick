//! The averaged lane's runs, kept between requests.
//!
//! Every run the history closes is folded once into the display row it
//! belongs to, onto bins on absolute time: a bin's edges are multiples of its
//! width, never offsets from the lane's moving start, and the width comes
//! from a fixed ladder. A stretch of history is therefore binned the same way
//! on every frame — the averaged depth holds still while the clock advances —
//! and a request costs the runs closed since the last one, not the lane.
//!
//! The averaging keeps the rules it always had. Each row's current constant
//! interval stays exact; older intervals become duration-weighted means that
//! count observed zero depth; a row begins at its first nonzero observation,
//! never at an unknown before it; a deleted row's mean ends at its last
//! observation; nothing is averaged across a transport gap, a generation or a
//! side. Under the cap every interval is drawn exact.

use std::collections::{BTreeMap, VecDeque};

use rust_decimal::Decimal;

use crate::grouping::{EffectiveGrouping, GroupingWindow, VisualLiquidityRun, bucket_for_price};
use crate::history::{LiquidityHistory, LiquidityRun, RestingSide};

#[cfg(test)]
#[path = "lane_fold_tests.rs"]
mod tests;

/// The bin widths an averaged lane chooses from, finest first. Fixed, so a
/// change of width is a step with a reason — a busier or a wider lane —
/// never a drift with the clock.
pub const TIME_AVERAGE_BIN_LADDER_MS: [i64; 12] = [
    100, 250, 500, 1_000, 2_500, 5_000, 10_000, 30_000, 60_000, 300_000, 900_000, 1_800_000,
];

/// The share of the free cells a finer width must fit in before a lane
/// moves to it, as `(numerator, denominator)`: a lane whose bins sit at the
/// boundary keeps its width rather than alternating between two.
const FINER_WIDTH_SHARE: (usize, usize) = (3, 4);

/// A lane run as the painter receives it: a constant interval of one row,
/// or one bin's duration-weighted mean.
pub(super) struct DepthRun {
    pub run: VisualLiquidityRun,
    pub time_averaged: bool,
}

/// A display row: one generation, side and visual bucket.
type RowKey = (u64, RestingSide, Decimal);
/// A captured level: one generation, side and capture bucket.
type BaseKey = (u64, RestingSide, Decimal);

/// One closed constant interval of a row with depth in it.
#[derive(Debug, Clone, Copy)]
struct Interval {
    start_ms: i64,
    end_ms: i64,
    quantity: Decimal,
}

/// A row: its open interval, its closed ones, and their bins.
#[derive(Debug, Default)]
struct Row {
    /// Depth since `since_ms`; zero while the row holds none.
    quantity: Decimal,
    since_ms: i64,
    /// The first nonzero observation: the row's means never start earlier.
    first_ms: Option<i64>,
    /// Where the newest closed interval with depth ends.
    last_end_ms: Option<i64>,
    closed: VecDeque<Interval>,
    /// The closed intervals' quantity × milliseconds, per bin start.
    bins: BTreeMap<i64, Decimal>,
}

impl Row {
    fn change(&mut self, at_ms: i64, delta: Decimal, width_ms: i64) {
        if self.quantity > Decimal::ZERO && at_ms > self.since_ms {
            let interval = Interval {
                start_ms: self.since_ms,
                end_ms: at_ms,
                quantity: self.quantity,
            };
            fold_into(&mut self.bins, interval, width_ms);
            self.closed.push_back(interval);
            self.last_end_ms = Some(at_ms);
        }
        self.quantity += delta;
        self.since_ms = at_ms;
        if self.quantity > Decimal::ZERO {
            self.first_ms.get_or_insert(at_ms);
        }
    }

    /// Where the row's averaged history ends: its exact interval's start, or
    /// a deleted row's last observation.
    fn averaged_end_ms(&self) -> Option<i64> {
        if self.quantity > Decimal::ZERO {
            Some(self.since_ms)
        } else {
            self.last_end_ms
        }
    }
}

const fn bin_start(time_ms: i64, width_ms: i64) -> i64 {
    time_ms.div_euclid(width_ms) * width_ms
}

fn fold_into(bins: &mut BTreeMap<i64, Decimal>, interval: Interval, width_ms: i64) {
    let mut from_ms = interval.start_ms;
    while from_ms < interval.end_ms {
        let bin = bin_start(from_ms, width_ms);
        let until_ms = interval.end_ms.min(bin.saturating_add(width_ms));
        *bins.entry(bin).or_insert(Decimal::ZERO) +=
            interval.quantity * Decimal::from(until_ms - from_ms);
        from_ms = until_ms;
    }
}

/// A change of one row's depth at one instant.
struct Event {
    at_ms: i64,
    row: RowKey,
    delta: Decimal,
}

/// What a fold was built for: a change of any of it refolds the lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FoldKey {
    capture: Decimal,
    grouping: EffectiveGrouping,
    width_ms: i64,
}

#[derive(Debug)]
struct Fold {
    key: FoldKey,
    /// Intervals and bins are held from this bin edge on.
    from_ms: i64,
    /// The history's finalized-run count when last brought up to date.
    runs_closed: u64,
    /// The history's newest book time then: every run since starts here or
    /// later.
    frontier_ms: i64,
    /// The captured runs open then: their start and depth.
    open: BTreeMap<BaseKey, (i64, Decimal)>,
    rows: BTreeMap<RowKey, Row>,
}

fn base_of(run: &LiquidityRun) -> BaseKey {
    (run.generation, run.side, run.price_bucket)
}

impl Fold {
    fn row_of(&self, run: &LiquidityRun) -> RowKey {
        (
            run.generation,
            run.side,
            bucket_for_price(run.price_bucket, self.key.grouping),
        )
    }

    /// Fold every run the history holds from the bin edge at or before
    /// `start_ms` on.
    fn build(history: &LiquidityHistory, key: FoldKey, start_ms: i64) -> Self {
        let from_ms = bin_start(start_ms, key.width_ms);
        let mut fold = Self {
            key,
            from_ms,
            runs_closed: history.counters().runs_closed,
            frontier_ms: history.latest_book_ms().unwrap_or(i64::MIN),
            open: BTreeMap::new(),
            rows: BTreeMap::new(),
        };
        let floor_ms = history
            .retention_start_ms()
            .map_or(from_ms, |retained| retained.max(from_ms));
        let mut events = Vec::new();
        let closed = history
            .runs_intersecting(floor_ms, i64::MAX)
            .filter(|run| run.end_ms.is_some());
        for run in closed.chain(history.open_runs()) {
            let row = fold.row_of(run);
            events.push(Event {
                at_ms: run.start_ms.max(floor_ms),
                row,
                delta: run.quantity,
            });
            match run.end_ms {
                Some(end_ms) => events.push(Event {
                    at_ms: end_ms,
                    row,
                    delta: -run.quantity,
                }),
                None => {
                    fold.open.insert(base_of(run), (run.start_ms, run.quantity));
                }
            }
        }
        fold.apply(events);
        fold
    }

    /// Fold the runs the history closed or opened since; `false` when the
    /// history no longer continues this fold (a reset, a regroup, a run
    /// evicted before it was read), and the caller refolds.
    fn advance(&mut self, history: &LiquidityHistory) -> bool {
        let Some(closed) = history.runs_closed_since(self.runs_closed) else {
            return false;
        };
        let mut events = Vec::new();
        for run in closed {
            let Some(end_ms) = run.end_ms else {
                return false;
            };
            let row = self.row_of(run);
            match self.open.get(&base_of(run)) {
                Some(&(start_ms, _)) if start_ms == run.start_ms => {
                    self.open.remove(&base_of(run));
                }
                _ if run.start_ms >= self.frontier_ms => events.push(Event {
                    at_ms: run.start_ms.max(self.from_ms),
                    row,
                    delta: run.quantity,
                }),
                _ => return false,
            }
            events.push(Event {
                at_ms: end_ms,
                row,
                delta: -run.quantity,
            });
        }
        let mut open = BTreeMap::new();
        for run in history.open_runs() {
            let base = base_of(run);
            match self.open.remove(&base) {
                Some(held) if held == (run.start_ms, run.quantity) => {}
                None if run.start_ms >= self.frontier_ms => events.push(Event {
                    at_ms: run.start_ms.max(self.from_ms),
                    row: self.row_of(run),
                    delta: run.quantity,
                }),
                _ => return false,
            }
            open.insert(base, (run.start_ms, run.quantity));
        }
        // An open run must close through the archive, never vanish.
        if !self.open.is_empty() {
            return false;
        }
        self.open = open;
        self.runs_closed = history.counters().runs_closed;
        self.frontier_ms = history.latest_book_ms().unwrap_or(self.frontier_ms);
        self.apply(events);
        true
    }

    /// Apply `events` in time order, netting each row's changes at one
    /// instant: a change undone at the same instant splits nothing.
    fn apply(&mut self, mut events: Vec<Event>) {
        events.sort_by_key(|event| (event.at_ms, event.row));
        let mut events = events.into_iter().peekable();
        while let Some(event) = events.next() {
            let mut delta = event.delta;
            while let Some(next) =
                events.next_if(|next| next.at_ms == event.at_ms && next.row == event.row)
            {
                delta += next.delta;
            }
            if !delta.is_zero() {
                self.rows.entry(event.row).or_default().change(
                    event.at_ms,
                    delta,
                    self.key.width_ms,
                );
            }
        }
    }

    /// Each visible row's averaged stretch, `(row, low, high)`: from its
    /// first observation, never before its coverage, to its exact interval (a
    /// deleted row: its last observation), never past `observed_end_ms` or a
    /// gap. Only rows with some of it after `start_ms`.
    fn spans(
        &self,
        history: &LiquidityHistory,
        visible: impl Fn(&RowKey) -> bool,
        start_ms: i64,
        observed_end_ms: i64,
    ) -> Vec<(RowKey, i64, i64)> {
        let coverage: BTreeMap<u64, (i64, Option<i64>)> = history
            .coverage_segments()
            .map(|segment| (segment.generation, (segment.start_ms, segment.end_ms)))
            .collect();
        self.rows
            .iter()
            .filter(|(key, _)| visible(key))
            .filter_map(|(key, row)| {
                let &(covered_ms, ended_ms) = coverage.get(&key.0)?;
                let low = row.first_ms?.max(covered_ms);
                let high = row
                    .averaged_end_ms()?
                    .min(observed_end_ms)
                    .min(ended_ms.unwrap_or(observed_end_ms));
                (high > low.max(start_ms)).then_some((*key, low, high))
            })
            .collect()
    }

    /// Drop what lies wholly before the bin edge at or before `start_ms`.
    fn evict(&mut self, start_ms: i64) {
        let from_ms = bin_start(start_ms, self.key.width_ms);
        if from_ms <= self.from_ms {
            return;
        }
        self.from_ms = from_ms;
        self.rows.retain(|_, row| {
            while row
                .closed
                .front()
                .is_some_and(|interval| interval.end_ms <= from_ms)
            {
                row.closed.pop_front();
            }
            row.bins = row.bins.split_off(&from_ms);
            row.quantity > Decimal::ZERO || !row.closed.is_empty()
        });
    }
}

/// The finest ladder width whose bins over `spans` fit `slots`. A width in
/// use is kept while it fits, unless a finer one fits in
/// [`FINER_WIDTH_SHARE`] of the slots.
fn choose_width(spans: &[(i64, i64)], slots: usize, current_ms: i64) -> i64 {
    let bins = |width_ms: i64| -> usize {
        spans
            .iter()
            .map(|&(low, high)| {
                let count = (high - 1).div_euclid(width_ms) - low.div_euclid(width_ms) + 1;
                usize::try_from(count).unwrap_or(0)
            })
            .sum()
    };
    let (share, of) = FINER_WIDTH_SHARE;
    if TIME_AVERAGE_BIN_LADDER_MS.contains(&current_ms) && bins(current_ms) <= slots {
        return TIME_AVERAGE_BIN_LADDER_MS
            .into_iter()
            .take_while(|&width| width < current_ms)
            .find(|&width| bins(width).saturating_mul(of) <= slots.saturating_mul(share))
            .unwrap_or(current_ms);
    }
    TIME_AVERAGE_BIN_LADDER_MS
        .into_iter()
        .find(|&width| bins(width) <= slots)
        .unwrap_or(TIME_AVERAGE_BIN_LADDER_MS[TIME_AVERAGE_BIN_LADDER_MS.len() - 1])
}

/// The averaged lane's fold, held by its worker between requests. A fresh
/// one draws the same runs, folding the history at once.
#[derive(Debug, Default)]
pub struct LaneFold {
    fold: Option<Fold>,
}

impl LaneFold {
    /// Forget the fold: the lane is not averaged.
    pub(crate) fn clear(&mut self) {
        self.fold = None;
    }

    fn fold(
        &mut self,
        history: &LiquidityHistory,
        grouping: EffectiveGrouping,
        width_ms: i64,
        start_ms: i64,
    ) -> &mut Fold {
        let key = FoldKey {
            capture: history.config().price_grouping,
            grouping,
            width_ms,
        };
        let kept = self
            .fold
            .take()
            .filter(|fold| fold.key == key && fold.from_ms <= bin_start(start_ms, width_ms));
        let fold = kept
            .and_then(|mut fold| fold.advance(history).then_some(fold))
            .unwrap_or_else(|| Fold::build(history, key, start_ms));
        let fold = self.fold.insert(fold);
        fold.evict(start_ms);
        fold
    }

    /// The lane's runs over `window`, brought up to `history`: every
    /// visible row's constant intervals while they fit `cap`; past it, each
    /// row's current interval exact and its history averaged on absolute bins
    /// of the finest ladder width that fits, never past `observed_end_ms`.
    pub(super) fn project(
        &mut self,
        history: &LiquidityHistory,
        grouping: EffectiveGrouping,
        window: GroupingWindow,
        observed_end_ms: i64,
        cap: usize,
    ) -> Vec<DepthRun> {
        let render_end_ms = window.end_ms.min(window.open_run_end_ms);
        if render_end_ms <= window.start_ms || window.price_high <= window.price_low {
            return Vec::new();
        }
        let start_ms = window.start_ms;
        let visible = |row: &RowKey| {
            row.2 < window.price_high && row.2 + grouping.bucket_width > window.price_low
        };
        let run = |row: &RowKey, quantity: Decimal, from_ms: i64, to_ms: i64| VisualLiquidityRun {
            generation: row.0,
            side: row.1,
            price_bucket: row.2,
            quantity,
            start_ms: from_ms,
            end_ms: to_ms,
        };
        let width_ms = self
            .fold
            .as_ref()
            .map_or(TIME_AVERAGE_BIN_LADDER_MS[0], |fold| fold.key.width_ms);
        let fold = self.fold(history, grouping, width_ms, start_ms);
        let mut current = Vec::new();
        let mut closed = 0;
        for (key, row) in fold.rows.iter().filter(|(key, _)| visible(key)) {
            closed += row.closed.len()
                - row
                    .closed
                    .partition_point(|interval| interval.end_ms <= start_ms);
            let from_ms = row.since_ms.max(start_ms);
            if row.quantity > Decimal::ZERO && render_end_ms > from_ms {
                current.push(DepthRun {
                    run: run(key, row.quantity, from_ms, render_end_ms),
                    time_averaged: false,
                });
            }
        }
        if cap == 0 || current.len() + closed <= cap {
            for (key, row) in fold.rows.iter().filter(|(key, _)| visible(key)) {
                for interval in row
                    .closed
                    .iter()
                    .filter(|interval| interval.end_ms > start_ms)
                {
                    current.push(DepthRun {
                        run: run(
                            key,
                            interval.quantity,
                            interval.start_ms.max(start_ms),
                            interval.end_ms,
                        ),
                        time_averaged: false,
                    });
                }
            }
            return current;
        }
        let mut spans = fold.spans(history, visible, start_ms, observed_end_ms);
        let on_screen: Vec<_> = spans
            .iter()
            .map(|&(_, low, high)| (low.max(start_ms), high))
            .collect();
        let slots = cap.saturating_sub(current.len()).max(1);
        let width_ms = choose_width(&on_screen, slots, fold.key.width_ms);
        let fold = if width_ms == fold.key.width_ms {
            fold
        } else {
            // Refolded from an earlier bin edge: its rows may start earlier.
            let fold = self.fold(history, grouping, width_ms, start_ms);
            spans = fold.spans(history, visible, start_ms, observed_end_ms);
            fold
        };
        for (key, low, high) in spans {
            let Some(row) = fold.rows.get(&key) else {
                continue;
            };
            for (&bin, &weighted) in row.bins.range(bin_start(low.max(start_ms), width_ms)..high) {
                let (from_ms, to_ms) = (bin.max(low), bin.saturating_add(width_ms).min(high));
                if to_ms > from_ms && to_ms > start_ms {
                    current.push(DepthRun {
                        run: run(
                            &key,
                            weighted / Decimal::from(to_ms - from_ms),
                            from_ms.max(start_ms),
                            to_ms,
                        ),
                        time_averaged: true,
                    });
                }
            }
        }
        current
    }
}
