use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;

use crate::model::validate_level;
use crate::{BookCoverage, BookDelta, BookError, BookLevel, BookSide, BookSnapshot};

/// Result of applying a structurally valid depth event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// The event advanced the local book.
    Applied {
        /// First exchange update id in the event.
        first_update_id: u64,
        /// Final exchange update id in the event and new local update id.
        final_update_id: u64,
        /// Number of levels whose stored value actually changed.
        changed_levels: usize,
    },
    /// The event was already fully represented by the local book.
    Stale {
        /// Final update id of the ignored event.
        final_update_id: u64,
        /// Update id already represented by the local book.
        current_update_id: u64,
    },
}

/// A deterministic, locally reconstructed limit order book.
///
/// Levels are ordered by exact [`Decimal`] price. Incoming updates are absolute:
/// a positive quantity replaces the stored quantity and zero removes the level.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OrderBook {
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
    last_update_id: Option<u64>,
    coverage: Option<BookCoverage>,
}

impl OrderBook {
    /// Construct an empty, uninitialized book.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Atomically replace the entire local state with `snapshot`.
    ///
    /// Zero-quantity snapshot entries represent absent levels and are omitted.
    /// Every level, duplicate and crossed-book invariant is validated against
    /// temporary maps before any field in `self` changes.
    ///
    /// # Errors
    ///
    /// Returns a typed [`BookError`] for invalid levels, duplicate side/price
    /// entries, or a crossed/locked resulting book. On error, `self` is
    /// unchanged.
    pub fn install_snapshot(&mut self, snapshot: BookSnapshot) -> Result<(), BookError> {
        let (last_update_id, bids, asks, coverage) = snapshot.into_parts();
        let next_bids = build_side(BookSide::Bid, &bids)?;
        let next_asks = build_side(BookSide::Ask, &asks)?;
        validate_not_crossed(&next_bids, &next_asks)?;

        self.bids = next_bids;
        self.asks = next_asks;
        self.last_update_id = Some(last_update_id);
        self.coverage = Some(coverage);
        Ok(())
    }

    /// Apply one absolute depth event atomically.
    ///
    /// An event is stale when its final update id is already represented. An
    /// overlapping event is accepted when it covers the next required update
    /// id. An event beginning after that id is a typed sequence gap.
    ///
    /// # Errors
    ///
    /// Returns [`BookError::NotInitialized`] before the first snapshot,
    /// [`BookError::InvalidSequence`] for an inverted range,
    /// [`BookError::SequenceGap`] for a hole, or an invariant error. On every
    /// error, the book and its update id remain unchanged.
    pub fn apply_delta(&mut self, delta: &BookDelta) -> Result<ApplyOutcome, BookError> {
        validate_delta(delta)?;

        let Some(current_update_id) = self.last_update_id else {
            return Err(BookError::NotInitialized);
        };

        if delta.final_update_id() <= current_update_id {
            return Ok(ApplyOutcome::Stale {
                final_update_id: delta.final_update_id(),
                current_update_id,
            });
        }

        let expected_update_id = current_update_id.saturating_add(1);
        if delta.first_update_id() > expected_update_id {
            return Err(BookError::SequenceGap {
                expected_update_id,
                first_update_id: delta.first_update_id(),
                final_update_id: delta.final_update_id(),
            });
        }

        let mut next_bids = self.bids.clone();
        let mut next_asks = self.asks.clone();
        let changed_levels =
            apply_levels(&mut next_bids, delta.bids()) + apply_levels(&mut next_asks, delta.asks());
        validate_not_crossed(&next_bids, &next_asks)?;

        self.bids = next_bids;
        self.asks = next_asks;
        self.last_update_id = Some(delta.final_update_id());
        Ok(ApplyOutcome::Applied {
            first_update_id: delta.first_update_id(),
            final_update_id: delta.final_update_id(),
            changed_levels,
        })
    }

