//! Test-only memoization prototype; production placement remains the baseline.

use super::*;
use std::collections::{BTreeMap, BTreeSet};

const EPOCH: i64 = 1_790_102_415_000;
const CELLS: usize = 6_500;

struct Fixture {
    clusters: Vec<AggressionCluster>,
    timeline: BarTimeline,
    dots: VolumeDots,
    prices: PriceWindow,
    reference: Decimal,
}

impl Fixture {
    fn new(diverse: bool) -> Self {
        let config = config();
        let now = EPOCH + 14_099;
        let quantities = [1, 1, 1, 2, 2, 3, 5, 10, 20, 50, 100];
        let raw: Vec<_> = (0..CELLS)
            .map(|index| Aggression {
                agg_id: index as u64 + 1,
                timestamp_ms: EPOCH + 1_001 + (index / 50) as i64 * 100,
                price: Decimal::from(187_450 + (if diverse { index } else { index % 50 }) * 5),
                quantity: if diverse {
                    Decimal::new(index as i64 + 1, 2)
                } else {
                    Decimal::from(quantities[index % quantities.len()])
                },
                side: if index.is_multiple_of(3) {
                    Side::Sell
                } else {
                    Side::Buy
                },
                generation: None,
            })
            .collect();
        let dots = VolumeDots {
            tape_only: true,
            tape_window_ms: 100,
            tape_level_ticks: 1,
            candle_level_ticks: 1,
            bars: Vec::new(),
            forming: None,
        };
        let clusters = fold_dots(
            cluster_aggressions(&raw, &[], native_grouping(&config), 0),
            true,
            &dots,
            native_grouping(&config),
            DotHorizon {
                recorded_from_ms: None,
                evicted_through_ms: None,
            },
        );
        assert_eq!(clusters.len(), CELLS);
        Self {
            clusters,
            timeline: BarTimeline::from_bars(
                0,
                &[],
                Some(&bar(EPOCH, now)),
                Some(crate::LiveEdge {
                    now_ms: now,
                    window_ms: 15_000,
                    reference_ms: 15_000,
                    on_newest_bar: true,
                }),
            )
            .with_full_lane_coverage(),
            dots,
            prices: PriceWindow::new(
                dec("187400"),
                if diverse {
                    dec("220000")
                } else {
                    dec("187750")
                },
            )
            .unwrap(),
            reference: dec("100"),
        }
    }

    fn current(&self) -> Vec<AggressionPrimitive> {
        tier_primitives(
            TierClusters {
                tape: self.clusters.clone(),
                slot: Vec::new(),
                tape_facts: None,
            },
            &self.timeline,
            self.prices,
            self.reference,
            self.reference,
            Some(&self.dots),
        )
    }

    /// Prototype only: the memo tables live for one native tier call and
    /// retain the exact existing functions as their sole value producers.
    fn memoized(&self) -> Vec<AggressionPrimitive> {
        let mut y_by_price = BTreeMap::new();
        let mut size_by_quantity = BTreeMap::new();
        let (from, now) = self.timeline.lane_bounds_ms().unwrap();
        let right = self.timeline.live_now_position().unwrap().normalized;
        let left = self
            .timeline
            .locate_in_lane_clamped(from)
            .unwrap()
            .normalized;
        let from = Decimal::from(from);
        let duration = Decimal::from(
            now.saturating_sub(self.timeline.lane_start_ms().unwrap())
                .max(1),
        );
        let now_window = window_start(now, self.dots.tape_window_ms);
        self.clusters
            .clone()
            .into_iter()
            .filter_map(|cluster| {
                let x = if window_start(cluster.last_timestamp_ms, self.dots.tape_window_ms)
                    == now_window
                {
                    right
                } else {
                    let mean = cluster.timestamp_quantity / cluster.quantity;
                    let fraction = ((mean - from) / duration).to_f64()?;
                    left + (right - left) * fraction.clamp(0.0, 1.0)
                };
                let y = (*y_by_price
                    .entry(cluster.price)
                    .or_insert_with(|| self.prices.y_unclamped(cluster.price)))?;
                let size = *size_by_quantity
                    .entry(cluster.quantity)
                    .or_insert_with(|| normalized_area_size(cluster.quantity, self.reference));
                Some(primitive_at(cluster, true, x, y, size))
            })
            .collect()
    }
}

#[test]
fn native_price_and_quantity_memoization_prototype_preserves_every_primitive_field() {
    for diverse in [false, true] {
        let fixture = Fixture::new(diverse);
        let current = fixture.current();
        assert_eq!(current.len(), CELLS);
        assert_eq!(fixture.memoized(), current);
    }
}

#[test]
#[ignore = "opt-in prototype measurement; no elapsed-time pass threshold"]
fn native_price_and_quantity_memoization_cost() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};

    for diverse in [false, true] {
        let fixture = Fixture::new(diverse);
        let expected = fixture.current();
        assert_eq!(fixture.memoized(), expected);
        let unique_prices = fixture
            .clusters
            .iter()
            .map(|cluster| cluster.price)
            .collect::<BTreeSet<_>>()
            .len();
        let unique_quantities = fixture
            .clusters
            .iter()
            .map(|cluster| cluster.quantity)
            .collect::<BTreeSet<_>>()
            .len();
        let iterations = 40;
        let mut current = Duration::ZERO;
        let mut memoized = Duration::ZERO;
        for index in 0..iterations {
            // Alternate order so neither implementation always follows a cold
            // cache or gets all of a later stretch of processor scheduling.
            for memo in [index % 2 == 0, index % 2 != 0] {
                let started = Instant::now();
                black_box(if memo {
                    fixture.memoized()
                } else {
                    fixture.current()
                });
                if memo {
                    memoized += started.elapsed();
                } else {
                    current += started.elapsed();
                }
            }
        }
        assert_eq!(fixture.memoized(), expected);
        let per_frame = |elapsed: Duration| elapsed.as_secs_f64() * 1_000.0 / f64::from(iterations);
        eprintln!(
            "native placement prototype: {CELLS} cells, {unique_prices} prices, {unique_quantities} quantities, {iterations} frames, current {:.3} ms/frame, memoized {:.3} ms/frame",
            per_frame(current),
            per_frame(memoized)
        );
    }
}
