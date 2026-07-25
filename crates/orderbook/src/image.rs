//! Turning full book images into snapshot-plus-delta streams.
//!
//! Venues publish depth in one of two shapes. Some send a starting image and
//! then incremental updates carrying their own sequence ids (Binance). Others
//! send a *complete image every time anything changes* and carry no ids at all
//! — MetaTrader 5's DOM works this way: one `OnBookEvent`, one full
//! `MarketBookGet` result.
//!
//! [`SnapshotDiffer`] converts the second shape into the first. It keeps the
//! last accepted image, and for each new one emits only the levels that
//! actually changed, numbered with synthetic monotonic update ids. Consumers
//! then speak a single dialect: [`OrderBook`](crate::OrderBook) applies the
//! result unchanged, and a run-length history downstream stores one transition
//! per real change instead of a fresh full book many times a second.
//!
//! That is also why the diff lives here rather than in the consumer: a 40-level
//! image republished 50 times a second is 2 000 levels/s of which typically a
//! handful moved. Diffing at the source keeps everything after it proportional
//! to real change.
//!
//! # Determinism
//!
//! Same images in, same events out. Levels are normalized (sorted by exact
//! price, duplicates summed, non-positive prices and quantities dropped) before
//! any comparison, so a source that reorders its levels between events produces
//! no spurious updates.

use rust_decimal::Decimal;

use crate::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

/// One normalized price level: exact price and total resting quantity.
type Level = (Decimal, Decimal);

/// What one observed image means for a snapshot-plus-delta consumer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageOutcome {
    /// First accepted image of this generation. Install it before any delta.
    Snapshot(BookSnapshot),
    /// Levels that changed since the previous image; zero quantity removes.
    Delta(BookDelta),
    /// The image matched the previous one exactly: nothing to publish.
    ///
    /// Real feeds emit these constantly — MT5 fires a book event whenever any
    /// order in the DOM changes, including changes outside the visible depth
    /// or in market-order rows this differ drops.
    Unchanged,
    /// The image was rejected because its best bid is at or above its best ask.
    ///
    /// A crossed image is not a bug to hide: B3 publishes one during the
    /// pre-open auction, when the theoretical price is still forming. The
    /// previous accepted image is kept and the caller decides what to say about
    /// the skip — it must never be presented as live, uncrossed liquidity.
    Crossed {
        /// Highest bid in the rejected image.
        best_bid: Decimal,
        /// Lowest ask in the rejected image.
        best_ask: Decimal,
    },
}

/// Converts successive full book images into a snapshot and absolute deltas.
///
/// One differ tracks one generation. Call [`reset`](Self::reset) when
/// continuity ends (the source restarted, the consumer resubscribed); the next
/// image then starts a fresh [`ImageOutcome::Snapshot`].
#[derive(Debug, Clone)]
pub struct SnapshotDiffer {
    coverage: BookCoverage,
    bids: Vec<Level>,
    asks: Vec<Level>,
    // Scratch for the image being observed, reused so the steady state does not
    // allocate: only the emitted delta's own vectors do.
    next_bids: Vec<Level>,
    next_asks: Vec<Level>,
    last_update_id: u64,
    initialized: bool,
}

impl SnapshotDiffer {
    /// A differ awaiting its first image, declaring `coverage` on the snapshot
    /// it will emit.
    #[must_use]
    pub fn new(coverage: BookCoverage) -> Self {
        Self {
            coverage,
            bids: Vec::new(),
            asks: Vec::new(),
            next_bids: Vec::new(),
            next_asks: Vec::new(),
            last_update_id: 0,
            initialized: false,
        }
    }

    /// Update id carried by the most recently emitted event (0 before the
    /// first snapshot).
    #[must_use]
    pub fn last_update_id(&self) -> u64 {
        self.last_update_id
    }

    /// Whether an image has been accepted since construction or the last reset.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Number of levels currently held per side, as `(bids, asks)`.
    #[must_use]
    pub fn level_counts(&self) -> (usize, usize) {
        (self.bids.len(), self.asks.len())
    }

