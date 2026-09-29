//! Native screen precision contract against the independent original oracle.

use super::*;

struct Fixture {
    clusters: Vec<AggressionCluster>,
    timeline: BarTimeline,
    dots: VolumeDots,
    prices: PriceWindow,
}

impl Fixture {
    fn new(epoch: i64, cells: usize, weighted: bool) -> Self {
        let config = config();
        let count = if weighted { 6 } else { 1 };
        let trades: Vec<_> = (0..cells)
            .flat_map(|index| {
                (0..count).map(move |member| Aggression {
                    agg_id: (index * count + member + 1) as u64,
                    timestamp_ms: epoch + 1_001 + (index / 50) as i64 * 100 + member as i64 * 13,
                    price: Decimal::from(187_450 + (index % 50) * 5)
                        + Decimal::new((member % 3) as i64, 2),
                    quantity: Decimal::new(((index * 17 + member * 11) % 397 + 1) as i64, 2),
                    side: if (index + member).is_multiple_of(3) {
                        Side::Sell
                    } else {
                        Side::Buy
                    },
                    generation: None,
                })
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
        let now = epoch + 14_099;
        let clusters = fold_dots(
            cluster_aggressions(&trades, &[], native_grouping(&config), 0),
            true,
            &dots,
            native_grouping(&config),
            DotHorizon {
                recorded_from_ms: None,
                evicted_through_ms: None,
            },
        );
        assert_eq!(clusters.len(), cells);
        Self {
            clusters,
            timeline: BarTimeline::from_bars(
                0,
                &[],
                Some(&bar(epoch, now)),
                Some(crate::LiveEdge {
                    now_ms: now,
                    window_ms: 15_000,
                    reference_ms: 15_000,
                    on_newest_bar: true,
                }),
            )
            .with_full_lane_coverage(),
            dots,
            prices: PriceWindow::new(dec("187440"), dec("187710")).unwrap(),
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
            dec("100"),
            dec("100"),
            Some(&self.dots),
        )
    }

    fn original(&self) -> Vec<AggressionPrimitive> {
        let reference = dec("100");
        self.clusters
            .clone()
            .into_iter()
            .filter_map(|cluster| {
                reference_place(
                    cluster,
                    true,
                    &self.timeline,
                    self.prices,
                    reference,
                    Some(&self.dots),
                )
            })
            .collect()
    }

    fn assert_contract(&self) {
        assert_native_screen_contract(&self.current(), &self.original());
    }

    /// Same frozen arithmetic and owned primitive payloads, without first
    /// materializing a second contiguous buffer of AggressionCluster values.
    fn original_streamed(&self) -> Vec<AggressionPrimitive> {
        let reference = dec("100");
        self.clusters
            .iter()
            .cloned()
            .filter_map(|cluster| {
                reference_place(
                    cluster,
                    true,
                    &self.timeline,
                    self.prices,
                    reference,
                    Some(&self.dots),
                )
            })
            .collect()
    }

    fn retime(&mut self, from: i64) {
        let now = from + 15_000;
        self.timeline = BarTimeline::from_bars(
            0,
            &[],
            Some(&bar(from, now)),
            Some(crate::LiveEdge {
                now_ms: now,
                window_ms: 15_000,
                reference_ms: 15_000,
                on_newest_bar: true,
            }),
        )
        .with_full_lane_coverage();
    }
}

#[test]
#[ignore = "opt-in input-buffer experiment; no elapsed-time pass threshold"]
fn native_placement_input_buffer_cost() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    for weighted in [false, true] {
        let fixture = Fixture::new(1_790_102_415_000, 6_500, weighted);
        let expected = fixture.original();
        assert_eq!(fixture.original_streamed(), expected);
        let mut buffered = Duration::ZERO;
        let mut streamed = Duration::ZERO;
        let iterations = 40;
        for index in 0..iterations {
            for streaming in [index % 2 == 0, index % 2 != 0] {
                let started = Instant::now();
                black_box(if streaming {
                    fixture.original_streamed()
                } else {
                    fixture.original()
                });
                if streaming {
                    streamed += started.elapsed();
                } else {
                    buffered += started.elapsed();
                }
            }
        }
        assert_eq!(fixture.original_streamed(), expected);
        let ms = |elapsed: Duration| elapsed.as_secs_f64() * 1_000.0 / f64::from(iterations);
        eprintln!(
            "native input-buffer experiment: 6500 cells, {} source prints, identical original placement, buffered {:.3} ms/frame, streamed {:.3} ms/frame",
            if weighted { 39_000 } else { 6_500 },
            ms(buffered),
            ms(streamed)
        );
    }
}

#[test]
fn relative_screen_projection_preserves_weighted_facts_across_exchange_epochs() {
    for epoch in [-1_790_102_415_000, 0, 1_790_102_415_000] {
        Fixture::new(epoch, 150, true).assert_contract();
    }
}

#[test]
fn relative_screen_projection_keeps_clip_boundaries_orientation_and_clamping() {
    let mut fixture = Fixture::new(0, 1, false);
    fixture.prices = PriceWindow::new(dec("100"), dec("200")).unwrap();
    for price in [
        "-1000000",
        "99.99999999999999999999999999",
        "100",
        "100.00000000000000000000000001",
        "125",
        "200",
        "200.00000000000000000000000001",
        "225",
        "1000000",
    ] {
        fixture.clusters[0].price = dec(price);
        fixture.assert_contract();
    }
    for (price, expected_y) in [("200", 0.0), ("100", 1.0), ("125", 0.75)] {
        fixture.clusters[0].price = dec(price);
        let y = fixture.current()[0].y;
        assert_eq!(y, expected_y);
        assert_eq!(
            1.0 - y,
            1.0 - expected_y,
            "inverting the rendered axis preserves orientation"
        );
    }
    let (from, now) = fixture.timeline.lane_bounds_ms().unwrap();
    let left = fixture
        .timeline
        .locate_in_lane_clamped(from)
        .unwrap()
        .normalized;
    let right = fixture.timeline.live_now_position().unwrap().normalized;
    for (mean, expected_x) in [
        (from - 1, left),
        (from, left),
        (now, right),
        (now + 1, right),
    ] {
        fixture.clusters[0].timestamp_quantity = Decimal::from(mean) * fixture.clusters[0].quantity;
        fixture.assert_contract();
        assert_eq!(fixture.current()[0].x, expected_x);
    }
    fixture.clusters[0].last_timestamp_ms = 14_001;
    assert_eq!(
        fixture.current()[0].x,
        right,
        "forming windows still ride NOW"
    );
}

#[test]
fn relative_screen_projection_falls_back_when_checked_offset_arithmetic_overflows() {
    let mut fixture = Fixture::new(0, 1, false);
    fixture.retime(i64::MAX - 16_000);
    fixture.clusters[0].quantity = dec("100000000000000000000");
    fixture.clusters[0].timestamp_quantity = Decimal::ZERO;
    let from = fixture.timeline.lane_start_ms().unwrap();
    assert!(
        Decimal::from(from)
            .checked_mul(fixture.clusters[0].quantity)
            .is_none()
    );
    fixture.assert_contract();
    assert_eq!(
        fixture.current()[0].x,
        fixture
            .timeline
            .locate_in_lane_clamped(from)
            .unwrap()
            .normalized
    );

    fixture.retime(-1_000_000_000_000_000_000);
    fixture.clusters[0].quantity = dec("79000000000");
    fixture.clusters[0].timestamp_quantity = Decimal::MAX;
    let from = Decimal::from(fixture.timeline.lane_start_ms().unwrap());
    let product = from.checked_mul(fixture.clusters[0].quantity).unwrap();
    assert!(
        fixture.clusters[0]
            .timestamp_quantity
            .checked_sub(product)
            .is_none()
    );
    fixture.assert_contract();
    assert_eq!(
        fixture.current()[0].x,
        fixture.timeline.live_now_position().unwrap().normalized
    );
}

#[test]
#[ignore = "opt-in production placement measurement against the original oracle; no elapsed-time pass threshold"]
fn relative_screen_projection_cost_for_single_and_weighted_native_cells() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    for weighted in [false, true] {
        let fixture = Fixture::new(1_790_102_415_000, 6_500, weighted);
        fixture.assert_contract();
        let mut original = Duration::ZERO;
        let mut production = Duration::ZERO;
        let iterations = 40;
        for index in 0..iterations {
            for faster in [index % 2 == 0, index % 2 != 0] {
                let started = Instant::now();
                black_box(if faster {
                    fixture.current()
                } else {
                    fixture.original()
                });
                if faster {
                    production += started.elapsed();
                } else {
                    original += started.elapsed();
                }
            }
        }
        fixture.assert_contract();
        let ms = |time: Duration| time.as_secs_f64() * 1_000.0 / f64::from(iterations);
        eprintln!(
            "native screen placement: 6500 native cells, {} source prints, original oracle {:.3} ms/frame, production {:.3} ms/frame",
            if weighted { 39_000 } else { 6_500 },
            ms(original),
            ms(production)
        );
    }
}
