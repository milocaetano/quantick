//! Exact full-fold oracle for the synchronous bridge over a published prefix.

use super::*;
use crate::history::Aggression;
use crate::interaction::cluster_aggressions;
use crate::projection::dots::{DotHorizon, fold_dots, native_grouping, window_start};
use crate::projection::tiers::{TierClusters, tier_primitives};
use std::sync::Arc;

mod profiling;

struct Scene {
    config: HeatmapConfig,
    timeline: BarTimeline,
    dots: VolumeDots,
    prices: PriceWindow,
}

impl Scene {
    fn new(now_ms: i64, window_ms: i64, floor: f64) -> Self {
        let mut config = config();
        config.retention_ms = 60_000;
        config.max_aggressions = 100_000;
        config.live_lane.enabled = true;
        config.live_lane.show_aggressions = true;
        config.live_lane.tape_only = true;
        config.volume_dots.enabled = true;
        config.bubbles.min_quantity = floor;
        Self {
            config,
            timeline: BarTimeline::from_bars(
                0,
                &[],
                Some(&bar(0, now_ms)),
                Some(crate::LiveEdge {
                    now_ms,
                    window_ms,
                    reference_ms: window_ms,
                    on_newest_bar: true,
                }),
            )
            .with_full_lane_coverage(),
            dots: VolumeDots {
                native_tape: true,
                tape_window_ms: 100,
                tape_level_ticks: 1,
                candle_level_ticks: 1,
                bars: Vec::new(),
                forming: None,
            },
            prices: PriceWindow::new(dec("90"), dec("160")).unwrap(),
        }
    }

    fn project(
        &self,
        pending: &PendingTape,
        published: Option<&HeatmapProjection>,
    ) -> HeatmapProjection {
        pending.project(
            published,
            &self.config,
            &self.timeline,
            self.prices,
            &self.dots,
        )
    }

    /// Preserve the pre-optimization algorithm: raw suffix first, every
    /// published cell next, one complete canonical fold, then floor and place.
    /// This deliberately never calls PendingTape::project.
    fn reference(
        &self,
        published: Option<&HeatmapProjection>,
        suffix: &[Trade],
        horizon: Option<i64>,
        openings: Vec<i64>,
    ) -> HeatmapProjection {
        let native = native_grouping(&self.config);
        let from = self.timeline.lane_start_ms().unwrap();
        let raw: Vec<_> = suffix
            .iter()
            .filter(|trade| window_start(trade.timestamp_ms, 100) >= from)
            .map(|trade| Aggression {
                agg_id: trade.agg_id,
                timestamp_ms: trade.timestamp_ms,
                price: trade.price,
                quantity: trade.quantity,
                side: trade.side,
                generation: None,
            })
            .collect();
        let facts = published.and_then(|frame| frame.tape_facts.as_deref());
        let mut clusters = cluster_aggressions(&raw, &[], native, 0);
        if let Some(facts) = facts {
            clusters.extend(facts.clusters.iter().cloned());
        }
        clusters.retain(|cluster| window_start(cluster.first_timestamp_ms, 100) >= from);
        let folded = fold_dots(
            clusters,
            true,
            &self.dots,
            native,
            DotHorizon {
                recorded_from_ms: None,
                evicted_through_ms: horizon,
            },
        );
        let floor = self
            .config
            .bubbles
            .min_quantity_decimal()
            .unwrap_or_default();
        let floored_quantity: Decimal = folded
            .iter()
            .filter(|cluster| cluster.quantity < floor)
            .map(|cluster| cluster.quantity)
            .sum();
        let mut expected = published
            .cloned()
            .unwrap_or_else(|| HeatmapProjection::empty(true, native));
        expected.floored_quantity +=
            floored_quantity - facts.map_or(Decimal::ZERO, |facts| facts.floored_quantity);
        expected.tape_facts = Some(Arc::new(TapeFacts {
            clusters: folded.clone(),
            evicted_through_ms: horizon,
            floored_quantity,
            opening_bursts: openings,
        }));
        expected.aggressions.retain(|mark| !mark.live);
        let reference = dot_full_quantity(&self.config);
        expected.aggressions.extend(tier_primitives(
            TierClusters {
                tape: folded
                    .into_iter()
                    .filter(|cluster| cluster.quantity >= floor)
                    .collect(),
                slot: Vec::new(),
                tape_facts: None,
            },
            &self.timeline,
            self.prices,
            reference,
            reference,
            Some(&self.dots),
        ));
        expected.aggressions.sort_by_key(|mark| mark.quantity);
        expected.volume_dots = true;
        expected.live_now_x = self
            .timeline
            .live_now_position()
            .map(|position| position.normalized);
        expected
    }
}

