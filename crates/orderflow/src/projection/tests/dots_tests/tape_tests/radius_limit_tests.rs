//! Test-only radius guard experiment. Production remains the frozen oracle.

use super::*;

#[derive(Default)]
struct Counts {
    candidates: usize,
    hypots: usize,
}

/// Preserve the current arithmetic and iteration order. The experimental
/// branch rejects only an axis distance exceeding twice the current bound:
/// hypot cannot improve `share` there, with ample floating-point margin.
fn measured_limit(
    marks: &[AggressionPrimitive],
    sizing: DotSizing,
    bubbles: &BubbleStyle,
    lane: &LiveLaneStyle,
    geometry: TapeDotGeometry,
    guarded: bool,
) -> (f32, Counts) {
    let shown: Vec<_> = marks.iter().filter(|mark| geometry.visible(mark)).collect();
    let full = sizing.full_quantity(shown.iter().copied(), true);
    let radii: Vec<_> = shown
        .iter()
        .map(|mark| sizing.radius(bubbles, lane, mark, full))
        .collect();
    let mut neighbours = Neighbours {
        cells: BTreeMap::new(),
        span_px: f64::from(bubbles.max_radius.max(f32::MIN_POSITIVE)) * 2.0,
        geometry,
    };
    let mut share = 1.0_f64;
    let mut counts = Counts::default();
    for (index, mark) in shown.iter().enumerate() {
        if guarded && share == 0.0 {
            break;
        }
        let (x, y) = geometry.position(mark);
        for other in neighbours.candidates(mark) {
            counts.candidates += 1;
            let (other_x, other_y) = geometry.position(shown[other]);
            let clearance = f64::from(
                radii[index] + radii[other]
                    - SMALLER_DOT_OVERLAP_SHARE * radii[index].min(radii[other]),
            );
            if clearance > 0.0 {
                let dx = x - other_x;
                let dy = y - other_y;
                let conservative_bound = 2.0 * clearance * share;
                if guarded && (dx.abs() > conservative_bound || dy.abs() > conservative_bound) {
                    continue;
                }
                counts.hypots += 1;
                share = share.min(dx.hypot(dy) / clearance);
            }
        }
        neighbours.insert(index, mark);
    }
    (bubbles.max_radius * share.clamp(0.0, 1.0) as f32, counts)
}

fn mark(index: usize, x: f64, y: f64, quantity: i64) -> AggressionPrimitive {
    let quantity = Decimal::from(quantity);
    AggressionPrimitive {
        agg_id: index as u64 + 1,
        agg_ids: vec![index as u64 + 1],
        generation: None,
        side: AggressorSide::Buy,
        consumed_side: RestingSide::Ask,
        quantity,
        buy_share: 1.0,
        live: true,
        price_bucket: Decimal::from(187_000),
        price_span: Decimal::from(5),
        price: Decimal::from(187_000),
        trade_count: 1,
        first_timestamp_ms: index as i64,
        last_timestamp_ms: index as i64,
        timestamp_quantity: Decimal::from(index) * quantity,
        matched_quantity: Decimal::ZERO,
        buy_quantity: quantity,
        matched_fraction: 0.0,
        liquidity_event_ids: Vec::new(),
        x,
        y,
        size: 0.0,
        folded_marks: 0,
    }
}

fn fixture(dwarfed: bool) -> Vec<AggressionPrimitive> {
    let count = if dwarfed { 3_731 } else { 700 };
    (0..count)
        .map(|index| {
            let (x, y) = if dwarfed {
                let x = (index as f64 + 0.25) / count as f64;
                let y = 0.15 + 0.7 * x + (index as f64 * 0.73).sin() * 0.04;
                (x, y)
            } else {
                let x = (index % 35) as f64 + 0.35 + (index as f64 * 0.37).sin() * 0.15;
                let y = (index / 35) as f64 + 0.35 + (index as f64 * 0.61).cos() * 0.15;
                (x / 35.0, y / 20.0)
            };
            let quantity = if dwarfed && index == 0 {
                74_365
            } else if dwarfed {
                1 + (index * 17 % 35) as i64
            } else {
                1 + (index * 17 % 1_000) as i64
            };
            mark(index, x, y, quantity)
        })
        .collect()
}

fn geometry() -> TapeDotGeometry {
    TapeDotGeometry {
        left_x: 0.0,
        right_x: 1.0,
        width_px: 789.0,
        height_px: 646.0,
    }
}

fn style() -> (BubbleStyle, LiveLaneStyle, DotSizing) {
    (
        BubbleStyle {
            max_radius: 18.0,
            ..BubbleStyle::default()
        },
        LiveLaneStyle {
            tape_only: true,
            ..LiveLaneStyle::default()
        },
        DotSizing {
            tape_column_px: 1.0,
            candle_column_px: 1.0,
            px_per_price: 1.0,
            typed_full: None,
        },
    )
}

