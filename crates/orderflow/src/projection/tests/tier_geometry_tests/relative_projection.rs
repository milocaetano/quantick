//! Screen-only precision experiment; exact execution fields remain the oracle.

use super::*;

const TOLERANCE: f64 = 1e-12;

struct Geometry {
    from: Decimal,
    duration: Decimal,
    left: f64,
    right: f64,
    now_window: i64,
}

impl Geometry {
    fn new(timeline: &BarTimeline) -> Self {
        let (from, now) = timeline.lane_bounds_ms().unwrap();
        Self {
            from: Decimal::from(from),
            duration: Decimal::from(now.saturating_sub(from).max(1)),
            left: timeline.locate_in_lane_clamped(from).unwrap().normalized,
            right: timeline.live_now_position().unwrap().normalized,
            now_window: window_start(now, 100),
        }
    }

    fn original_fraction(&self, cluster: &AggressionCluster) -> f64 {
        ((cluster.timestamp_quantity / cluster.quantity - self.from) / self.duration)
            .to_f64()
            .unwrap()
    }

    fn relative_fraction(&self, cluster: &AggressionCluster) -> Option<f64> {
        let offset = cluster
            .timestamp_quantity
            .checked_sub(self.from.checked_mul(cluster.quantity)?)?;
        Some(offset.to_f64()? / cluster.quantity.to_f64()? / self.duration.to_f64()?)
    }

    fn x(&self, cluster: &AggressionCluster) -> f64 {
        if window_start(cluster.last_timestamp_ms, 100) == self.now_window {
            return self.right;
        }
        let fraction = screen_fraction(self.relative_fraction(cluster), || {
            self.original_fraction(cluster)
        });
        self.left + (self.right - self.left) * fraction.clamp(0.0, 1.0)
    }
}

/// Exact old arithmetic resolves clip boundaries and extreme off-axis values.
/// Keeping its rounding there prevents a tiny approximate sign change from
/// affecting visibility or autoscale admission. The common interior stays fast.
fn screen_fraction(approximate: Option<f64>, original: impl FnOnce() -> f64) -> f64 {
    match approximate {
        Some(value)
            if value.is_finite()
                && (-2.0..=3.0).contains(&value)
                && value.abs() > TOLERANCE
                && (value - 1.0).abs() > TOLERANCE =>
        {
            value
        }
        _ => original(),
    }
}

fn relative_y(prices: PriceWindow, price: Decimal) -> f64 {
    let approximate = prices.high.checked_sub(price).and_then(|offset| {
        let span = prices.high.checked_sub(prices.low)?;
        Some(offset.to_f64()? / span.to_f64()?)
    });
    screen_fraction(approximate, || prices.y_unclamped(price).unwrap())
}