    /// Forget the current image so the next one starts a new generation.
    ///
    /// Update ids keep advancing across a reset. They are only meaningful
    /// inside one generation, and never reusing an id keeps a late event from a
    /// discarded generation from looking current.
    pub fn reset(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.initialized = false;
    }

    /// Observe one complete image of the book.
    ///
    /// Levels may arrive in any order and may repeat a price (quantities are
    /// summed). Entries with a non-positive price or quantity are dropped: MT5
    /// reports market-order rows with no meaningful price, and an absent level
    /// is exactly what "quantity zero" means in an image.
    pub fn observe(&mut self, bids: &[BookLevel], asks: &[BookLevel]) -> ImageOutcome {
        normalize(bids, &mut self.next_bids);
        normalize(asks, &mut self.next_asks);

        if let (Some(&(best_bid, _)), Some(&(best_ask, _))) =
            (self.next_bids.last(), self.next_asks.first())
            && best_bid >= best_ask
        {
            return ImageOutcome::Crossed { best_bid, best_ask };
        }

        if !self.initialized {
            std::mem::swap(&mut self.bids, &mut self.next_bids);
            std::mem::swap(&mut self.asks, &mut self.next_asks);
            self.initialized = true;
            self.last_update_id = self.last_update_id.saturating_add(1);
            return ImageOutcome::Snapshot(BookSnapshot::new(
                self.last_update_id,
                levels(&self.bids),
                levels(&self.asks),
                self.coverage,
            ));
        }

        let bid_updates = diff_side(&self.bids, &self.next_bids);
        let ask_updates = diff_side(&self.asks, &self.next_asks);
        if bid_updates.is_empty() && ask_updates.is_empty() {
            return ImageOutcome::Unchanged;
        }

        std::mem::swap(&mut self.bids, &mut self.next_bids);
        std::mem::swap(&mut self.asks, &mut self.next_asks);
        self.last_update_id = self.last_update_id.saturating_add(1);
        ImageOutcome::Delta(BookDelta::new(
            self.last_update_id,
            self.last_update_id,
            bid_updates,
            ask_updates,
        ))
    }
}

/// Copy usable entries into `dst`, sorted by price with duplicates summed.
fn normalize(src: &[BookLevel], dst: &mut Vec<Level>) {
    dst.clear();
    dst.extend(
        src.iter()
            .filter(|level| level.price() > Decimal::ZERO && level.quantity() > Decimal::ZERO)
            .map(|level| (level.price(), level.quantity())),
    );
    dst.sort_unstable_by_key(|level| level.0);

    // Fold repeated prices into the first occurrence. A venue may split one
    // price across rows; the book stores one quantity per price.
    let mut write = 0;
    for read in 1..dst.len() {
        if dst[read].0 == dst[write].0 {
            let merged = dst[write].1.saturating_add(dst[read].1);
            dst[write].1 = merged;
        } else {
            write += 1;
            dst[write] = dst[read];
        }
    }
    if !dst.is_empty() {
        dst.truncate(write + 1);
    }
}

/// Absolute updates carrying `next` from `previous`, walking both sorted sides
/// once. A level only present in `previous` is emitted with quantity zero.
fn diff_side(previous: &[Level], next: &[Level]) -> Vec<BookLevel> {
    let mut updates = Vec::new();
    let (mut old, mut new) = (0, 0);
    while old < previous.len() && new < next.len() {
        let (old_price, old_quantity) = previous[old];
        let (new_price, new_quantity) = next[new];
        match old_price.cmp(&new_price) {
            std::cmp::Ordering::Less => {
                updates.push(level(old_price, Decimal::ZERO));
                old += 1;
            }
            std::cmp::Ordering::Greater => {
                updates.push(level(new_price, new_quantity));
                new += 1;
            }
            std::cmp::Ordering::Equal => {
                if old_quantity != new_quantity {
                    updates.push(level(new_price, new_quantity));
                }
                old += 1;
                new += 1;
            }
        }
    }
    updates.extend(
        previous[old..]
            .iter()
            .map(|&(price, _)| level(price, Decimal::ZERO)),
    );
    updates.extend(
        next[new..]
            .iter()
            .map(|&(price, quantity)| level(price, quantity)),
    );
    updates
}

