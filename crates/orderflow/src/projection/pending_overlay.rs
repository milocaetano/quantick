//! The pending prints' overlay cells, folded again only where new prints
//! landed.
//!
//! Every frame used to cluster and fold every pending print again. While the
//! worker keeps up that is a few hundred prints; while it lags, every second
//! of lag adds about ten thousand WIN prints at 50x, and the frame paid a
//! microsecond for each of them. A native cell's fold reads only its own
//! key's prints and published cell ([`fold_dots`] folds each key alone, in
//! an order of its own), so [`OverlayCells`] keeps each key's prints and
//! folded cell, and a frame folds again only the keys its new prints touch.
//! Anything else that could change a cell — another publication, a
//! receipt, another grid or window, a lane start or horizon moving back —
//! folds them all again, as before.

use std::collections::BTreeMap;
use std::sync::Arc;

use quantick_engine::Trade;
use rust_decimal::Decimal;

use super::dots::{DotHorizon, fold_dots, window_start};
use super::{HeatmapProjection, TapeFacts, VolumeDots};
use crate::grouping::EffectiveGrouping;
use crate::history::Aggression;
use crate::interaction::{AggressionCluster, cluster_aggressions, cluster_order, sort_clusters};

/// What the cells were folded against: a cell folded under anything else
/// is another cell.
#[derive(Clone)]
struct Basis {
    published: Arc<HeatmapProjection>,
    native: EffectiveGrouping,
    tape_window_ms: i64,
    tape_level_ticks: i64,
    native_tape: bool,
    /// The oldest pending print when the cells were first folded: a receipt
    /// retires prints from the front.
    front: Option<u64>,
}

impl Basis {
    fn holds(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.published, &other.published)
            && self.native == other.native
            && self.tape_window_ms == other.tape_window_ms
            && self.tape_level_ticks == other.tape_level_ticks
            && self.native_tape == other.native_tape
            && self.front == other.front
    }
}

/// One native key's pending prints, each its own raw cluster in the order
/// the complete fold reads them, and the cell they fold into.
#[derive(Default)]
struct KeyCells {
    prints: Vec<AggressionCluster>,
    folded: Vec<AggressionCluster>,
}

/// The overlay's cells, kept from one frame to the next.
pub(super) struct OverlayCells {
    basis: Basis,
    from_ms: i64,
    evicted_through_ms: Option<i64>,
    /// The newest pending ordinal read.
    through: u64,
    keys: BTreeMap<(i64, Decimal), KeyCells>,
}

/// Where one frame's overlay is folded, and what it reads.
pub(super) struct OverlayFrame<'a> {
    pub(super) published: &'a Arc<HeatmapProjection>,
    pub(super) facts: &'a TapeFacts,
    pub(super) native: EffectiveGrouping,
    pub(super) dots: &'a VolumeDots,
    pub(super) from_ms: i64,
    pub(super) evicted_through_ms: Option<i64>,
}

impl OverlayFrame<'_> {
    fn basis(&self, front: Option<u64>) -> Basis {
        Basis {
            published: Arc::clone(self.published),
            native: self.native,
            tape_window_ms: self.dots.tape_window_ms,
            tape_level_ticks: self.dots.tape_level_ticks,
            native_tape: self.dots.native_tape,
            front,
        }
    }

    fn window(&self, timestamp_ms: i64) -> i64 {
        window_start(timestamp_ms, self.dots.tape_window_ms)
    }

    /// A pending print's native key.
    fn key(&self, print: &AggressionCluster) -> (i64, Decimal) {
        let width = self.native.bucket_width;
        (
            self.window(print.timestamp_ms),
            (print.price / width).floor() * width,
        )
    }

    /// Whether the tape reads a cell in `window` at all.
    fn reads(&self, window: i64) -> bool {
        window >= self.from_ms
            && self
                .evicted_through_ms
                .is_none_or(|horizon| window > horizon)
    }

    /// The published cell of `key`, which the key's prints fold onto.
    fn published_cell(&self, (start, bucket): (i64, Decimal)) -> Option<&AggressionCluster> {
        if !self.reads(start) {
            return None;
        }
        let cells = &self.facts.clusters;
        let low = cells.partition_point(|cell| self.window(cell.first_timestamp_ms) < start);
        cells[low..]
            .iter()
            .take_while(|cell| self.window(cell.first_timestamp_ms) == start)
            .find(|cell| cell.price_bucket == bucket)
    }

    /// `prints` of one key and its published cell, folded as the complete
    /// overlay folds them.
    fn fold(&self, key: (i64, Decimal), prints: &[AggressionCluster]) -> Vec<AggressionCluster> {
        let mut cells: Vec<AggressionCluster> = prints.to_vec();
        cells.extend(self.published_cell(key).cloned());
        cells.retain(|cell| self.window(cell.first_timestamp_ms) >= self.from_ms);
        fold_dots(
            cells,
            true,
            self.dots,
            self.native,
            DotHorizon {
                recorded_from_ms: None,
                evicted_through_ms: self.evicted_through_ms,
            },
        )
    }
}