    /// Whether a snapshot has initialized the book.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.last_update_id.is_some()
    }

    /// Update id currently represented by the book.
    #[must_use]
    pub fn last_update_id(&self) -> Option<u64> {
        self.last_update_id
    }

    /// Coverage declared by the installed snapshot.
    #[must_use]
    pub fn coverage(&self) -> Option<BookCoverage> {
        self.coverage
    }

    /// All bids, ordered from lowest to highest price.
    #[must_use]
    pub fn bids(&self) -> &BTreeMap<Decimal, Decimal> {
        &self.bids
    }

    /// All asks, ordered from lowest to highest price.
    #[must_use]
    pub fn asks(&self) -> &BTreeMap<Decimal, Decimal> {
        &self.asks
    }

    /// Number of stored bid levels.
    #[must_use]
    pub fn bid_count(&self) -> usize {
        self.bids.len()
    }

    /// Number of stored ask levels.
    #[must_use]
    pub fn ask_count(&self) -> usize {
        self.asks.len()
    }

    /// Highest bid, if present.
    #[must_use]
    pub fn best_bid(&self) -> Option<BookLevel> {
        self.bids
            .last_key_value()
            .map(|(&price, &quantity)| BookLevel { price, quantity })
    }

    /// Lowest ask, if present.
    #[must_use]
    pub fn best_ask(&self) -> Option<BookLevel> {
        self.asks
            .first_key_value()
            .map(|(&price, &quantity)| BookLevel { price, quantity })
    }
}

fn build_side(
    side: BookSide,
    levels: &[BookLevel],
) -> Result<BTreeMap<Decimal, Decimal>, BookError> {
    validate_updates(side, levels)?;
    Ok(levels
        .iter()
        .filter(|level| level.quantity > Decimal::ZERO)
        .map(|level| (level.price, level.quantity))
        .collect())
}

fn validate_delta(delta: &BookDelta) -> Result<(), BookError> {
    if delta.first_update_id() > delta.final_update_id() {
        return Err(BookError::InvalidSequence {
            first_update_id: delta.first_update_id(),
            final_update_id: delta.final_update_id(),
        });
    }
    validate_updates(BookSide::Bid, delta.bids())?;
    validate_updates(BookSide::Ask, delta.asks())
}

fn validate_updates(side: BookSide, levels: &[BookLevel]) -> Result<(), BookError> {
    let mut seen = BTreeSet::new();
    for &level in levels {
        validate_level(side, level)?;
        if !seen.insert(level.price) {
            return Err(BookError::DuplicatePrice {
                side,
                price: level.price,
            });
        }
    }
    Ok(())
}

fn apply_levels(book: &mut BTreeMap<Decimal, Decimal>, levels: &[BookLevel]) -> usize {
    let mut changed = 0;
    for level in levels {
        if level.quantity == Decimal::ZERO {
            changed += usize::from(book.remove(&level.price).is_some());
        } else if book.get(&level.price) != Some(&level.quantity) {
            book.insert(level.price, level.quantity);
            changed += 1;
        }
    }
    changed
}