fn primitive_at(cluster: AggressionCluster, x: f64, y: f64, size: f32) -> AggressionPrimitive {
    let buy_share = cluster.buy_share();
    let matched_fraction = cluster.matched_fraction();
    AggressionPrimitive {
        agg_id: cluster.agg_id,
        agg_ids: cluster.agg_ids,
        generation: cluster.generation,
        side: cluster.side,
        consumed_side: cluster.consumed_side,
        quantity: cluster.quantity,
        buy_share,
        live: true,
        price_bucket: cluster.price_bucket,
        price_span: cluster.price_span,
        price: cluster.price,
        trade_count: cluster.trade_count,
        first_timestamp_ms: cluster.first_timestamp_ms,
        last_timestamp_ms: cluster.last_timestamp_ms,
        timestamp_quantity: cluster.timestamp_quantity,
        matched_quantity: cluster.matched_quantity,
        buy_quantity: cluster.buy_quantity,
        matched_fraction,
        liquidity_event_ids: cluster.liquidity_event_ids,
        x,
        y,
        size,
        folded_marks: 0,
    }
}

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

    fn original(&self) -> Vec<AggressionPrimitive> {
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

    fn relative(&self) -> Vec<AggressionPrimitive> {
        let geometry = Geometry::new(&self.timeline);
        let reference = dec("100");
        self.clusters
            .clone()
            .into_iter()
            .map(|cluster| {
                let x = geometry.x(&cluster);
                let y = relative_y(self.prices, cluster.price);
                let size = normalized_area_size(cluster.quantity, reference);
                primitive_at(cluster, x, y, size)
            })
            .collect()
    }

    fn assert_contract(&self) {
        let original = self.original();
        let relative = self.relative();
        assert_eq!(original.len(), relative.len());
        for (expected, mut actual) in original.into_iter().zip(relative) {
            assert!((expected.x - actual.x).abs() <= TOLERANCE);
            assert!((expected.y - actual.y).abs() <= TOLERANCE);
            for (old, new) in [(expected.x, actual.x), (expected.y, actual.y)] {
                assert_eq!((0.0..=1.0).contains(&old), (0.0..=1.0).contains(&new));
                // Even a 16K surface differs by under one millionth of a pixel.
                assert!((old - new).abs() * 16_384.0 < 1e-6);
            }
            actual.x = expected.x;
            actual.y = expected.y;
            assert_eq!(actual, expected, "only final screen coordinates may differ");
        }
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
    let prices = PriceWindow::new(dec("100"), dec("200")).unwrap();
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
        let price = dec(price);
        let old = prices.y_unclamped(price).unwrap();
        let new = relative_y(prices, price);
        assert!((old - new).abs() <= TOLERANCE);
        assert_eq!((0.0..=1.0).contains(&old), (0.0..=1.0).contains(&new));
    }
    assert_eq!(relative_y(prices, prices.high), 0.0);
    assert_eq!(relative_y(prices, prices.low), 1.0);
    assert_eq!(relative_y(prices, dec("125")), 0.75);
    assert_eq!(
        1.0 - relative_y(prices, dec("125")),
        0.25,
        "inverting the rendered axis preserves its orientation"
    );

    let fixture = Fixture::new(0, 1, false);
    let geometry = Geometry::new(&fixture.timeline);
    let mut cluster = fixture.clusters[0].clone();
    for mean in [
        geometry.from - Decimal::ONE,
        geometry.from,
        geometry.from + geometry.duration,
        geometry.from + geometry.duration + Decimal::ONE,
    ] {
        cluster.timestamp_quantity = mean * cluster.quantity;
        let original = geometry.left
            + (geometry.right - geometry.left)
                * geometry.original_fraction(&cluster).clamp(0.0, 1.0);
        assert_eq!(geometry.x(&cluster), original);
    }
    cluster.last_timestamp_ms = 14_001;
    assert_eq!(
        geometry.x(&cluster),
        geometry.right,
        "forming windows still ride NOW"
    );
}

#[test]
fn relative_screen_projection_falls_back_when_checked_offset_arithmetic_overflows() {
    let fixture = Fixture::new(0, 1, false);
    let mut geometry = Geometry::new(&fixture.timeline);
    let mut cluster = fixture.clusters[0].clone();
    geometry.from = Decimal::from(i64::MAX - 15_000);
    cluster.quantity = dec("100000000000000000000");
    cluster.timestamp_quantity = Decimal::ZERO;
    assert!(geometry.from.checked_mul(cluster.quantity).is_none());
    assert!(geometry.relative_fraction(&cluster).is_none());
    assert_eq!(geometry.x(&cluster), geometry.left);

    geometry.from = dec("-1000000000000000000");
    cluster.quantity = dec("79000000000");
    cluster.timestamp_quantity = Decimal::MAX;
    assert!(geometry.from.checked_mul(cluster.quantity).is_some());
    assert!(geometry.relative_fraction(&cluster).is_none());
    assert_eq!(geometry.x(&cluster), geometry.right);
}

#[test]
#[ignore = "opt-in screen precision experiment; no elapsed-time pass threshold"]
fn relative_screen_projection_cost_for_single_and_weighted_native_cells() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    for weighted in [false, true] {
        let fixture = Fixture::new(1_790_102_415_000, 6_500, weighted);
        fixture.assert_contract();
        let mut original = Duration::ZERO;
        let mut relative = Duration::ZERO;
        let iterations = 40;
        for index in 0..iterations {
            for faster in [index % 2 == 0, index % 2 != 0] {
                let started = Instant::now();
                black_box(if faster {
                    fixture.relative()
                } else {
                    fixture.original()
                });
                if faster {
                    relative += started.elapsed();
                } else {
                    original += started.elapsed();
                }
            }
        }
        fixture.assert_contract();
        let ms = |time: Duration| time.as_secs_f64() * 1_000.0 / f64::from(iterations);
        eprintln!(
            "relative screen prototype: 6500 native cells, {} source prints, current {:.3} ms/frame, relative {:.3} ms/frame",
            if weighted { 39_000 } else { 6_500 },
            ms(original),
            ms(relative)
        );
    }
}