impl OverlayCells {
    /// The overlay's cells for `frame` over the pending `trades`, carrying on
    /// from `kept` where nothing but new prints changed.
    pub(super) fn fold(
        kept: Option<Self>,
        frame: &OverlayFrame<'_>,
        trades: &std::collections::VecDeque<(u64, Trade)>,
    ) -> (Vec<AggressionCluster>, Self) {
        let basis = frame.basis(trades.front().map(|(ordinal, _)| *ordinal));
        let mut cells = kept
            .filter(|kept| {
                kept.basis.holds(&basis) && frame.evicted_through_ms >= kept.evicted_through_ms
            })
            .unwrap_or_else(|| Self {
                basis,
                from_ms: frame.from_ms,
                evicted_through_ms: frame.evicted_through_ms,
                through: 0,
                keys: BTreeMap::new(),
            });
        // A lane start or horizon that moved on drops whole keys: every
        // print of a key shares its window.
        cells.keys.retain(|(window, _), _| frame.reads(*window));
        // A lane start that moved back reaches prints already read and left
        // out: they are read again, as the complete overlay reads them.
        let reached = cells.from_ms;
        let returning = trades
            .iter()
            .take_while(|(ordinal, _)| *ordinal <= cells.through)
            .map(|(_, trade)| trade)
            .filter(|trade| {
                let window = frame.window(trade.timestamp_ms);
                window >= frame.from_ms && window < reached
            });
        let first_fresh = trades.partition_point(|(ordinal, _)| *ordinal <= cells.through);
        let fresh: Vec<Aggression> = returning
            .chain(
                trades
                    .range(first_fresh..)
                    .map(|(_, trade)| trade)
                    .filter(|trade| frame.window(trade.timestamp_ms) >= frame.from_ms),
            )
            .map(|trade| Aggression {
                agg_id: trade.agg_id,
                timestamp_ms: trade.timestamp_ms,
                price: trade.price,
                quantity: trade.quantity,
                side: trade.side,
                generation: None,
            })
            .collect();
        cells.from_ms = frame.from_ms;
        cells.evicted_through_ms = frame.evicted_through_ms;
        cells.through = trades.back().map_or(cells.through, |(ordinal, _)| *ordinal);
        let mut touched = Vec::new();
        for print in cluster_aggressions(&fresh, &[], frame.native, 0) {
            let key = frame.key(&print);
            let held = cells.keys.entry(key).or_default();
            // In the order the complete overlay reads a key's prints.
            let at = held
                .prints
                .partition_point(|other| cluster_order(other, &print).is_le());
            held.prints.insert(at, print);
            touched.push(key);
        }
        touched.sort_unstable();
        touched.dedup();
        for key in touched {
            if let Some(held) = cells.keys.get_mut(&key) {
                held.folded = frame.fold(key, &held.prints);
            }
        }
        let mut folded: Vec<AggressionCluster> = cells
            .keys
            .values()
            .flat_map(|held| held.folded.iter().cloned())
            .collect();
        sort_clusters(&mut folded);
        (folded, cells)
    }
}