fn print(id: u64, time: i64, price: i64, quantity: &str, side: Side) -> Trade {
    Trade {
        agg_id: id,
        timestamp_ms: time,
        price: Decimal::from(price),
        quantity: dec(quantity),
        side,
    }
}

fn record(pending: &mut PendingTape, scene: &Scene, prints: &[Trade]) {
    for trade in prints {
        pending.record(trade, &scene.config);
    }
}

fn dense_prefix() -> Vec<Trade> {
    (0..6_500)
        .map(|index| {
            print(
                index + 1,
                1_001 + (index / 50) as i64 * 100,
                100 + (index % 50) as i64,
                if index.is_multiple_of(3) {
                    "0.125"
                } else {
                    "2.75"
                },
                if index.is_multiple_of(2) {
                    Side::Buy
                } else {
                    Side::Sell
                },
            )
        })
        .collect()
}

fn dense_suffix() -> Vec<Trade> {
    (0..20)
        .map(|index| {
            print(
                7_000 + index,
                if index < 2 {
                    1_017 + index as i64
                } else {
                    13_917 + index as i64
                },
                100 + (index % 4) as i64,
                "0.375",
                if index.is_multiple_of(2) {
                    Side::Sell
                } else {
                    Side::Buy
                },
            )
        })
        .collect()
}

#[test]
fn a_small_pending_suffix_preserves_the_complete_large_prefix_and_late_keys() {
    let scene = Scene::new(14_099, 15_000, 0.5);
    let prefix = dense_prefix();
    let mut pending = PendingTape::default();
    record(&mut pending, &scene, &prefix);
    let mut published = scene.reference(None, &prefix, None, pending.opening_bursts(None));
    for cluster in &mut Arc::make_mut(published.tape_facts.as_mut().unwrap()).clusters {
        cluster.generation = Some(7);
        cluster.matched_quantity = dec("0.125");
        cluster.liquidity_event_ids = vec![cluster.agg_id + 10_000];
    }
    assert_eq!(published.tape_facts.as_ref().unwrap().clusters.len(), 6_500);
    // Non-live state and its floor contribution belong to the published frame.
    let mut slot = published.aggressions[0].clone();
    slot.live = false;
    published.aggressions.push(slot);
    published.floored_quantity += dec("13");
    pending.acknowledge(prefix.len() as u64);
    let suffix = dense_suffix();
    record(&mut pending, &scene, &suffix);
    let actual = scene.project(&pending, Some(&published));
    let expected = scene.reference(
        Some(&published),
        &suffix,
        None,
        pending.opening_bursts(Some(&published)),
    );
    assert_eq!(
        actual, expected,
        "every primitive field, fact, tie order and floor total must match"
    );
    assert_eq!(actual.tape_facts.as_ref().unwrap().clusters.len(), 6_500);
    assert_eq!(
        pending.len(),
        20,
        "projection never acknowledges its own suffix"
    );
}

#[test]
fn successive_pending_frames_keep_exact_same_window_facts_and_restarted_ids() {
    let scene = Scene::new(1_199, 15_000, 1.0);
    let prefix = [print(90, 1_017, 100, "0.625", Side::Buy)];
    let mut pending = PendingTape::default();
    record(&mut pending, &scene, &prefix);
    let published = scene.reference(None, &prefix, None, pending.opening_bursts(None));
    pending.acknowledge(1);
    let mut suffix = vec![print(1, 1_009, 100, "0.375", Side::Sell)];
    for next in [None, Some(print(1, 1_099, 100, "0.25", Side::Sell))] {
        if let Some(next) = next {
            suffix.push(next);
        }
        pending.record(suffix.last().unwrap(), &scene.config);
        let actual = scene.project(&pending, Some(&published));
        let expected = scene.reference(
            Some(&published),
            &suffix,
            None,
            pending.opening_bursts(Some(&published)),
        );
        assert_eq!(actual, expected);
        assert_eq!(
            scene.project(&pending, Some(&published)),
            actual,
            "repeated frames must not accumulate the suffix twice"
        );
    }
    let final_frame = scene.project(&pending, Some(&published));
    let mark = &final_frame.aggressions[0];
    assert_eq!(mark.quantity, dec("1.25"));
    assert_eq!(mark.buy_quantity, dec("0.625"));
    assert_eq!(mark.timestamp_quantity, dec("1288.75"));
    assert_eq!(mark.agg_ids, [1, 90]);
    assert_eq!(
        mark.side,
        Side::Sell,
        "equal sides retain the earliest input's side"
    );
    assert_eq!(final_frame.floored_quantity, Decimal::ZERO);
    pending.acknowledge(3);
    assert!(pending.is_empty());
    let after_receipt = scene.project(&pending, Some(&final_frame));
    assert_eq!(
        after_receipt, final_frame,
        "a frame-bound receipt preserves complete native facts"
    );
}

