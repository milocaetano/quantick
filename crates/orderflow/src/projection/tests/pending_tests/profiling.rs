//! Opt-in cost breakdown, with unchanged production output as the oracle.

use super::*;
use crate::interaction::sort_clusters;
use rust_decimal::prelude::ToPrimitive as _;
use std::hint::black_box;
use std::time::Instant;

fn measure<T>(label: &str, mut run: impl FnMut() -> T) {
    black_box(run());
    let iterations = 40;
    let started = Instant::now();
    for _ in 0..iterations {
        black_box(run());
    }
    eprintln!(
        "pending stage: {label}, {iterations} frames, {:.3} ms/frame",
        started.elapsed().as_secs_f64() * 1_000.0 / f64::from(iterations)
    );
}

#[test]
#[ignore = "opt-in stage profile; overlapping stages are not additive and have no timing threshold"]
fn pending_projection_stage_costs_for_complete_native_cells() {
    // Keep every native cell visible so fact and primitive clone counts match.
    let scene = Scene::new(14_099, 15_000, 0.0);
    let prefix = dense_prefix();
    let suffix = dense_suffix();
    let mut pending = PendingTape::default();
    record(&mut pending, &scene, &prefix);
    let published = scene.reference(None, &prefix, None, pending.opening_bursts(None));
    pending.acknowledge(prefix.len() as u64);
    record(&mut pending, &scene, &suffix);
    let actual = scene.project(&pending, Some(&published));
    let expected = scene.reference(
        Some(&published),
        &suffix,
        None,
        pending.opening_bursts(Some(&published)),
    );
    assert_eq!(actual, expected);
    let facts = actual.tape_facts.as_ref().unwrap();
    assert_eq!(facts.clusters.len(), 6_500);
    assert_eq!(actual.aggressions.len(), 6_500);
    let reference = dot_full_quantity(&scene.config);

    measure("complete pending projection", || {
        scene.project(&pending, Some(&published))
    });
    measure("clone native facts", || facts.clusters.clone());
    measure("clone native primitives", || actual.aggressions.clone());
    measure("clone and canonical-sort native facts", || {
        let mut clusters = facts.clusters.clone();
        sort_clusters(&mut clusters);
        clusters
    });
    measure("clone and quantity-sort native primitives", || {
        let mut marks = actual.aggressions.clone();
        marks.sort_by_key(|mark| mark.quantity);
        marks
    });
    measure("clone and construct TapeFacts", || {
        Arc::new(TapeFacts {
            clusters: facts.clusters.clone(),
            evicted_through_ms: facts.evicted_through_ms,
            floored_quantity: facts.floored_quantity,
            opening_bursts: facts.opening_bursts.clone(),
            floor: facts.floor,
            seal: facts.seal.clone(),
        })
    });
    measure("clone facts and derive all tier primitives", || {
        tier_primitives(
            TierClusters {
                tape: facts.clusters.clone(),
                slot: Vec::new(),
                tape_facts: None,
            },
            &scene.timeline,
            scene.prices,
            reference,
            reference,
            Some(&scene.dots),
        )
    });

    let (from, now) = scene.timeline.lane_bounds_ms().unwrap();
    let right = scene.timeline.live_now_position().unwrap().normalized;
    let left = scene
        .timeline
        .locate_in_lane_clamped(from)
        .unwrap()
        .normalized;
    let duration = Decimal::from(now.saturating_sub(from).max(1));
    let from = Decimal::from(from);
    let now_window = window_start(now, scene.dots.tape_window_ms);
    measure("exact time moments and x coordinates only", || {
        for cluster in &facts.clusters {
            let x = if window_start(cluster.last_timestamp_ms, scene.dots.tape_window_ms)
                == now_window
            {
                right
            } else {
                let mean = cluster.timestamp_quantity / cluster.quantity;
                let fraction = ((mean - from) / duration).to_f64().unwrap();
                left + (right - left) * fraction.clamp(0.0, 1.0)
            };
            black_box(x);
        }
    });
    measure("exact price to y coordinates only", || {
        for cluster in &facts.clusters {
            black_box(scene.prices.y_unclamped(cluster.price));
        }
    });
    measure("area sizes only", || {
        for cluster in &facts.clusters {
            black_box(normalized_area_size(cluster.quantity, reference));
        }
    });
    measure("buy shares and matched fractions only", || {
        for cluster in &facts.clusters {
            black_box((cluster.buy_share(), cluster.matched_fraction()));
        }
    });
    assert_eq!(scene.project(&pending, Some(&published)), expected);
}
