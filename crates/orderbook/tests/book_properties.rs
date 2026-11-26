//! Properties of `OrderBook::apply_delta` over generated books and depth
//! events: a successful apply never leaves the book crossed or locked, a
//! failed one leaves it untouched, an apply matches the obvious
//! clone-apply-check model, and installing a snapshot equals replaying the
//! deltas that reach it.

use std::collections::BTreeMap;

use proptest::prelude::*;
use quantick_orderbook::{
    ApplyOutcome, BookCoverage, BookDelta, BookError, BookLevel, BookSnapshot, OrderBook,
};
use rust_decimal::Decimal;

/// Prices 1..=MAX_PRICE: small enough that generated updates hit stored
/// levels, cross the touch and remove the best level often.
const MAX_PRICE: i64 = 40;
/// Bids are generated at or below this price and asks above it, so every
/// generated snapshot is valid.
const MID_PRICE: i64 = MAX_PRICE / 2;

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(Decimal::from(price), Decimal::from(quantity)).unwrap()
}

fn levels(map: &BTreeMap<Decimal, Decimal>) -> Vec<BookLevel> {
    map.iter()
        .map(|(&price, &quantity)| BookLevel::new(price, quantity).unwrap())
        .collect()
}

/// One side of a valid snapshot: unique prices in `low..=high`, positive
/// quantities.
fn snapshot_side(low: i64, high: i64) -> impl Strategy<Value = Vec<BookLevel>> {
    prop::collection::btree_map(low..=high, 1_i64..=5, 0..=12)
        .prop_map(|map| map.into_iter().map(|(p, q)| level(p, q)).collect())
}

fn snapshot() -> impl Strategy<Value = BookSnapshot> {
    (
        snapshot_side(1, MID_PRICE),
        snapshot_side(MID_PRICE + 1, MAX_PRICE),
        prop_oneof![
            Just(BookCoverage::Full),
            Just(BookCoverage::Limited {
                levels_per_side: 12
            }),
        ],
    )
        .prop_map(|(bids, asks, coverage)| BookSnapshot::new(10, bids, asks, coverage))
}

/// How a generated update list is ordered: as a venue sends it, or not.
#[derive(Clone, Copy, Debug)]
enum Order {
    Ascending,
    Descending,
    Shuffled,
}

/// One side of a depth event: prices anywhere in the range, zero removes,
/// any order, and sometimes a duplicate price.
fn update_side() -> impl Strategy<Value = Vec<BookLevel>> {
    (
        prop::collection::btree_map(1..=MAX_PRICE, 0_i64..=4, 0..=6),
        prop_oneof![
            Just(Order::Ascending),
            Just(Order::Descending),
            Just(Order::Shuffled),
        ],
        prop::bool::weighted(0.1),
        any::<prop::sample::Index>(),
    )
        .prop_flat_map(|(map, order, duplicate, at)| {
            let mut side: Vec<BookLevel> = map.into_iter().map(|(p, q)| level(p, q)).collect();
            if duplicate && !side.is_empty() {
                let copy = side[at.index(side.len())];
                side.push(BookLevel::new(copy.price(), copy.quantity() + Decimal::ONE).unwrap());
            }
            match order {
                Order::Ascending => Just(side).boxed(),
                Order::Descending => {
                    side.reverse();
                    Just(side).boxed()
                }
                Order::Shuffled => Just(side).prop_shuffle().boxed(),
            }
        })
}

/// A depth event without ids: its levels and whether it skips an update id.
#[derive(Clone, Debug)]
struct Event {
    bids: Vec<BookLevel>,
    asks: Vec<BookLevel>,
    gap: bool,
}

fn event() -> impl Strategy<Value = Event> {
    (update_side(), update_side(), prop::bool::weighted(0.05)).prop_map(|(bids, asks, gap)| Event {
        bids,
        asks,
        gap,
    })
}

/// The next delta for `book`: one update id after it, or two on a gap.
fn delta_for(book: &OrderBook, event: &Event) -> BookDelta {
    let next = book.last_update_id().unwrap() + if event.gap { 2 } else { 1 };
    BookDelta::new(next, next, event.bids.clone(), event.asks.clone())
}