#[test]
fn pending_projection_applies_lane_and_both_eviction_horizons_before_flooring() {
    let mut scene = Scene::new(1_299, 15_000, 10.0);
    scene.config.max_aggressions = 2;
    let prefix = [
        print(1, 1_001, 100, "6", Side::Buy),
        print(2, 1_101, 110, "6", Side::Sell),
    ];
    let mut pending = PendingTape::default();
    record(&mut pending, &scene, &prefix);
    let published = scene.reference(None, &prefix, None, pending.opening_bursts(None));
    pending.acknowledge(2);
    let suffix = [print(3, 1_201, 120, "12", Side::Buy)];
    record(&mut pending, &scene, &suffix);
    let actual = scene.project(&pending, Some(&published));
    assert_eq!(
        actual,
        scene.reference(
            Some(&published),
            &suffix,
            Some(1_001),
            pending.opening_bursts(Some(&published))
        )
    );
    assert_eq!(actual.tape_facts.as_ref().unwrap().clusters.len(), 2);
    assert_eq!(actual.floored_quantity, dec("6"));

    let mut byte_evicted = published.clone();
    Arc::make_mut(byte_evicted.tape_facts.as_mut().unwrap()).evicted_through_ms = Some(1_150);
    let actual = scene.project(&pending, Some(&byte_evicted));
    assert_eq!(
        actual,
        scene.reference(
            Some(&byte_evicted),
            &suffix,
            Some(1_150),
            pending.opening_bursts(Some(&byte_evicted))
        )
    );
    assert_eq!(actual.tape_facts.as_ref().unwrap().clusters.len(), 1);
    assert_eq!(actual.floored_quantity, Decimal::ZERO);

    let expired = Scene::new(16_201, 15_000, 10.0);
    let actual = expired.project(&pending, Some(&byte_evicted));
    assert_eq!(
        actual,
        expired.reference(
            Some(&byte_evicted),
            &suffix,
            Some(1_150),
            pending.opening_bursts(Some(&byte_evicted))
        )
    );
    assert!(actual.tape_facts.as_ref().unwrap().clusters.is_empty());
    assert!(actual.aggressions.is_empty());
    assert_eq!(actual.floored_quantity, Decimal::ZERO);
}

#[test]
fn pending_epoch_reset_drops_old_suffix_facts_even_when_venue_ids_restart() {
    let scene = Scene::new(1_199, 15_000, 0.0);
    let mut pending = PendingTape::default();
    record(
        &mut pending,
        &scene,
        &[print(1, 1_017, 100, "99", Side::Buy)],
    );
    let old_epoch = pending.epoch();
    let _ = scene.project(&pending, None);
    assert_ne!(pending.reset(), old_epoch);
    pending.clear_opening_bursts();
    let fresh = [print(1, 101, 120, "0.125", Side::Sell)];
    record(&mut pending, &scene, &fresh);
    let actual = scene.project(&pending, None);
    assert_eq!(actual, scene.reference(None, &fresh, None, vec![100]));
    assert_eq!(actual.aggressions.len(), 1);
    assert_eq!(actual.aggressions[0].quantity, dec("0.125"));
}

#[test]
#[ignore = "opt-in measurement; timings are reported without a pass threshold"]
fn pending_projection_cost_for_6500_published_cells_and_20_prints() {
    use std::hint::black_box;
    use std::time::Instant;

    let scene = Scene::new(14_099, 15_000, 0.5);
    let prefix = dense_prefix();
    let suffix = dense_suffix();
    let mut pending = PendingTape::default();
    record(&mut pending, &scene, &prefix);
    let published = scene.reference(None, &prefix, None, pending.opening_bursts(None));
    pending.acknowledge(prefix.len() as u64);
    record(&mut pending, &scene, &suffix);
    let expected = scene.reference(
        Some(&published),
        &suffix,
        None,
        pending.opening_bursts(Some(&published)),
    );
    assert_eq!(scene.project(&pending, Some(&published)), expected);
    let iterations = 40;
    let started = Instant::now();
    for _ in 0..iterations {
        black_box(scene.project(black_box(&pending), Some(black_box(&published))));
    }
    let elapsed = started.elapsed();
    assert_eq!(scene.project(&pending, Some(&published)), expected);
    eprintln!(
        "pending projection: 6500 published cells, 20 suffix prints, {iterations} frames, {:.3} ms/frame",
        elapsed.as_secs_f64() * 1_000.0 / f64::from(iterations)
    );
}