fn check(marks: &[AggressionPrimitive], geometry: TapeDotGeometry, radius: f32) {
    let (mut bubbles, lane, sizing) = style();
    bubbles.max_radius = radius;
    let production = tape_radius_limit(marks, sizing, &bubbles, &lane, geometry);
    let (oracle, _) = measured_limit(marks, sizing, &bubbles, &lane, geometry, false);
    let (guarded, _) = measured_limit(marks, sizing, &bubbles, &lane, geometry, true);
    assert_eq!(production.to_bits(), oracle.to_bits(), "frozen oracle");
    assert_eq!(guarded.to_bits(), oracle.to_bits(), "exact radius bits");
}

#[test]
fn radius_guard_preserves_dense_tiny_and_ordinary_disc_caps_exactly() {
    for dwarfed in [false, true] {
        let mut marks = fixture(dwarfed);
        check(&marks, geometry(), 18.0);
        let (bubbles, lane, sizing) = style();
        let (_, baseline) = measured_limit(&marks, sizing, &bubbles, &lane, geometry(), false);
        let (_, guarded) = measured_limit(&marks, sizing, &bubbles, &lane, geometry(), true);
        assert!(
            guarded.hypots < baseline.hypots,
            "fixture exercises rejection"
        );
        marks.reverse();
        check(&marks, geometry(), 18.0);
    }
}

#[test]
fn radius_guard_preserves_zero_touching_clipping_and_extreme_geometry() {
    let mut marks = vec![mark(0, 0.25, 0.5, 1), mark(1, 0.75, 0.5, 1)];
    let touching = TapeDotGeometry {
        width_px: 68.4,
        ..geometry()
    };
    for width_px in [
        touching.width_px.next_down(),
        touching.width_px,
        touching.width_px.next_up(),
    ] {
        check(
            &marks,
            TapeDotGeometry {
                width_px,
                ..touching
            },
            18.0,
        );
    }
    marks[1].x = marks[0].x;
    check(&marks, geometry(), 18.0);
    check(&marks, geometry(), 0.0);
    marks.extend([
        mark(2, f64::NAN, 0.5, 1),
        mark(3, 1.1, 0.5, 1),
        mark(4, 0.5, -0.1, 1),
    ]);
    check(&marks, geometry(), 18.0);
    check(&[], geometry(), 18.0);
    for (width_px, height_px) in [(1e12, 1e-6), (1e-6, 1e12), (1e-30, 1e-30)] {
        let marks = [mark(0, 0.25, 0.5, 1), mark(1, 0.75, 0.5, 4)];
        check(
            &marks,
            TapeDotGeometry {
                width_px,
                height_px,
                ..geometry()
            },
            18.0,
        );
    }
}

#[test]
#[ignore = "test-only radius guard experiment; synthetic post-merge layouts, no timing threshold"]
fn radius_guard_cost_for_3731_tiny_and_700_ordinary_discs() {
    use std::hint::black_box;
    use std::time::{Duration, Instant};
    let (bubbles, lane, sizing) = style();
    for dwarfed in [true, false] {
        let marks = fixture(dwarfed);
        let geometry = geometry();
        check(&marks, geometry, 18.0);
        let mut elapsed = [Duration::ZERO; 2];
        let samples = 30;
        for index in 0..samples + 3 {
            for guarded in [index % 2 == 0, index % 2 != 0] {
                let start = Instant::now();
                black_box(measured_limit(
                    black_box(&marks),
                    sizing,
                    &bubbles,
                    &lane,
                    geometry,
                    guarded,
                ));
                if index >= 3 {
                    elapsed[usize::from(guarded)] += start.elapsed();
                }
            }
        }
        let (radius, original) = measured_limit(&marks, sizing, &bubbles, &lane, geometry, false);
        let (_, guarded) = measured_limit(&marks, sizing, &bubbles, &lane, geometry, true);
        eprintln!(
            "TAPE_RADIUS_GUARD synthetic_post_merge={} opening_dwarfed={dwarfed} samples={samples} warmups=3 radius={radius:?} baseline_ms={:.3} guarded_ms={:.3} baseline_candidates={} guarded_candidates={} baseline_hypots={} guarded_hypots={}; includes sizing/index/query/hypot and counters, excludes cloning/positioning/merging/UI/GPU",
            marks.len(),
            elapsed[0].as_secs_f64() * 1000.0 / f64::from(samples),
            elapsed[1].as_secs_f64() * 1000.0 / f64::from(samples),
            original.candidates,
            guarded.candidates,
            original.hypots,
            guarded.hypots
        );
    }
}