/// The definition `apply_delta` must match: apply to copies, then check.
fn model_apply(book: &OrderBook, event: &Event) -> Option<OrderBook> {
    let unique = |side: &[BookLevel]| {
        let mut prices: Vec<Decimal> = side.iter().map(|level| level.price()).collect();
        prices.sort();
        prices.windows(2).all(|pair| pair[0] != pair[1])
    };
    if event.gap || !unique(&event.bids) || !unique(&event.asks) {
        return None;
    }
    let apply = |map: &BTreeMap<Decimal, Decimal>, side: &[BookLevel]| {
        let mut next = map.clone();
        for level in side {
            if level.quantity().is_zero() {
                next.remove(&level.price());
            } else {
                next.insert(level.price(), level.quantity());
            }
        }
        next
    };
    let bids = apply(book.bids(), &event.bids);
    let asks = apply(book.asks(), &event.asks);
    if let (Some(bid), Some(ask)) = (bids.keys().next_back(), asks.keys().next())
        && bid >= ask
    {
        return None;
    }
    let mut expected = OrderBook::new();
    expected
        .install_snapshot(BookSnapshot::new(
            book.last_update_id().unwrap() + 1,
            levels(&bids),
            levels(&asks),
            book.coverage().unwrap(),
        ))
        .unwrap();
    Some(expected)
}

fn is_crossed(book: &OrderBook) -> bool {
    matches!(
        (book.best_bid(), book.best_ask()),
        (Some(bid), Some(ask)) if bid.price() >= ask.price()
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn apply_is_atomic_never_crosses_and_matches_the_model(
        snapshot in snapshot(),
        events in prop::collection::vec(event(), 1..=24),
    ) {
        let mut book = OrderBook::new();
        book.install_snapshot(snapshot).unwrap();
        for event in &events {
            let before = book.clone();
            let expected = model_apply(&before, event);
            match book.apply_delta(&delta_for(&before, event)) {
                Ok(ApplyOutcome::Applied { .. }) => {
                    prop_assert!(!is_crossed(&book), "crossed after apply: {:?}", book);
                    prop_assert_eq!(Some(&book), expected.as_ref());
                }
                Ok(stale) => prop_assert!(false, "a fresh event read as stale: {:?}", stale),
                Err(error) => {
                    prop_assert_eq!(&book, &before, "{} changed the book", error);
                    prop_assert!(expected.is_none(), "model applied what failed: {}", error);
                    let expected_kind = matches!(
                        error,
                        BookError::CrossedBook { .. }
                            | BookError::DuplicatePrice { .. }
                            | BookError::SequenceGap { .. }
                    );
                    prop_assert!(expected_kind, "unexpected error: {}", error);
                }
            }
        }
    }

    #[test]
    fn installing_a_snapshot_equals_replaying_its_deltas(
        snapshot in snapshot(),
        events in prop::collection::vec(event(), 0..=24),
    ) {
        // Replaying events reaches some state; a snapshot of that state
        // installs to the same book.
        let mut replayed = OrderBook::new();
        replayed.install_snapshot(snapshot.clone()).unwrap();
        for event in &events {
            let _ = replayed.apply_delta(&delta_for(&replayed, event));
        }
        let mut installed = OrderBook::new();
        installed
            .install_snapshot(BookSnapshot::new(
                replayed.last_update_id().unwrap(),
                levels(replayed.bids()),
                levels(replayed.asks()),
                replayed.coverage().unwrap(),
            ))
            .unwrap();
        prop_assert_eq!(&installed, &replayed);

        // A snapshot's levels sent as one delta onto an empty book install to
        // the same book as the snapshot itself.
        let mut direct = OrderBook::new();
        direct.install_snapshot(snapshot.clone()).unwrap();
        let mut from_empty = OrderBook::new();
        from_empty
            .install_snapshot(BookSnapshot::new(0, vec![], vec![], snapshot.coverage()))
            .unwrap();
        let outcome = from_empty.apply_delta(&BookDelta::new(
            1,
            snapshot.last_update_id(),
            snapshot.bids().to_vec(),
            snapshot.asks().to_vec(),
        ));
        prop_assert!(matches!(outcome, Ok(ApplyOutcome::Applied { .. })), "{:?}", outcome);
        prop_assert_eq!(&from_empty, &direct);
    }
}
