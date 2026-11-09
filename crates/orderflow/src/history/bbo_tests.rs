//! The book's top as the history records it: one step per change of the
//! best bid or best ask, a book-less step at every gap, nothing else.

use super::*;
use crate::history::LiquidityHistory;
use quantick_orderbook::{BookCoverage, BookDelta, BookLevel, BookSnapshot};

fn level(price: i64, quantity: i64) -> BookLevel {
    BookLevel::new(price.into(), quantity.into()).unwrap()
}

fn top(start_ms: i64, generation: Option<u64>, bid: Option<i64>, ask: Option<i64>) -> BboStep {
    BboStep {
        start_ms,
        generation,
        bid: bid.map(Decimal::from),
        ask: ask.map(Decimal::from),
    }
}

/// Bid 950 and ask 960 at 1,000; a quantity-only change at 1,100; the best
/// ask pulled at 1,200; a better bid at 1,300; a gap at 1,400; a new
/// generation at 1,600; a confirmation that changes nothing at 1,700.
pub(crate) fn fixture() -> LiquidityHistory {
    let mut history = LiquidityHistory::new(crate::HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        ..crate::HeatmapConfig::default()
    });
    history
        .install_snapshot(
            1_000,
            1,
            BookSnapshot::new(
                10,
                vec![level(940, 3), level(950, 5)],
                vec![level(960, 2), level(970, 4)],
                BookCoverage::Full,
            ),
        )
        .unwrap();
    let deltas = [
        (11, 1_100, vec![level(950, 6)], vec![]),
        (12, 1_200, vec![], vec![level(960, 0)]),
        (13, 1_300, vec![level(955, 1)], vec![]),
    ];
    for (id, time, bids, asks) in deltas {
        history
            .apply_delta(time, &BookDelta::new(id, id, bids, asks))
            .unwrap();
    }
    history.mark_gap(1_400, "transport_closed").unwrap();
    history
        .install_snapshot(
            1_600,
            2,
            BookSnapshot::new(
                20,
                vec![level(940, 1)],
                vec![level(950, 1)],
                BookCoverage::Full,
            ),
        )
        .unwrap();
    history
        .apply_delta(1_700, &BookDelta::new(21, 21, vec![], vec![]))
        .unwrap();
    history
}

#[test]
fn the_trail_records_each_change_of_the_top_and_a_bookless_step_at_a_gap() {
    let history = fixture();
    let steps: Vec<_> = history.bbo().steps().cloned().collect();
    assert_eq!(
        steps,
        vec![
            top(1_000, Some(1), Some(950), Some(960)),
            top(1_200, Some(1), Some(950), Some(970)),
            top(1_300, Some(1), Some(955), Some(970)),
            top(1_400, None, None, None),
            top(1_600, Some(2), Some(940), Some(950)),
        ],
        "a quantity change and a bare confirmation move no step"
    );
}

#[test]
fn a_same_instant_change_replaces_the_step_rather_than_stacking_a_zero_length_one() {
    let mut trail = BboTrail::default();
    let mut book = OrderBook::new();
    book.install_snapshot(BookSnapshot::new(
        1,
        vec![level(10, 1)],
        vec![level(12, 1)],
        BookCoverage::Full,
    ))
    .unwrap();
    trail.observe(100, Some(1), &book);
    book.apply_delta(&BookDelta::new(2, 2, vec![level(11, 1)], vec![]))
        .unwrap();
    trail.observe(100, Some(1), &book);
    assert_eq!(
        trail.steps().cloned().collect::<Vec<_>>(),
        vec![top(100, Some(1), Some(11), Some(12))]
    );
    book.apply_delta(&BookDelta::new(3, 3, vec![level(11, 0)], vec![]))
        .unwrap();
    trail.observe(200, Some(1), &book);
    book.apply_delta(&BookDelta::new(4, 4, vec![level(11, 1)], vec![]))
        .unwrap();
    trail.observe(200, Some(1), &book);
    assert_eq!(
        trail.steps().cloned().collect::<Vec<_>>(),
        vec![top(100, Some(1), Some(11), Some(12))],
        "a change undone at its own instant leaves no step behind"
    );
}

#[test]
fn eviction_keeps_the_step_in_force_at_the_cutoff() {
    let history = fixture();
    let mut trail = history.bbo().clone();
    trail.truncate_before(1_250);
    assert_eq!(
        trail.steps().map(|step| step.start_ms).collect::<Vec<_>>(),
        vec![1_200, 1_300, 1_400, 1_600],
        "the 1,200 top still holds at 1,250, so it stays"
    );
    trail.truncate_before(5_000);
    assert_eq!(
        trail.steps().map(|step| step.start_ms).collect::<Vec<_>>(),
        vec![1_600]
    );
}

#[test]
fn regrouping_the_capture_forgets_the_trail_with_the_book() {
    let mut history = fixture();
    history.reset_price_grouping(Decimal::TEN).unwrap();
    assert_eq!(history.bbo().steps().count(), 0);
}

/// The byte cap reads the history without the trail: a history whose trail
/// is fed evicts at exactly the same point as one whose trail stays empty.
#[test]
fn the_trail_never_moves_the_byte_cap() {
    let config = crate::HeatmapConfig {
        enabled: true,
        price_grouping: Decimal::ONE,
        max_history_bytes: 4 * 1_024,
        ..crate::HeatmapConfig::default()
    };
    let (mut fed, mut bare) = (
        LiquidityHistory::new(config.clone()),
        LiquidityHistory::new(config),
    );
    let book = BookSnapshot::new(
        1,
        vec![level(99, 1), level(100, 1)],
        vec![level(101, 1)],
        BookCoverage::Full,
    );
    for history in [&mut fed, &mut bare] {
        history.install_snapshot(1_000, 1, book.clone()).unwrap();
    }
    for id in 2..400_u64 {
        // The best bid is pulled and restored: every delta moves the top.
        let quantity = i64::try_from(id % 2).unwrap();
        let delta = BookDelta::new(id, id, vec![level(100, quantity)], vec![]);
        let time = 1_000 + i64::try_from(id).unwrap();
        for history in [&mut fed, &mut bare] {
            history.apply_delta(time, &delta).unwrap();
        }
        bare.bbo.clear();
        assert_eq!(
            fed.approximate_history_bytes(),
            bare.approximate_history_bytes(),
            "at delta {id}"
        );
        assert_eq!(fed.counters(), bare.counters(), "at delta {id}");
    }
    assert!(fed.counters().runs_evicted > 0, "the cap evicted");
    assert!(fed.bbo().steps().count() > 1, "the trail was fed");
}
