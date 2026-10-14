use super::*;
use crate::config::DisplayGrouping;

#[test]
fn cached_rows_match_exact_clipping_on_each_price_grid() {
    for multiple in [1, 2, 7] {
        let grouping = EffectiveGrouping::resolve(
            DisplayGrouping::Multiple(multiple),
            Decimal::new(5, 2),
            Decimal::ONE,
        );
        for (low, high) in [(-37, 49), (8_328_403, 8_338_117)] {
            let prices = PriceWindow::new(Decimal::new(low, 2), Decimal::new(high, 2)).unwrap();
            let mut cached = PriceRows::new(grouping, prices);
            for _ in 0..3 {
                for offset in -20..120 {
                    let bucket = Decimal::new(low + offset, 2);
                    assert_eq!(cached.get(bucket), bucket_rows(bucket, grouping, prices));
                }
            }
            assert_eq!(cached.rows.len(), 140);
        }
    }
}

fn drafts(count: usize) -> Vec<DraftCell> {
    (0..count)
        .map(|index| {
            // Repeated rank keys deliberately differ in fields the cap does
            // not rank, so a boundary tie must preserve the input's order.
            let key = index % 17;
            DraftCell {
                generation: (key % 3) as u64,
                side: if index.is_multiple_of(2) {
                    RestingSide::Bid
                } else {
                    RestingSide::Ask
                },
                price_bucket: Decimal::new((key % 4) as i64, 1),
                quantity: Decimal::new((key % 7 + 1) as i64, 2),
                x0: (key % 5) as f64 / 10.0,
                x1: (index + 1) as f64 / (count + 1) as f64,
                y0: 0.2,
                y1: 0.3,
                carried: index.is_multiple_of(3),
                time_averaged: false,
                current_book: false,
                current_book_source: None,
            }
        })
        .collect()
}

#[test]
fn selection_keeps_the_stable_full_sort_prefix_and_drop_count() {
    for count in [0, 1, 4, 97, 1_024] {
        for cap in [0, 1, 3, 31, count, count + 1] {
            let mut config = HeatmapConfig {
                max_visible_cells: cap,
                ..HeatmapConfig::default()
            };
            let mut expected = drafts(count);
            if count > cap {
                expected.sort_by(|a, b| {
                    b.quantity
                        .cmp(&a.quantity)
                        .then_with(|| a.generation.cmp(&b.generation))
                        .then_with(|| a.price_bucket.cmp(&b.price_bucket))
                        .then_with(|| a.x0.total_cmp(&b.x0))
                });
                expected.truncate(cap);
            }
            let actual = finish_cells(drafts(count), Decimal::ONE, &config);
            config.max_visible_cells = usize::MAX;
            let (expected, _) = finish_cells(expected, Decimal::ONE, &config);
            assert_eq!(actual, (expected, count.saturating_sub(cap)));
        }
    }
}

#[test]
fn hidden_heat_does_not_report_a_cap_drop() {
    let config = HeatmapConfig {
        max_visible_cells: 1,
        show_liquidity: false,
        ..HeatmapConfig::default()
    };
    assert_eq!(
        finish_cells(drafts(97), Decimal::ONE, &config),
        (Vec::new(), 0)
    );
}

#[test]
fn selected_percentile_matches_the_sorted_positive_rank() {
    for count in [0, 1, 2, 99, 100, 101, 10_000] {
        let values: Vec<_> = (0..count)
            .map(|index| Decimal::new(((index * 37) % 127) as i64 - 8, 3))
            .collect();
        let mut positive: Vec<_> = values
            .iter()
            .copied()
            .filter(|quantity| *quantity > Decimal::ZERO)
            .collect();
        positive.sort_unstable();
        let rank = (99 * positive.len()).div_ceil(100);
        let expected = rank
            .checked_sub(1)
            .map_or(Decimal::ZERO, |index| positive[index]);
        assert_eq!(percentile_99(values.into_iter()), expected);
    }
}