fn levels(side: &[Level]) -> Vec<BookLevel> {
    side.iter()
        .map(|&(price, quantity)| level(price, quantity))
        .collect()
}

/// Build a level whose validity `normalize` already guaranteed (positive price,
/// non-negative quantity), skipping the checked constructor in the hot path.
fn level(price: Decimal, quantity: Decimal) -> BookLevel {
    BookLevel { price, quantity }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OrderBook;
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn image(side: &[(&str, &str)]) -> Vec<BookLevel> {
        side.iter()
            .map(|(price, quantity)| BookLevel::new(dec(price), dec(quantity)).unwrap())
            .collect()
    }

    fn differ() -> SnapshotDiffer {
        SnapshotDiffer::new(BookCoverage::Limited {
            levels_per_side: 10,
        })
    }

    #[test]
    fn the_first_image_becomes_a_snapshot() {
        let mut differ = differ();
        let outcome = differ.observe(
            &image(&[("100", "3"), ("99", "5")]),
            &image(&[("101", "4")]),
        );
        let ImageOutcome::Snapshot(snapshot) = outcome else {
            panic!("expected a snapshot, got {outcome:?}");
        };
        assert_eq!(snapshot.last_update_id(), 1);
        // Normalized: ascending by exact price, whatever order the source used.
        assert_eq!(
            snapshot
                .bids()
                .iter()
                .map(|level| level.price())
                .collect::<Vec<_>>(),
            vec![dec("99"), dec("100")]
        );
        assert_eq!(
            snapshot.coverage(),
            BookCoverage::Limited {
                levels_per_side: 10
            }
        );
        assert_eq!(differ.level_counts(), (2, 1));
        assert!(differ.is_initialized());
    }

    #[test]
    fn an_identical_image_publishes_nothing() {
        let mut differ = differ();
        let bids = image(&[("100", "3")]);
        let asks = image(&[("101", "4")]);
        assert!(matches!(
            differ.observe(&bids, &asks),
            ImageOutcome::Snapshot(_)
        ));
        assert_eq!(differ.observe(&bids, &asks), ImageOutcome::Unchanged);
        // A reordered image is still the same image.
        assert_eq!(
            differ.observe(&image(&[("100", "3")]), &image(&[("101", "4")])),
            ImageOutcome::Unchanged
        );
        assert_eq!(differ.last_update_id(), 1);
    }

    #[test]
    fn only_changed_levels_reach_the_delta() {
        let mut differ = differ();
        differ.observe(
            &image(&[("99", "5"), ("100", "3")]),
            &image(&[("101", "4"), ("102", "7")]),
        );
        // 99 unchanged, 100 shrinks, 98 appears; 101 vanishes, 102 unchanged.
        let outcome = differ.observe(
            &image(&[("98", "2"), ("99", "5"), ("100", "1")]),
            &image(&[("102", "7")]),
        );
        let ImageOutcome::Delta(delta) = outcome else {
            panic!("expected a delta, got {outcome:?}");
        };
        assert_eq!(delta.first_update_id(), 2);
        assert_eq!(delta.final_update_id(), 2);
        assert_eq!(
            delta
                .bids()
                .iter()
                .map(|l| (l.price(), l.quantity()))
                .collect::<Vec<_>>(),
            vec![(dec("98"), dec("2")), (dec("100"), dec("1"))]
        );
        assert_eq!(
            delta
                .asks()
                .iter()
                .map(|l| (l.price(), l.quantity()))
                .collect::<Vec<_>>(),
            vec![(dec("101"), Decimal::ZERO)]
        );
    }

    #[test]
    fn market_orders_and_empty_rows_are_dropped() {
        // MT5 reports market-order rows with price 0 and pads with empties.
        let mut differ = differ();
        let bids = vec![
            // A price-0 row never passes the checked constructor, which is
            // exactly the shape the terminal reports for market orders.
            level(Decimal::ZERO, dec("40")),
            level(dec("100"), dec("3")),
            level(dec("99"), Decimal::ZERO),
        ];
        let ImageOutcome::Snapshot(snapshot) = differ.observe(&bids, &image(&[("101", "1")]))
        else {
            panic!("expected a snapshot");
        };
        assert_eq!(snapshot.bids().len(), 1);
        assert_eq!(snapshot.bids()[0].price(), dec("100"));
    }

    #[test]
    fn duplicate_prices_are_summed_not_duplicated() {
        let mut differ = differ();
        let ImageOutcome::Snapshot(snapshot) =
            differ.observe(&image(&[("100", "3"), ("100", "4")]), &[])
        else {
            panic!("expected a snapshot");
        };
        assert_eq!(snapshot.bids().len(), 1);
        assert_eq!(snapshot.bids()[0].quantity(), dec("7"));
    }

    #[test]
    fn a_crossed_image_is_rejected_and_the_previous_one_survives() {
        let mut differ = differ();
        differ.observe(&image(&[("100", "3")]), &image(&[("101", "4")]));
        let outcome = differ.observe(&image(&[("102", "3")]), &image(&[("101", "4")]));
        assert_eq!(
            outcome,
            ImageOutcome::Crossed {
                best_bid: dec("102"),
                best_ask: dec("101"),
            }
        );
        assert_eq!(differ.last_update_id(), 1);
        assert_eq!(differ.level_counts(), (1, 1));
        // A locked book (bid == ask) is refused for the same reason.
        assert!(matches!(
            differ.observe(&image(&[("101", "3")]), &image(&[("101", "4")])),
            ImageOutcome::Crossed { .. }
        ));
        // And the stream resumes from the surviving image, no snapshot needed.
        assert!(matches!(
            differ.observe(&image(&[("100", "9")]), &image(&[("101", "4")])),
            ImageOutcome::Delta(_)
        ));
    }

    #[test]
    fn a_reset_starts_a_new_snapshot_without_reusing_update_ids() {
        let mut differ = differ();
        differ.observe(&image(&[("100", "3")]), &image(&[("101", "4")]));
        differ.observe(&image(&[("100", "5")]), &image(&[("101", "4")]));
        assert_eq!(differ.last_update_id(), 2);

        differ.reset();
        assert!(!differ.is_initialized());
        let ImageOutcome::Snapshot(snapshot) =
            differ.observe(&image(&[("100", "3")]), &image(&[("101", "4")]))
        else {
            panic!("expected a fresh snapshot after the reset");
        };
        assert_eq!(snapshot.last_update_id(), 3);
    }

    #[test]
    fn the_emitted_stream_replays_into_an_orderbook() {
        // The whole point: an image-only source drives the same core as a
        // diff-based one, and the book stays equal to the latest image.
        let mut differ = differ();
        let mut book = OrderBook::new();
        let images = [
            (vec![("99", "5"), ("100", "3")], vec![("101", "4")]),
            (vec![("99", "5"), ("100", "8")], vec![("101", "4")]),
            (vec![("99", "5")], vec![("101", "1"), ("102", "6")]),
        ];
        for (bids, asks) in &images {
            match differ.observe(&image(bids), &image(asks)) {
                ImageOutcome::Snapshot(snapshot) => book.install_snapshot(snapshot).unwrap(),
                ImageOutcome::Delta(delta) => {
                    book.apply_delta(&delta).unwrap();
                }
                other => panic!("unexpected outcome {other:?}"),
            }
        }
        let (last_bids, last_asks) = images.last().unwrap();
        assert_eq!(book.bid_count(), last_bids.len());
        assert_eq!(book.ask_count(), last_asks.len());
        assert_eq!(book.last_update_id(), Some(differ.last_update_id()));
    }
}