fn validate_not_crossed(
    bids: &BTreeMap<Decimal, Decimal>,
    asks: &BTreeMap<Decimal, Decimal>,
) -> Result<(), BookError> {
    if let (Some((&best_bid, _)), Some((&best_ask, _))) =
        (bids.last_key_value(), asks.first_key_value())
        && best_bid >= best_ask
    {
        return Err(BookError::CrossedBook { best_bid, best_ask });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr as _;

    fn dec(value: &str) -> Decimal {
        Decimal::from_str(value).unwrap()
    }

    fn level(price: &str, quantity: &str) -> BookLevel {
        BookLevel::new(dec(price), dec(quantity)).unwrap()
    }

    fn snapshot(last_update_id: u64) -> BookSnapshot {
        BookSnapshot::new(
            last_update_id,
            vec![level("99", "4"), level("100", "3")],
            vec![level("101", "2"), level("102", "5")],
            BookCoverage::Limited {
                levels_per_side: 5_000,
            },
        )
    }

    fn initialized(last_update_id: u64) -> OrderBook {
        let mut book = OrderBook::new();
        book.install_snapshot(snapshot(last_update_id)).unwrap();
        book
    }

    #[test]
    fn snapshot_initializes_levels_accessors_and_best_prices() {
        let mut book = OrderBook::new();
        assert!(!book.is_initialized());

        book.install_snapshot(snapshot(10)).unwrap();

        assert!(book.is_initialized());
        assert_eq!(book.last_update_id(), Some(10));
        assert_eq!(
            book.coverage(),
            Some(BookCoverage::Limited {
                levels_per_side: 5_000
            })
        );
        assert_eq!(book.bid_count(), 2);
        assert_eq!(book.ask_count(), 2);
        assert_eq!(book.best_bid(), Some(level("100", "3")));
        assert_eq!(book.best_ask(), Some(level("101", "2")));
        assert_eq!(
            book.bids().keys().copied().collect::<Vec<_>>(),
            [dec("99"), dec("100")]
        );
    }

    #[test]
    fn a_new_snapshot_replaces_the_whole_state() {
        let mut book = initialized(10);
        book.install_snapshot(BookSnapshot::new(
            50,
            vec![level("200", "7")],
            vec![level("201", "8")],
            BookCoverage::Full,
        ))
        .unwrap();

        assert_eq!(book.last_update_id(), Some(50));
        assert_eq!(book.coverage(), Some(BookCoverage::Full));
        assert_eq!(book.bids(), &BTreeMap::from([(dec("200"), dec("7"))]));
        assert_eq!(book.asks(), &BTreeMap::from([(dec("201"), dec("8"))]));
    }

    #[test]
    fn zero_quantity_snapshot_levels_are_not_stored() {
        let mut book = OrderBook::new();
        book.install_snapshot(BookSnapshot::new(
            1,
            vec![level("99", "0"), level("100", "2")],
            vec![level("101", "0"), level("102", "3")],
            BookCoverage::Full,
        ))
        .unwrap();

        assert_eq!(book.bid_count(), 1);
        assert_eq!(book.ask_count(), 1);
        assert!(!book.bids().contains_key(&dec("99")));
        assert!(!book.asks().contains_key(&dec("101")));
    }

    #[test]
    fn snapshot_is_atomic_when_crossed() {
        let mut book = initialized(10);
        let before = book.clone();

        let error = book
            .install_snapshot(BookSnapshot::new(
                20,
                vec![level("105", "1")],
                vec![level("104", "1")],
                BookCoverage::Full,
            ))
            .unwrap_err();

        assert_eq!(
            error,
            BookError::CrossedBook {
                best_bid: dec("105"),
                best_ask: dec("104"),
            }
        );
        assert_eq!(book, before);
    }

    #[test]
    fn duplicate_snapshot_level_is_rejected_atomically() {
        let mut book = initialized(10);
        let before = book.clone();

        let error = book
            .install_snapshot(BookSnapshot::new(
                20,
                vec![level("100", "1"), level("100", "2")],
                vec![level("101", "1")],
                BookCoverage::Full,
            ))
            .unwrap_err();

        assert_eq!(
            error,
            BookError::DuplicatePrice {
                side: BookSide::Bid,
                price: dec("100"),
            }
        );
        assert_eq!(book, before);
    }

    #[test]
    fn delta_quantities_are_absolute_not_additive() {
        let mut book = initialized(10);
        let outcome = book
            .apply_delta(&BookDelta::new(
                11,
                11,
                vec![level("100", "9")],
                vec![level("101", "6")],
            ))
            .unwrap();

        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                first_update_id: 11,
                final_update_id: 11,
                changed_levels: 2,
            }
        );
        assert_eq!(book.bids().get(&dec("100")), Some(&dec("9")));
        assert_eq!(book.asks().get(&dec("101")), Some(&dec("6")));
        assert_eq!(book.last_update_id(), Some(11));
    }

    #[test]
    fn zero_quantity_deletes_and_missing_delete_is_a_noop() {
        let mut book = initialized(10);
        let outcome = book
            .apply_delta(&BookDelta::new(
                11,
                12,
                vec![level("100", "0"), level("98", "0")],
                vec![level("101", "0")],
            ))
            .unwrap();

        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                first_update_id: 11,
                final_update_id: 12,
                changed_levels: 2,
            }
        );
        assert_eq!(book.best_bid(), Some(level("99", "4")));
        assert_eq!(book.best_ask(), Some(level("102", "5")));
    }

    #[test]
    fn fully_stale_delta_is_an_idempotent_noop() {
        let mut book = initialized(20);
        let before = book.clone();
        let delta = BookDelta::new(15, 20, vec![level("100", "999")], vec![level("101", "999")]);

        assert_eq!(
            book.apply_delta(&delta).unwrap(),
            ApplyOutcome::Stale {
                final_update_id: 20,
                current_update_id: 20,
            }
        );
        assert_eq!(book, before);
        assert_eq!(
            book.apply_delta(&delta).unwrap(),
            ApplyOutcome::Stale {
                final_update_id: 20,
                current_update_id: 20,
            }
        );
    }

    #[test]
    fn overlapping_delta_covering_the_next_id_is_applied() {
        let mut book = initialized(20);
        let outcome = book
            .apply_delta(&BookDelta::new(18, 23, vec![level("100", "8")], vec![]))
            .unwrap();

        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                first_update_id: 18,
                final_update_id: 23,
                changed_levels: 1,
            }
        );
        assert_eq!(book.last_update_id(), Some(23));
    }

    #[test]
    fn sequence_gap_is_typed_and_atomic() {
        let mut book = initialized(20);
        let before = book.clone();

        let error = book
            .apply_delta(&BookDelta::new(22, 25, vec![level("100", "8")], vec![]))
            .unwrap_err();

        assert_eq!(
            error,
            BookError::SequenceGap {
                expected_update_id: 21,
                first_update_id: 22,
                final_update_id: 25,
            }
        );
        assert_eq!(book, before);
    }

    #[test]
    fn invalid_sequence_is_typed_and_atomic() {
        let mut book = initialized(20);
        let before = book.clone();

        let error = book
            .apply_delta(&BookDelta::new(22, 21, vec![], vec![]))
            .unwrap_err();

        assert_eq!(
            error,
            BookError::InvalidSequence {
                first_update_id: 22,
                final_update_id: 21,
            }
        );
        assert_eq!(book, before);
    }

    #[test]
    fn delta_before_snapshot_is_rejected() {
        let mut book = OrderBook::new();
        assert_eq!(
            book.apply_delta(&BookDelta::new(1, 1, vec![], vec![])),
            Err(BookError::NotInitialized)
        );
    }

    #[test]
    fn a_crossing_delta_does_not_partially_mutate_the_book() {
        let mut book = initialized(10);
        let before = book.clone();

        let error = book
            .apply_delta(&BookDelta::new(
                11,
                11,
                vec![level("103", "10"), level("99", "20")],
                vec![],
            ))
            .unwrap_err();

        assert_eq!(
            error,
            BookError::CrossedBook {
                best_bid: dec("103"),
                best_ask: dec("101"),
            }
        );
        assert_eq!(book, before);
    }

    #[test]
    fn locked_book_is_also_rejected() {
        let mut book = initialized(10);
        let before = book.clone();

        let error = book
            .apply_delta(&BookDelta::new(11, 11, vec![level("101", "1")], vec![]))
            .unwrap_err();

        assert!(matches!(error, BookError::CrossedBook { .. }));
        assert_eq!(book, before);
    }

    #[test]
    fn duplicate_delta_level_is_rejected_before_mutation() {
        let mut book = initialized(10);
        let before = book.clone();
        let error = book
            .apply_delta(&BookDelta::new(
                11,
                11,
                vec![level("100", "7"), level("100", "8")],
                vec![],
            ))
            .unwrap_err();

        assert_eq!(
            error,
            BookError::DuplicatePrice {
                side: BookSide::Bid,
                price: dec("100"),
            }
        );
        assert_eq!(book, before);
    }

    #[test]
    fn same_absolute_value_advances_sequence_without_counting_a_change() {
        let mut book = initialized(10);
        let outcome = book
            .apply_delta(&BookDelta::new(11, 11, vec![level("100", "3")], vec![]))
            .unwrap();

        assert_eq!(
            outcome,
            ApplyOutcome::Applied {
                first_update_id: 11,
                final_update_id: 11,
                changed_levels: 0,
            }
        );
        assert_eq!(book.last_update_id(), Some(11));
    }

    #[test]
    fn input_order_does_not_affect_canonical_book_state() {
        let first = BookSnapshot::new(
            10,
            vec![level("98", "1"), level("100", "3"), level("99", "2")],
            vec![level("103", "6"), level("101", "4"), level("102", "5")],
            BookCoverage::Full,
        );
        let second = BookSnapshot::new(
            10,
            vec![level("100", "3"), level("99", "2"), level("98", "1")],
            vec![level("102", "5"), level("103", "6"), level("101", "4")],
            BookCoverage::Full,
        );
        let mut a = OrderBook::new();
        let mut b = OrderBook::new();
        a.install_snapshot(first).unwrap();
        b.install_snapshot(second).unwrap();

        a.apply_delta(&BookDelta::new(
            11,
            11,
            vec![level("99", "9"), level("97", "7")],
            vec![level("102", "0"), level("104", "8")],
        ))
        .unwrap();
        b.apply_delta(&BookDelta::new(
            11,
            11,
            vec![level("97", "7"), level("99", "9")],
            vec![level("104", "8"), level("102", "0")],
        ))
        .unwrap();

        assert_eq!(a, b);
        assert_eq!(
            a.bids().iter().map(|(&p, &q)| (p, q)).collect::<Vec<_>>(),
            [
                (dec("97"), dec("7")),
                (dec("98"), dec("1")),
                (dec("99"), dec("9")),
                (dec("100"), dec("3")),
            ]
        );
    }
}
